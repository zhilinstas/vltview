use reqwest::RequestBuilder;
use zeroize::{Zeroize, ZeroizeOnDrop};

/// Authentication manager for Vault tokens
/// Token is securely zeroed from memory when dropped
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct AuthManager {
    /// Vault token (zeroed on drop)
    token: String,
}

impl AuthManager {
    /// Create a new authentication manager
    pub fn new(token: String) -> Self {
        Self { token }
    }

    /// Add authentication header to request
    pub fn add_auth_header(&self, request: RequestBuilder) -> RequestBuilder {
        request.header("X-Vault-Token", &self.token)
    }
}

// Custom Debug to avoid leaking tokens
impl std::fmt::Debug for AuthManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthManager")
            .field("token", &"***REDACTED***")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auth_manager_creation() {
        let auth = AuthManager::new("test-token".to_string());
        // Don't expose token in tests
        assert!(!format!("{:?}", auth).contains("test-token"));
    }
}
