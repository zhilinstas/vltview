/// Secure audit logging module
/// Logs actions without exposing sensitive data like tokens or secret values
use std::collections::HashMap;

/// Redact sensitive information from paths and data for logging
pub fn redact_secret_value(value: &str) -> String {
    if value.len() <= 8 {
        "***".to_string()
    } else {
        format!("{}...{}", &value[..4], &value[value.len() - 4..])
    }
}

/// Redact secret data for logging - shows keys but not values
pub fn redact_secret_data(data: &HashMap<String, String>) -> HashMap<String, String> {
    data.keys()
        .map(|k| (k.clone(), "***REDACTED***".to_string()))
        .collect()
}

/// Extract path without revealing full hierarchy for logging
pub fn sanitize_path_for_logging(path: &str) -> String {
    // Keep first and last segment, redact middle for privacy
    let segments: Vec<&str> = path.trim_matches('/').split('/').collect();

    match segments.len() {
        0 => "/".to_string(),
        1 => format!("/{}", segments[0]),
        2 => format!("/{}/{}", segments[0], segments[1]),
        _ => format!(
            "/{}/.../{} ({} segments)",
            segments[0],
            segments[segments.len() - 1],
            segments.len()
        ),
    }
}

/// Log successful secret read operation
pub fn log_secret_read(path: &str) {
    let safe_path = sanitize_path_for_logging(path);
    tracing::info!(
        action = "secret.read",
        path = %safe_path,
        "Secret read successfully"
    );
}

/// Log secret list operation
pub fn log_secret_list(path: &str, count: usize) {
    let safe_path = sanitize_path_for_logging(path);
    tracing::info!(
        action = "secret.list",
        path = %safe_path,
        count = count,
        "Listed secrets"
    );
}

/// Log secret write operation
pub fn log_secret_write(path: &str, key_count: usize) {
    let safe_path = sanitize_path_for_logging(path);
    tracing::info!(
        action = "secret.write",
        path = %safe_path,
        keys = key_count,
        "Secret written successfully"
    );
}

/// Log secret delete operation
pub fn log_secret_delete(path: &str) {
    let safe_path = sanitize_path_for_logging(path);
    tracing::warn!(
        action = "secret.delete",
        path = %safe_path,
        "Secret deleted"
    );
}

/// Log policy read operation
pub fn log_policy_read(policy_name: &str) {
    tracing::info!(
        action = "policy.read",
        policy = %policy_name,
        "Policy read successfully"
    );
}

/// Log policy list operation
pub fn log_policy_list(count: usize) {
    tracing::info!(action = "policy.list", count = count, "Listed policies");
}

/// Log authentication attempts (success only, failures are security events)
pub fn log_auth_success(addr: &str) {
    // Don't log tokens, only that auth succeeded
    tracing::info!(
        action = "auth.success",
        vault_addr = %addr,
        "Authenticated to Vault successfully"
    );
}

/// Log clipboard operations (for audit trail)
pub fn log_clipboard_copy(data_size: usize, timeout_secs: u64) {
    tracing::info!(
        action = "clipboard.copy",
        bytes = data_size,
        timeout_secs = timeout_secs,
        "Copied to clipboard with auto-clear"
    );
}

/// Log security-relevant events
pub fn log_security_event(event: &str, details: &str) {
    tracing::warn!(
        action = "security.event",
        event = %event,
        details = %details,
        "Security event logged"
    );
}
