//! The library's tracing events, kept while a load or a run goes on without
//! the GIL and handed to Python's `logging` once the call has it back.

use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

use pyo3::prelude::*;
use tracing::field::{Field, Visit};
use tracing::level_filters::LevelFilter;
use tracing::span::{Attributes, Id, Record};
use tracing::subscriber::Interest;
use tracing::{Dispatch, Level, Metadata, Subscriber};

/// One event, as it reaches `logging` and a run's warnings.
pub struct Event {
    level: Level,
    target: &'static str,
    message: String,
    fields: String,
}

impl Event {
    /// The message followed by the event's other fields as `name=value`.
    fn rendered(&self) -> String {
        if self.message.is_empty() {
            self.fields.trim_start().to_owned()
        } else {
            format!("{}{}", self.message, self.fields)
        }
    }
}

/// The events a call keeps, and the least level it hands to `logging`.
pub struct Kept {
    threshold: LevelFilter,
    events: Vec<Event>,
}

// [spec:cg3:req:python.logging]
/// The least level the `cg3` logger handles at the start of a call, as
/// `logging.disable` and the logger's effective level decide. Nothing below it
/// is kept.
pub fn threshold(py: Python<'_>) -> PyResult<LevelFilter> {
    let logger = py.import("logging")?.call_method1("getLogger", ("cg3",))?;
    for level in [
        Level::TRACE,
        Level::DEBUG,
        Level::INFO,
        Level::WARN,
        Level::ERROR,
    ] {
        if logger
            .call_method1("isEnabledFor", (python_level(level),))?
            .is_truthy()?
        {
            return Ok(LevelFilter::from_level(level));
        }
    }
    Ok(LevelFilter::OFF)
}

/// `logging`'s number for a level. Python has no trace level, so trace is 5,
/// below `logging.DEBUG`.
fn python_level(level: Level) -> u8 {
    match level {
        Level::ERROR => 40,
        Level::WARN => 30,
        Level::INFO => 20,
        Level::DEBUG => 10,
        Level::TRACE => 5,
    }
}

// [spec:cg3:req:python.logging]
/// Runs `f`, keeping every event at `threshold` or above, and every warning and
/// error whatever the threshold, for the run's warnings.
pub fn collecting<T>(threshold: LevelFilter, f: impl FnOnce() -> T) -> (T, Kept) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let dispatch = Dispatch::new(Collector {
        keep: threshold.max(LevelFilter::WARN),
        events: Arc::clone(&events),
    });
    let value = tracing::dispatcher::with_default(&dispatch, f);
    drop(dispatch);
    let events = std::mem::take(&mut *events.lock().unwrap_or_else(PoisonError::into_inner));
    (value, Kept { threshold, events })
}

impl Kept {
    // [spec:cg3:req:python.logging]
    /// Logs each kept event at the threshold or above, in order, on the logger
    /// named after its target with `::` read as `.`.
    pub fn hand_over(&self, py: Python<'_>) -> PyResult<()> {
        let mut events = self
            .events
            .iter()
            .filter(|e| e.level <= self.threshold)
            .peekable();
        if events.peek().is_none() {
            return Ok(());
        }
        let get_logger = py.import("logging")?.getattr("getLogger")?;
        for event in events {
            get_logger
                .call1((event.target.replace("::", "."),))?
                .call_method1("log", (python_level(event.level), event.rendered()))?;
        }
        Ok(())
    }

    /// The message of every warning and error, in order.
    pub fn warnings(&self) -> Vec<String> {
        self.events
            .iter()
            .filter(|e| e.level <= Level::WARN)
            .map(|e| e.message.clone())
            .collect()
    }
}

/// The subscriber for one call, set as its thread's default for the call
/// alone. Spans are never enabled, so none is ever made.
///
/// Tracing caches one interest per callsite for every live subscriber
/// together, and rebuilds it whenever a subscriber is made, so a call made with
/// a lower threshold than another thread's still sees its events. `event`
/// checks again what it receives, because the cache is shared.
struct Collector {
    keep: LevelFilter,
    events: Arc<Mutex<Vec<Event>>>,
}

impl Collector {
    fn keeps(&self, metadata: &Metadata<'_>) -> bool {
        metadata.is_event() && *metadata.level() <= self.keep
    }
}

impl Subscriber for Collector {
    fn register_callsite(&self, metadata: &'static Metadata<'static>) -> Interest {
        if self.keeps(metadata) {
            Interest::always()
        } else {
            Interest::never()
        }
    }

    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        self.keeps(metadata)
    }

    fn max_level_hint(&self) -> Option<LevelFilter> {
        Some(self.keep)
    }

    fn new_span(&self, _: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }

    fn record(&self, _: &Id, _: &Record<'_>) {}

    fn record_follows_from(&self, _: &Id, _: &Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        let metadata = event.metadata();
        if !self.keeps(metadata) {
            return;
        }
        let mut text = Text::default();
        event.record(&mut text);
        self.events
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(Event {
                level: *metadata.level(),
                target: metadata.target(),
                message: text.message,
                fields: text.fields,
            });
    }

    fn enter(&self, _: &Id) {}

    fn exit(&self, _: &Id) {}
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
