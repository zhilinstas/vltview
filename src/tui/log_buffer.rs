use std::sync::{Arc, LazyLock, RwLock};

/// Log entry with timestamp and message
#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: String,
    pub level: String,
    pub target: String,
    pub message: String,
}

/// Global log buffer
static LOG_BUFFER: LazyLock<Arc<RwLock<Vec<LogEntry>>>> =
    LazyLock::new(|| Arc::new(RwLock::new(Vec::new())));

/// Add a log entry to the buffer
pub fn add_log(level: &str, target: &str, message: String) {
    let timestamp = jiff::ZonedDateTime::now()
        .strftime("%H:%M:%S%.3f")
        .to_string();
    let entry = LogEntry {
        timestamp,
        level: level.to_string(),
        target: target.to_string(),
        message,
    };

    if let Ok(mut buffer) = LOG_BUFFER.write() {
        buffer.push(entry);
        // Keep only last 10000 entries
        let len = buffer.len();
        if len > 10000 {
            let to_remove = len - 10000;
            buffer.drain(0..to_remove);
        }
    }
}

/// Get all log entries
pub fn get_logs() -> Vec<LogEntry> {
    LOG_BUFFER
        .read()
        .map(|buffer| buffer.clone())
        .unwrap_or_default()
}

/// Clear all logs
pub fn clear_logs() {
    if let Ok(mut buffer) = LOG_BUFFER.write() {
        buffer.clear();
    }
}
