use anyhow::{Context, Result};
use arboard::Clipboard;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

use crate::vault::audit_log;

/// Default clipboard timeout in seconds
const DEFAULT_CLIPBOARD_TIMEOUT: u64 = 30;

/// Clipboard manager for system clipboard operations
pub struct ClipboardManager {
    /// Clipboard instance (wrapped in Arc<Mutex> for async clearing)
    clipboard: Arc<Mutex<Clipboard>>,
}

impl ClipboardManager {
    /// Create a new clipboard manager
    pub fn new() -> Result<Self> {
        let clipboard = Clipboard::new().context("Failed to initialize clipboard")?;
        Ok(Self {
            clipboard: Arc::new(Mutex::new(clipboard)),
        })
    }

    /// Copy text to clipboard with automatic clearing after timeout
    /// This is a security feature to prevent secrets from persisting in clipboard
    pub async fn copy_text(&mut self, text: &str) -> Result<()> {
        self.copy_text_with_timeout(text, DEFAULT_CLIPBOARD_TIMEOUT)
            .await
    }

    /// Copy text to clipboard with custom timeout
    pub async fn copy_text_with_timeout(&mut self, text: &str, timeout_secs: u64) -> Result<()> {
        // Store the current text we're copying for comparison later
        let copied_text = text.to_string();
        let text_len = text.len();

        // Copy to clipboard
        {
            let mut clip = self.clipboard.lock().await;
            clip.set_text(text)
                .context("Failed to copy text to clipboard")?;
        }

        // Log clipboard operation for security audit trail
        audit_log::log_clipboard_copy(text_len, timeout_secs);

        // Schedule automatic clearing
        let clipboard_clone = Arc::clone(&self.clipboard);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(timeout_secs)).await;

            // Clear clipboard only if it still contains our text
            // This prevents clearing if user copied something else
            let mut clip = clipboard_clone.lock().await;
            if let Ok(current_text) = clip.get_text() {
                if current_text == copied_text {
                    let _ = clip.set_text("");
                    tracing::info!(
                        "Clipboard automatically cleared after {} seconds (security feature)",
                        timeout_secs
                    );
                    audit_log::log_security_event(
                        "clipboard.auto_cleared",
                        &format!("Clipboard cleared after {}s timeout", timeout_secs),
                    );
                } else {
                    tracing::debug!("Clipboard not cleared - content changed by user");
                }
            }
        });

        Ok(())
    }

    /// Immediately clear the clipboard
    pub async fn clear(&mut self) -> Result<()> {
        let mut clip = self.clipboard.lock().await;
        clip.set_text("").context("Failed to clear clipboard")?;
        tracing::debug!("Clipboard manually cleared");
        Ok(())
    }
}

impl Default for ClipboardManager {
    fn default() -> Self {
        Self::new().expect("Failed to initialize clipboard")
    }
}
