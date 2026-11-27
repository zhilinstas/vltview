use anyhow::{Context, Result};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use zeroize::Zeroizing;

use crate::vault::audit_log;

/// Default clipboard timeout in seconds
const DEFAULT_CLIPBOARD_TIMEOUT: u64 = 30;

/// The clipboard operations the manager needs. A trait so the clearing
/// logic can be tested without a display server.
trait ClipboardBackend: Send {
    fn get_text(&mut self) -> Result<String>;
    fn set_text(&mut self, text: &str) -> Result<()>;
}

impl ClipboardBackend for arboard::Clipboard {
    fn get_text(&mut self) -> Result<String> {
        Ok(arboard::Clipboard::get_text(self)?)
    }

    fn set_text(&mut self, text: &str) -> Result<()> {
        Ok(arboard::Clipboard::set_text(self, text)?)
    }
}

/// Text this application put on the clipboard and has not yet cleared
struct PendingClear {
    /// Identifies the copy, so an older timer does not clear a newer copy
    id: u64,
    text: Zeroizing<String>,
}

struct Inner {
    backend: Box<dyn ClipboardBackend>,
    pending: Option<PendingClear>,
    next_id: u64,
}

impl Inner {
    /// Clear the clipboard if it still holds the pending text.
    ///
    /// With `only_id` set, nothing happens unless that copy is still the
    /// pending one. Returns whether the clipboard was cleared. If the user
    /// has copied something else in the meantime, their content is left alone.
    fn clear_pending(&mut self, only_id: Option<u64>) -> bool {
        if only_id.is_some_and(|id| self.pending.as_ref().map(|p| p.id) != Some(id)) {
            return false;
        }

        let Some(pending) = self.pending.take() else {
            return false;
        };

        match self.backend.get_text() {
            Ok(current) if current == *pending.text => self.backend.set_text("").is_ok(),
            _ => {
                tracing::debug!("Clipboard not cleared - content changed by user");
                false
            }
        }
    }
}

/// Clipboard manager for system clipboard operations
///
/// Copied text is cleared after a timeout, and also when the manager is
/// dropped, so quitting the application does not leave a secret behind.
pub struct ClipboardManager {
    inner: Arc<Mutex<Inner>>,
}

impl ClipboardManager {
    /// Create a new clipboard manager
    pub fn new() -> Result<Self> {
        let clipboard = arboard::Clipboard::new().context("Failed to initialize clipboard")?;
        Ok(Self::with_backend(Box::new(clipboard)))
    }

    fn with_backend(backend: Box<dyn ClipboardBackend>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                backend,
                pending: None,
                next_id: 0,
            })),
        }
    }

    /// Copy text to clipboard with automatic clearing after timeout
    /// This is a security feature to prevent secrets from persisting in clipboard
    pub fn copy_text(&mut self, text: &str) -> Result<()> {
        self.copy_text_with_timeout(text, DEFAULT_CLIPBOARD_TIMEOUT)
    }

    /// Copy text to clipboard with custom timeout
    ///
    /// Must be called from within a Tokio runtime: the timed clear is a task.
    pub fn copy_text_with_timeout(&mut self, text: &str, timeout_secs: u64) -> Result<()> {
        let id = {
            let mut inner = lock(&self.inner);
            inner
                .backend
                .set_text(text)
                .context("Failed to copy text to clipboard")?;

            let id = inner.next_id;
            inner.next_id += 1;
            inner.pending = Some(PendingClear {
                id,
                text: Zeroizing::new(text.to_string()),
            });
            id
        };

        // Log clipboard operation for security audit trail
        audit_log::log_clipboard_copy(text.len(), timeout_secs);

        // Schedule automatic clearing. The task dies with the runtime, which
        // is why `Drop` clears as well.
        let inner = Arc::clone(&self.inner);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(timeout_secs)).await;

            if lock(&inner).clear_pending(Some(id)) {
                audit_log::log_security_event(
                    "clipboard.auto_cleared",
                    &format!("Clipboard cleared after {}s timeout", timeout_secs),
                );
            }
        });

        Ok(())
    }

    /// Clear the clipboard now if it still holds text copied by this manager
    pub fn clear_pending(&mut self) -> bool {
        lock(&self.inner).clear_pending(None)
    }
}

impl Drop for ClipboardManager {
    fn drop(&mut self) {
        if self.clear_pending() {
            audit_log::log_security_event(
                "clipboard.cleared_on_exit",
                "Clipboard cleared before its timeout because the application exited",
            );
        }
    }
}

impl Default for ClipboardManager {
    fn default() -> Self {
        Self::new().expect("Failed to initialize clipboard")
    }
}

/// Lock the state, recovering from a poisoned mutex: a panic elsewhere must
/// not stop the clipboard from being cleared.
fn lock(inner: &Mutex<Inner>) -> MutexGuard<'_, Inner> {
    inner
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// In-memory clipboard shared between the test and the manager
    #[derive(Clone, Default)]
    struct FakeClipboard(Arc<Mutex<String>>);

    impl FakeClipboard {
        fn content(&self) -> String {
            self.0.lock().unwrap().clone()
        }

        /// Simulate the user copying something in another application
        fn user_copies(&self, text: &str) {
            *self.0.lock().unwrap() = text.to_string();
        }
    }

    impl ClipboardBackend for FakeClipboard {
        fn get_text(&mut self) -> Result<String> {
            Ok(self.content())
        }

        fn set_text(&mut self, text: &str) -> Result<()> {
            self.user_copies(text);
            Ok(())
        }
    }

    fn manager() -> (ClipboardManager, FakeClipboard) {
        let fake = FakeClipboard::default();
        (ClipboardManager::with_backend(Box::new(fake.clone())), fake)
    }

    async fn advance(secs: u64) {
        tokio::time::sleep(Duration::from_secs(secs)).await;
    }

    #[tokio::test(start_paused = true)]
    async fn clears_after_timeout() {
        let (mut clipboard, fake) = manager();

        clipboard.copy_text("s3cret").unwrap();
        assert_eq!(fake.content(), "s3cret");

        advance(29).await;
        assert_eq!(fake.content(), "s3cret");

        advance(2).await;
        assert_eq!(fake.content(), "");
    }

    #[tokio::test(start_paused = true)]
    async fn clears_when_dropped_before_timeout() {
        // Regression: the timed clear died with the runtime, so quitting
        // within 30 seconds left the secret on the clipboard.
        let (mut clipboard, fake) = manager();

        clipboard.copy_text("s3cret").unwrap();
        advance(5).await;
        drop(clipboard);

        assert_eq!(fake.content(), "");
    }

    #[tokio::test(start_paused = true)]
    async fn leaves_content_the_user_copied_since() {
        let (mut clipboard, fake) = manager();

        clipboard.copy_text("s3cret").unwrap();
        fake.user_copies("my own text");

        advance(31).await;
        assert_eq!(fake.content(), "my own text");

        clipboard.copy_text("s3cret").unwrap();
        fake.user_copies("my own text");
        drop(clipboard);
        assert_eq!(fake.content(), "my own text");
    }

    #[tokio::test(start_paused = true)]
    async fn older_timer_does_not_clear_a_newer_copy() {
        let (mut clipboard, fake) = manager();

        clipboard.copy_text("first").unwrap();
        advance(10).await;
        clipboard.copy_text("second").unwrap();

        // The first copy's timer fires at 30s; "second" has 10s left
        advance(21).await;
        assert_eq!(fake.content(), "second");

        advance(10).await;
        assert_eq!(fake.content(), "");
    }

    #[tokio::test(start_paused = true)]
    async fn the_log_keeps_size_and_timeout_but_not_the_text() {
        let (buffer, _guard) = crate::tui::log_buffer::capture();
        let (mut clipboard, _fake) = manager();

        clipboard.copy_text("copied-value-41d9").unwrap();
        advance(31).await;

        let entries = buffer.entries();
        let entry = |action: &str| {
            entries
                .iter()
                .find(|e| e.field("action") == Some(action))
                .unwrap_or_else(|| panic!("no {action} entry in {entries:#?}"))
        };

        let copied = entry("clipboard.copy");
        assert_eq!(copied.field("bytes"), Some("17"));
        assert_eq!(copied.field("timeout_secs"), Some("30"));

        let cleared = entry("clipboard.auto_cleared");
        assert_eq!(cleared.level, "WARN");
        assert!(cleared.message.contains("30s"), "{cleared:?}");

        for entry in &entries {
            let text = format!("{entry:?}");
            assert!(!text.contains("copied-value-41d9"), "{text}");
        }
    }
}
