use std::sync::{Arc, LazyLock, RwLock};

use tracing_subscriber::Layer;

/// Entries kept; older ones are dropped
const MAX_ENTRIES: usize = 10000;

/// One captured log event
#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: String,
    pub level: String,
    pub message: String,
    /// Structured fields of the event, in the order it declared them
    pub fields: Vec<(String, String)>,
}

impl LogEntry {
    /// Value of a structured field
    pub fn field(&self, name: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(field, _)| field == name)
            .map(|(_, value)| value.as_str())
    }
}

/// Log entries shared between the capture layer and the log screen
#[derive(Clone, Default)]
pub struct LogBuffer {
    inner: Arc<RwLock<Inner>>,
}

#[derive(Default)]
struct Inner {
    entries: Vec<LogEntry>,
    /// Counts the changes to `entries`
    revision: u64,
}

/// The buffer of the running application
static GLOBAL: LazyLock<LogBuffer> = LazyLock::new(LogBuffer::default);

impl LogBuffer {
    /// The buffer the application logs into and the log screen shows
    pub fn global() -> Self {
        GLOBAL.clone()
    }

    fn push(&self, entry: LogEntry) {
        if let Ok(mut inner) = self.inner.write() {
            inner.entries.push(entry);
            let len = inner.entries.len();
            if len > MAX_ENTRIES {
                inner.entries.drain(0..len - MAX_ENTRIES);
            }
            inner.revision += 1;
        }
    }

    /// All entries, oldest first
    pub fn entries(&self) -> Vec<LogEntry> {
        self.inner
            .read()
            .map(|inner| inner.entries.clone())
            .unwrap_or_default()
    }

    /// A number that changes whenever the entries do, so a reader can tell
    /// whether what it made of them is still current
    pub fn revision(&self) -> u64 {
        self.inner
            .read()
            .map(|inner| inner.revision)
            .unwrap_or_default()
    }

    /// Remove all entries
    pub fn clear(&self) {
        if let Ok(mut inner) = self.inner.write() {
            inner.entries.clear();
            inner.revision += 1;
        }
    }
}

/// Tracing layer that stores every event in a [`LogBuffer`]
pub struct LogCaptureLayer {
    buffer: LogBuffer,
}

impl LogCaptureLayer {
    pub fn new(buffer: LogBuffer) -> Self {
        Self { buffer }
    }
}

impl<S> Layer<S> for LogCaptureLayer
where
    S: tracing::Subscriber,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let mut visitor = EventVisitor::default();
        event.record(&mut visitor);

        self.buffer.push(LogEntry {
            timestamp: jiff::ZonedDateTime::now()
                .strftime("%H:%M:%S%.3f")
                .to_string(),
            level: event.metadata().level().to_string(),
            message: visitor.message,
            fields: visitor.fields,
        });
    }
}

/// Collects the message and the fields of a tracing event
#[derive(Default)]
struct EventVisitor {
    message: String,
    fields: Vec<(String, String)>,
}

impl EventVisitor {
    fn record(&mut self, field: &tracing::field::Field, value: String) {
        match field.name() {
            "message" => self.message = value,
            // Where a record of the `log` crate came from, added by the
            // bridge to tracing: not something the event says
            name if name.starts_with("log.") => {}
            name => self.fields.push((name.to_string(), value)),
        }
    }
}

impl tracing::field::Visit for EventVisitor {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        // As it is: the `Debug` form would put it in quotes
        self.record(field, value.to_string());
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.record(field, format!("{value:?}"));
    }
}

#[cfg(test)]
pub use test_support::capture;

/// Capturing the log of one test while other tests log on other threads
#[cfg(test)]
mod test_support {
    use super::{LogBuffer, LogCaptureLayer};
    use std::cell::RefCell;
    use std::sync::Once;
    use tracing_subscriber::{Layer, layer::SubscriberExt};

    thread_local! {
        /// The layer of the capture in progress on this thread
        static CAPTURING: RefCell<Option<LogCaptureLayer>> = const { RefCell::new(None) };
    }

    /// Hands every event to the layer its own thread is capturing with.
    ///
    /// It is installed once, as the global subscriber of the test binary. A
    /// subscriber per test (`tracing::subscriber::set_default`) loses events:
    /// tracing caches per call site whether anyone listens, and a test that
    /// does not capture can fill that cache with "no one" for all threads.
    struct PerThread;

    impl<S: tracing::Subscriber> Layer<S> for PerThread {
        fn on_event(
            &self,
            event: &tracing::Event<'_>,
            ctx: tracing_subscriber::layer::Context<'_, S>,
        ) {
            CAPTURING.with_borrow(|capturing| {
                if let Some(layer) = capturing {
                    layer.on_event(event, ctx);
                }
            });
        }
    }

    /// Ends the capture when dropped
    pub struct CaptureGuard;

    impl Drop for CaptureGuard {
        fn drop(&mut self) {
            CAPTURING.set(None);
        }
    }

    /// Capture what the current thread logs, at every level, from now until
    /// the guard is dropped. The events go through a [`LogCaptureLayer`], as
    /// in the application.
    pub fn capture() -> (LogBuffer, CaptureGuard) {
        static INSTALL: Once = Once::new();
        INSTALL.call_once(|| {
            tracing::subscriber::set_global_default(tracing_subscriber::registry().with(PerThread))
                .expect("no other subscriber in tests");
            // Call sites registered while the subscriber was being installed
            tracing::callsite::rebuild_interest_cache();
        });

        let buffer = LogBuffer::default();
        CAPTURING.set(Some(LogCaptureLayer::new(buffer.clone())));
        (buffer, CaptureGuard)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_event_is_stored_with_its_message_and_its_fields() {
        let (buffer, _guard) = capture();

        tracing::warn!(
            action = "thing.done",
            target_name = %"db/primary",
            items = 3_usize,
            dry_run = false,
            "It is done"
        );

        let entries = buffer.entries();
        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert_eq!(entry.level, "WARN");
        assert_eq!(entry.message, "It is done");
        assert_eq!(entry.field("action"), Some("thing.done"));
        assert_eq!(entry.field("target_name"), Some("db/primary"));
        assert_eq!(entry.field("items"), Some("3"));
        assert_eq!(entry.field("dry_run"), Some("false"));
        assert_eq!(entry.field("message"), None);
    }

    #[test]
    fn a_message_in_quotes_keeps_them() {
        // The quotes used to be stripped as if they were formatting
        let (buffer, _guard) = capture();

        tracing::info!("\"quoted\"");

        assert_eq!(buffer.entries()[0].message, "\"quoted\"");
    }

    #[test]
    fn bookkeeping_of_the_log_crate_bridge_is_not_a_field() {
        // Records from the `log` crate (reqwest, rustls) arrive with
        // `log.target`, `log.file`, ... attached by the bridge
        let (buffer, _guard) = capture();

        tracing::debug!(log.target = "reqwest::connect", log.line = 12, "starting");

        let entry = &buffer.entries()[0];
        assert_eq!(entry.message, "starting");
        assert!(entry.fields.is_empty(), "{:?}", entry.fields);
    }

    #[test]
    fn the_oldest_entries_are_dropped_when_the_buffer_is_full() {
        let (buffer, _guard) = capture();

        for i in 0..MAX_ENTRIES + 2 {
            tracing::info!("entry {i}");
        }

        let entries = buffer.entries();
        assert_eq!(entries.len(), MAX_ENTRIES);
        assert_eq!(entries[0].message, "entry 2");
    }
}
