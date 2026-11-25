use anyhow::{Context, Result, anyhow};
use url::Url;

/// Sanitize and validate a logical Vault path.
///
/// Returns the path as `/seg/seg`, with empty, `.` and `..` segments removed.
/// The result is a logical path, not a URL: it may still contain characters
/// such as `%`, `?` or `#`, so it must go through [`vault_url`] rather than be
/// formatted into a URL string.
pub fn sanitize_vault_path(path: &str) -> Result<String> {
    let trimmed = path.trim();

    // Reject null bytes and other control characters outright
    if trimmed.chars().any(|c| c.is_control()) {
        return Err(anyhow!("Path contains invalid control characters"));
    }

    let segments: Vec<&str> = trimmed
        .split('/')
        .filter(|segment| !matches!(*segment, "" | "." | ".."))
        .collect();

    let sanitized = format!("/{}", segments.join("/"));

    tracing::debug!("Sanitized path: {} -> {}", path, sanitized);

    Ok(sanitized)
}

/// Build a Vault API URL from a base address, fixed API segments and a
/// sanitized logical path.
///
/// Every segment is percent-encoded individually, so nothing in a secret name
/// can change the shape of the URL: `%2e%2e` stays a literal name instead of
/// being resolved as `..`, and `?` or `#` cannot start a query or fragment.
pub fn vault_url(base_url: &str, api_segments: &[&str], path: &str) -> Result<Url> {
    let mut url = Url::parse(base_url).context("Invalid Vault address URL")?;

    {
        let mut segments = url
            .path_segments_mut()
            .map_err(|_| anyhow!("Vault address cannot be used as a base URL"))?;
        segments.pop_if_empty();
        segments.extend(api_segments);
        segments.extend(path.split('/').filter(|segment| !segment.is_empty()));
    }

    Ok(url)
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
            assert!(
                !sanitized.contains(".."),
                "Path still contains '..': {}",
                sanitized
            );
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
        let paths_with_control = vec!["/secret/my\0app", "/secret/my\napp", "/secret/my\rapp"];

        for path in paths_with_control {
            let result = sanitize_vault_path(path);
            assert!(
                result.is_err(),
                "Control characters should be rejected: {:?}",
                path
            );
        }
    }

    #[test]
    fn test_dots_inside_names_are_kept() {
        assert_eq!(sanitize_vault_path("/app/v1..2").unwrap(), "/app/v1..2");
        assert_eq!(sanitize_vault_path("/app/.env").unwrap(), "/app/.env");
        assert_eq!(sanitize_vault_path("/a/./b/../c").unwrap(), "/a/b/c");
        assert_eq!(sanitize_vault_path("///a////b///").unwrap(), "/a/b");
    }

    #[test]
    fn test_vault_url_plain_path() {
        let url = vault_url(
            "http://localhost:8200",
            &["v1", "secret", "data"],
            "/myapp/database",
        )
        .unwrap();
        assert_eq!(
            url.as_str(),
            "http://localhost:8200/v1/secret/data/myapp/database"
        );
    }

    #[test]
    fn test_vault_url_root_and_base_prefix() {
        let url = vault_url("http://localhost:8200", &["v1", "secret", "metadata"], "/").unwrap();
        assert_eq!(url.as_str(), "http://localhost:8200/v1/secret/metadata");

        // A base address with its own path prefix and trailing slash is kept
        let url = vault_url("https://example.com/vault/", &["v1", "sys"], "/x").unwrap();
        assert_eq!(url.as_str(), "https://example.com/vault/v1/sys/x");
    }

    #[test]
    fn test_vault_url_encoded_dots_cannot_traverse() {
        // Regression: "%2e%2e" used to reach the URL parser unencoded, which
        // resolves it as ".." and escaped the KV mount.
        let path = sanitize_vault_path("/%2e%2e/%2E%2E/sys/policies/acl").unwrap();
        let url = vault_url("http://localhost:8200", &["v1", "secret", "data"], &path).unwrap();
        assert_eq!(
            url.path(),
            "/v1/secret/data/%252e%252e/%252E%252E/sys/policies/acl"
        );
        assert!(url.path().starts_with("/v1/secret/data/"));
    }

    #[test]
    fn test_vault_url_query_and_fragment_are_encoded() {
        let path = sanitize_vault_path("/app/name?list=true#frag").unwrap();
        let url = vault_url("http://localhost:8200", &["v1", "secret", "data"], &path).unwrap();
        assert_eq!(url.query(), None);
        assert_eq!(url.fragment(), None);
        assert_eq!(url.path(), "/v1/secret/data/app/name%3Flist=true%23frag");
    }

    #[test]
    fn test_vault_url_encodes_spaces_and_slash_free_segments() {
        let url = vault_url(
            "http://localhost:8200",
            &["v1", "sys", "policies", "acl"],
            "/my policy",
        )
        .unwrap();
        assert_eq!(url.path(), "/v1/sys/policies/acl/my%20policy");
    }

    #[test]
    fn test_error_sanitization() {
        // Test that errors don't leak internal details
        let error =
            sanitize_vault_error(403, "permission denied: invalid ACL policy 'secret/admin'");
        assert!(!error.contains("ACL"));
        assert!(!error.contains("secret/admin"));
        assert_eq!(error, "Access denied - insufficient permissions");

        let error = sanitize_vault_error(404, "secret not found at path secret/prod/api-key");
        assert!(!error.contains("secret/prod"));
        assert_eq!(error, "Resource not found");
    }
}
