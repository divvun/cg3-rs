//! The library's tracing events: sent to the sink the Swift package installs,
//! as they happen, at the level it sets and above, and kept as the warnings of
//! the run going on on their thread.
//!
//! One subscriber takes every event: the global default of the library's own
//! copy of tracing, which the static library's prelink keeps private to it
//! ([dec:cg3:swift-package]), so another Rust library in the same app keeps
//! its own. A scoped subscriber would not do: tracing refuses to set one while
//! it dispatches an event on the same thread, and the caller's handler may
//! call into the library from inside one.

use std::cell::RefCell;
use std::fmt;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Once, PoisonError, RwLock};

use tracing::field::{Field, Visit};
use tracing::level_filters::LevelFilter;
use tracing::span::{Attributes, Id, Record};
use tracing::subscriber::Interest;
use tracing::{Dispatch, Event, Level, Metadata, Subscriber};

/// The level of an event, and of the least severe events sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl LogLevel {
    fn of(level: Level) -> LogLevel {
        match level {
            Level::ERROR => LogLevel::Error,
            Level::WARN => LogLevel::Warn,
            Level::INFO => LogLevel::Info,
            Level::DEBUG => LogLevel::Debug,
            Level::TRACE => LogLevel::Trace,
        }
    }

    /// The number `LEVEL` holds for it: 1 for errors to 5 for trace, so that 0
    /// can be off.
    fn number(self) -> u8 {
        self as u8 + 1
    }
}

// [spec:cg3:req:swift.logging]
/// Where the events go: the Swift package's unified-log writer and the
/// caller's handler. It is called on the thread that called into the library.
#[uniffi::export(with_foreign)]
pub trait LogSink: Send + Sync {
    fn log(&self, level: LogLevel, target: String, message: String);
}

static SINK: RwLock<Option<Arc<dyn LogSink>>> = RwLock::new(None);
static LEVEL: AtomicU8 = AtomicU8::new(0);

thread_local! {
    /// The warnings of each run going on on this thread, the innermost last: a
    /// handler may run the library again from inside a run.
    static RUNS: RefCell<Vec<Vec<String>>> = const { RefCell::new(Vec::new()) };
}

// [spec:cg3:req:swift.logging]
/// Sends the events at the level `set_log_level` sets, and above, to `sink`
/// from here on.
#[uniffi::export]
pub fn set_log_sink(sink: Arc<dyn LogSink>) {
    install();
    *SINK.write().unwrap_or_else(PoisonError::into_inner) = Some(sink);
}

// [spec:cg3:req:swift.logging]
/// Sends the events at `level` and above from here on, and none for `None`.
/// Tracing caches which callsites are enabled, so the cache is rebuilt
/// against the new level.
#[uniffi::export]
pub fn set_log_level(level: Option<LogLevel>) {
    install();
    LEVEL.store(level.map_or(0, LogLevel::number), Ordering::Relaxed);
    tracing::callsite::rebuild_interest_cache();
}

fn sent() -> LevelFilter {
    match LEVEL.load(Ordering::Relaxed) {
        0 => LevelFilter::OFF,
        1 => LevelFilter::ERROR,
        2 => LevelFilter::WARN,
        3 => LevelFilter::INFO,
        4 => LevelFilter::DEBUG,
        _ => LevelFilter::TRACE,
    }
}

/// Makes the subscriber the library's global default, once.
fn install() {
    static INSTALLED: Once = Once::new();
    INSTALLED.call_once(|| {
        // Only this library sets it, and the prelink keeps its tracing its own.
        let _ = tracing::dispatcher::set_global_default(Dispatch::new(Log));
    });
}

// [spec:cg3:req:swift.logging]
/// Runs `f`, sending its events on as they happen.
pub fn logging<T>(f: impl FnOnce() -> T) -> T {
    install();
    f()
}

/// Pops the innermost run's warnings, however its call ends: UniFFI catches
/// a panic and goes on using the thread.
struct Run;

impl Drop for Run {
    fn drop(&mut self) {
        let _ = RUNS.try_with(|runs| runs.borrow_mut().pop());
    }
}

// [spec:cg3:req:swift.logging]
/// Runs `f`, and returns what it returned with the message of every warning
/// and error logged while it ran, in order, whatever the level sent. The
/// library logs an error it carries on past (a malformed reading in a stream,
/// say) at error level, so those are kept too.
pub fn collecting<T>(f: impl FnOnce() -> T) -> (T, Vec<String>) {
    install();
    RUNS.with_borrow_mut(|runs| runs.push(Vec::new()));
    let run = Run;
    let value = f();
    let warnings = RUNS.with_borrow_mut(|runs| runs.last_mut().map(std::mem::take));
    drop(run);
    (value, warnings.unwrap_or_default())
}

/// The library's one subscriber. Spans are never enabled, so none is ever made.
struct Log;

impl Log {
    /// Every warning and error is kept for its run, and every event at the
    /// level sent goes to the sink.
    fn wants(metadata: &Metadata<'_>) -> bool {
        metadata.is_event() && *metadata.level() <= sent().max(LevelFilter::WARN)
    }
}

impl Subscriber for Log {
    fn register_callsite(&self, metadata: &'static Metadata<'static>) -> Interest {
        if Log::wants(metadata) {
            Interest::always()
        } else {
            Interest::never()
        }
    }

    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        Log::wants(metadata)
    }

    fn max_level_hint(&self) -> Option<LevelFilter> {
        Some(sent().max(LevelFilter::WARN))
    }

    fn new_span(&self, _: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }

    fn record(&self, _: &Id, _: &Record<'_>) {}

    fn record_follows_from(&self, _: &Id, _: &Id) {}

    fn event(&self, event: &Event<'_>) {
        let metadata = event.metadata();
        let level = *metadata.level();
        let mut text = Text::default();
        event.record(&mut text);
        if level <= Level::WARN {
            let _ = RUNS.try_with(|runs| {
                if let Some(run) = runs.borrow_mut().last_mut() {
                    run.push(text.message.clone());
                }
            });
        }
        if level > sent() {
            return;
        }
        // Cloned out, so that no lock is held while the sink runs: the
        // caller's handler may call into the library again.
        let sink = SINK.read().unwrap_or_else(PoisonError::into_inner).clone();
        if let Some(sink) = sink {
            sink.log(
                LogLevel::of(level),
                metadata.target().to_owned(),
                text.rendered(),
            );
        }
    }

    fn enter(&self, _: &Id) {}

    fn exit(&self, _: &Id) {}
}

#[derive(Default)]
struct Text {
    message: String,
    fields: String,
}

impl Text {
    /// The message followed by the event's other fields as `name=value`.
    fn rendered(self) -> String {
        if self.message.is_empty() {
            self.fields.trim_start().to_owned()
        } else {
            self.message + &self.fields
        }
    }
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
    use std::sync::Mutex;

    use super::*;

    /// A sink that keeps what it is sent and, for an event naming `again`,
    /// runs the library's logging again from inside the handler.
    #[derive(Default)]
    struct Kept {
        events: Mutex<Vec<String>>,
        nested: Mutex<Vec<Vec<String>>>,
    }

    impl LogSink for Kept {
        fn log(&self, level: LogLevel, target: String, message: String) {
            if message == "again" {
                let ((), warnings) = collecting(|| tracing::warn!("inside"));
                self.nested.lock().unwrap().push(warnings);
            }
            self.events
                .lock()
                .unwrap()
                .push(format!("{level:?} {target}: {message}"));
        }
    }

    impl Kept {
        fn taken(&self) -> Vec<String> {
            std::mem::take(&mut *self.events.lock().unwrap())
        }
    }

    #[test]
    fn levels_number_from_one() {
        assert_eq!(LogLevel::Error.number(), 1);
        assert_eq!(LogLevel::Trace.number(), 5);
        assert_eq!(LogLevel::of(Level::WARN), LogLevel::Warn);
    }

    // The sink, the level and the subscriber are global, so this one test is
    // the only one that sets them.
    // [spec:cg3:req:swift.logging/test]
    #[test]
    fn runs_keep_their_own_warnings() {
        let target = "cg3_swift::log::tests";
        let kept = Arc::new(Kept::default());
        set_log_sink(kept.clone());
        set_log_level(None);
        let ((), warnings) = collecting(|| {
            tracing::warn!("careful");
            tracing::error!(line = 3, "broken");
            tracing::info!("chatter");
        });
        assert_eq!(warnings, ["careful", "broken"]);
        assert!(kept.taken().is_empty(), "off sends nothing");

        set_log_level(Some(LogLevel::Info));
        let ((), warnings) = collecting(|| {
            tracing::warn!("careful");
            tracing::info!(count = 7, "chatter");
            tracing::debug!("noise");
        });
        assert_eq!(warnings, ["careful"]);
        assert_eq!(
            kept.taken(),
            [
                format!("Warn {target}: careful"),
                format!("Info {target}: chatter count=7")
            ]
        );

        let ((), outer) = collecting(|| {
            tracing::warn!("before");
            tracing::warn!("again");
            tracing::warn!("after");
        });
        assert_eq!(outer, ["before", "again", "after"]);
        assert_eq!(*kept.nested.lock().unwrap(), [vec!["inside".to_owned()]]);

        let outside = logging(|| {
            tracing::warn!("logged");
            7
        });
        assert_eq!(outside, 7);
        assert!(kept.taken().contains(&format!("Warn {target}: logged")));
        set_log_level(Some(LogLevel::Warn));
    }
}
