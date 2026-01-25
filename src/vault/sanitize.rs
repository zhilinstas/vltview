use anyhow::{anyhow, Result};

/// Sanitize and validate Vault paths to prevent path traversal attacks
pub fn sanitize_vault_path(path: &str) -> Result<String> {
    // Remove dangerous path traversal sequences
    let mut sanitized = path
        .replace("..", "")  // Remove parent directory references
        .replace("//", "/") // Remove double slashes
        .trim()
        .to_string();

    // Ensure path starts with /
    if !sanitized.starts_with('/') {
        sanitized = format!("/{}", sanitized);
    }

    // Remove trailing slash unless it's the root path
    if sanitized.len() > 1 && sanitized.ends_with('/') {
        sanitized.pop();
    }

    // Validate no null bytes or control characters
    if sanitized.chars().any(|c| c.is_control()) {
        return Err(anyhow!("Path contains invalid control characters"));
    }

    // Validate no unusual characters that could be used for injection
    if sanitized.contains('\0') {
        return Err(anyhow!("Path contains null bytes"));
    }

    // Ensure path is not empty after sanitization
    if sanitized.is_empty() || sanitized == "/" {
        // Root path is valid
        return Ok("/".to_string());
    }

    tracing::debug!("Sanitized path: {} -> {}", path, sanitized);

    Ok(sanitized)
}

/// Sanitize Vault namespace to prevent header injection
pub fn sanitize_namespace(namespace: &str) -> Result<String> {
    let sanitized = namespace.trim().to_string();

    if sanitized.is_empty() {
        return Err(anyhow!("Namespace cannot be empty"));
    }

    // Check for header injection attempts
    if sanitized.contains('\n') || sanitized.contains('\r') {
        return Err(anyhow!("Namespace contains invalid newline characters"));
    }

    // Check for control characters
    if sanitized.chars().any(|c| c.is_control()) {
        return Err(anyhow!("Namespace contains invalid control characters"));
    }

    Ok(sanitized)
}

/// Sanitize error messages from Vault API to avoid information disclosure
pub fn sanitize_vault_error(status: u16, raw_error: &str) -> String {
    // Log full error for debugging
    tracing::debug!("Vault API error ({}): {}", status, raw_error);

    // Return user-friendly sanitized message
    match status {
        400 => "Invalid request".to_string(),
        403 => "Access denied - insufficient permissions".to_string(),
        404 => "Resource not found".to_string(),
        429 => "Rate limit exceeded - please try again later".to_string(),
        500..=599 => "Vault server error - please contact administrator".to_string(),
        _ => format!("Request failed with status {}", status),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_traversal_blocked() {
        let dangerous_paths = vec![
            "../../../etc/passwd",
            "../../vault/config",
            "path/../../../secret",
            "path/./../../data",
            "/secret/../admin",
        ];

        for path in dangerous_paths {
            let result = sanitize_vault_path(path);
            assert!(result.is_ok(), "Path should be sanitized: {}", path);
            let sanitized = result.unwrap();
            assert!(!sanitized.contains(".."), "Path still contains '..': {}", sanitized);
        }
    }

    #[test]
    fn test_valid_paths() {
        let valid_paths = vec![
            ("/secret/myapp", "/secret/myapp"),
            ("secret/myapp", "/secret/myapp"),
            ("/secret/myapp/", "/secret/myapp"),
            ("//secret//myapp//", "/secret/myapp"),
            ("/", "/"),
        ];

        for (input, expected) in valid_paths {
            let result = sanitize_vault_path(input);
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), expected);
        }
    }

    #[test]
    fn test_control_characters_rejected() {
        let paths_with_control = vec![
            "/secret/my\0app",
            "/secret/my\napp",
            "/secret/my\rapp",
        ];

        for path in paths_with_control {
            let result = sanitize_vault_path(path);
            assert!(result.is_err(), "Control characters should be rejected: {:?}", path);
        }
    }

    #[test]
    fn test_namespace_validation() {
        // Valid namespaces
        assert!(sanitize_namespace("admin").is_ok());
        assert!(sanitize_namespace("my-namespace").is_ok());

        // Invalid namespaces
        assert!(sanitize_namespace("").is_err());
        assert!(sanitize_namespace("admin\n").is_err());
        assert!(sanitize_namespace("admin\r\n").is_err());
        assert!(sanitize_namespace("   ").is_err());
    }

    #[test]
    fn test_error_sanitization() {
        // Test that errors don't leak internal details
        let error = sanitize_vault_error(403, "permission denied: invalid ACL policy 'secret/admin'");
        assert!(!error.contains("ACL"));
        assert!(!error.contains("secret/admin"));
        assert_eq!(error, "Access denied - insufficient permissions");

        let error = sanitize_vault_error(404, "secret not found at path secret/prod/api-key");
        assert!(!error.contains("secret/prod"));
        assert_eq!(error, "Resource not found");
    }
}
