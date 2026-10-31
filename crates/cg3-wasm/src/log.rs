//! The library's tracing events: forwarded to the wrapper's logger at the
//! level it asks for, and, while a run is going on, also kept as that run's
//! warnings.
//!
//! Levels are numbered as the wrapper numbers them: 0 is off, then 1 error,
//! 2 warn, 3 info, 4 debug and 5 trace.

use std::fmt;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use tracing::field::{Field, Visit};
use tracing::level_filters::LevelFilter;
use tracing::span::{Attributes, Id, Record};
use tracing::subscriber::Interest;
use tracing::{Dispatch, Event, Level, Metadata, Subscriber};
use wasm_bindgen::prelude::wasm_bindgen;

/// Where forwarded events go: level, target, message.
pub type Forward = fn(u8, &str, &str);

static LEVEL: AtomicU8 = AtomicU8::new(0);
static FORWARD: OnceLock<Forward> = OnceLock::new();

/// Forwards events at `level` and above to `forward` from here on, outside
/// runs and in them. Only the first call's `forward` is kept.
pub fn install(level: u8, forward: Forward) {
    FORWARD.get_or_init(|| forward);
    // A second install finds the subscriber in place, and it reads the same
    // level and forwarder this one would.
    tracing::dispatcher::set_global_default(Dispatch::new(Log { run: None })).ok();
    set_level(level);
}

/// Forwards events at `level` and above from here on, and nothing below it.
/// Tracing caches which callsites are enabled, so the cache is rebuilt
/// against the new level.
#[wasm_bindgen(js_name = setLogLevel)]
pub fn set_level(level: u8) {
    LEVEL.store(level.min(5), Ordering::Relaxed);
    tracing::callsite::rebuild_interest_cache();
}

/// Runs `f`, and returns what it returned with the message of every warning
/// and error logged while it ran, in order. The library logs an error it
/// carries on past (a malformed reading in a stream, say) at error level, so
/// those are kept too. The events are still forwarded as any others are.
pub fn collecting<T>(f: impl FnOnce() -> T) -> (T, Vec<String>) {
    let run = Arc::new(Mutex::new(Vec::new()));
    let dispatch = Dispatch::new(Log {
        run: Some(Arc::clone(&run)),
    });
    let value = tracing::dispatcher::with_default(&dispatch, f);
    let warnings = std::mem::take(&mut *run.lock().unwrap_or_else(PoisonError::into_inner));
    (value, warnings)
}

const LEVELS: [Level; 5] = [
    Level::ERROR,
    Level::WARN,
    Level::INFO,
    Level::DEBUG,
    Level::TRACE,
];

fn level_number(level: Level) -> u8 {
    LEVELS
        .iter()
        .position(|known| *known == level)
        .map_or(5, |index| index as u8 + 1)
}

fn level_filter(number: u8) -> LevelFilter {
    match number.checked_sub(1) {
        None => LevelFilter::OFF,
        Some(index) => LevelFilter::from_level(LEVELS[usize::from(index).min(4)]),
    }
}

fn forward_filter() -> LevelFilter {
    level_filter(LEVEL.load(Ordering::Relaxed))
}

/// The one subscriber: the global one, which `install` sets, forwards; the
/// one `collecting` sets for a run also keeps the run's warnings. Spans are
/// never enabled, so none is ever made.
///
/// A callsite gets `Interest::never` when neither wants it, so a disabled
/// event costs one cached check. Tracing combines the interest of every live
/// subscriber into one cached value, so `event` checks again what it receives.
struct Log {
    run: Option<Arc<Mutex<Vec<String>>>>,
}

impl Log {
    fn forwards(metadata: &Metadata<'_>) -> bool {
        *metadata.level() <= forward_filter()
    }

    fn collects(&self, metadata: &Metadata<'_>) -> bool {
        self.run.is_some() && *metadata.level() <= Level::WARN
    }

    fn wants(&self, metadata: &Metadata<'_>) -> bool {
        metadata.is_event() && (self.collects(metadata) || Self::forwards(metadata))
    }
}

impl Subscriber for Log {
    fn register_callsite(&self, metadata: &'static Metadata<'static>) -> Interest {
        if self.wants(metadata) {
            Interest::always()
        } else {
            Interest::never()
        }
    }

    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        self.wants(metadata)
    }

    fn max_level_hint(&self) -> Option<LevelFilter> {
        let forwarded = forward_filter();
        Some(match self.run {
            Some(_) => forwarded.max(LevelFilter::WARN),
            None => forwarded,
        })
    }

    fn new_span(&self, _: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }

    fn record(&self, _: &Id, _: &Record<'_>) {}

    fn record_follows_from(&self, _: &Id, _: &Id) {}

    fn event(&self, event: &Event<'_>) {
        let metadata = event.metadata();
        if let Some(run) = self.run.as_ref().filter(|_| self.collects(metadata)) {
            let mut message = Text::default();
            event.record(&mut message);
            run.lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(message.message);
        }
        if let Some(forward) = FORWARD.get().filter(|_| Self::forwards(metadata)) {
            forward(
                level_number(*metadata.level()),
                metadata.target(),
                &render(event),
            );
        }
    }

    fn enter(&self, _: &Id) {}

    fn exit(&self, _: &Id) {}
}

/// An event's message followed by its other fields as `name=value`.
fn render(event: &Event<'_>) -> String {
    let mut text = Text::default();
    event.record(&mut text);
    if text.message.is_empty() {
        text.fields.trim_start().to_owned()
    } else {
        text.message + &text.fields
    }
}

#[derive(Default)]
struct Text {
    message: String,
    fields: String,
}

impl Visit for Text {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message.push_str(value);
        } else {
            self.fields
                .push_str(&format!(" {}={value:?}", field.name()));
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if field.name() == "message" {
            self.message.push_str(&format!("{value:?}"));
        } else {
            self.fields
                .push_str(&format!(" {}={value:?}", field.name()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_round_trip_through_their_numbers() {
        assert_eq!(level_filter(0), LevelFilter::OFF);
        for level in LEVELS {
            assert_eq!(
                level_filter(level_number(level)),
                LevelFilter::from_level(level)
            );
        }
        assert_eq!(level_number(Level::ERROR), 1);
        assert_eq!(level_number(Level::TRACE), 5);
        assert_eq!(level_filter(9), LevelFilter::TRACE);
    }

    static FORWARDED: Mutex<Vec<String>> = Mutex::new(Vec::new());

    fn capture(level: u8, target: &str, message: &str) {
        FORWARDED
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(format!("{level} {target}: {message}"));
    }

    fn forwarded() -> Vec<String> {
        std::mem::take(&mut *FORWARDED.lock().unwrap_or_else(PoisonError::into_inner))
    }

    // The forwarder and the level are global, so this one test is the only
    // one that sets them.
    #[test]
    fn forwarding_follows_the_level_and_runs_keep_warnings() {
        let target = "cg3_wasm::log::tests";
        install(0, capture);
        let ((), warnings) = collecting(|| {
            tracing::warn!("careful");
            tracing::error!(line = 3, "broken");
            tracing::info!("chatter");
        });
        assert_eq!(warnings, ["careful", "broken"]);
        assert!(forwarded().is_empty(), "off forwards nothing");

        set_level(3);
        let ((), warnings) = collecting(|| {
            tracing::warn!("careful");
            tracing::info!("chatter");
            tracing::debug!("noise");
        });
        assert_eq!(warnings, ["careful"]);
        assert_eq!(
            forwarded(),
            [
                format!("2 {target}: careful"),
                format!("3 {target}: chatter")
            ]
        );

        tracing::info!(count = 7, name = "sma", "outside a run");
        tracing::debug!("below the level");
        assert_eq!(
            forwarded(),
            [format!("3 {target}: outside a run count=7 name=\"sma\"")]
        );

        set_level(5);
        tracing::trace!(count = 7);
        assert_eq!(forwarded(), [format!("5 {target}: count=7")]);
        set_level(0);
    }
}
