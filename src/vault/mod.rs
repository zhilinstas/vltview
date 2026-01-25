use anyhow::Result;
use reqwest::{Client, ClientBuilder};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;

use crate::config::Config;

pub mod audit_log;
pub mod auth;
pub mod kv;
pub mod policy;
pub mod request;
pub mod response;
pub mod sanitize;

pub use auth::AuthManager;
pub use kv::KvEngine;
pub use policy::PolicyApi;

/// Maximum concurrent API requests to Vault
const MAX_CONCURRENT_REQUESTS: usize = 10;

/// Vault API client
pub struct VaultClient {
    /// HTTP client
    #[allow(dead_code)]
    client: Client,

    /// Base URL for Vault API
    #[allow(dead_code)]
    base_url: String,

    /// Authentication manager
    #[allow(dead_code)]
    auth: AuthManager,

    /// KV Engine API
    kv: KvEngine,

    /// Policy API
    policy: PolicyApi,

    /// Optional namespace for Enterprise
    #[allow(dead_code)]
    namespace: Option<String>,

    /// Rate limiter semaphore to prevent API flooding
    #[allow(dead_code)]
    rate_limiter: Arc<Semaphore>,
}

impl VaultClient {
    /// Create a new Vault client
    pub fn new(config: Config) -> Result<Self> {
        // Build HTTP client with appropriate settings
        let mut client_builder = ClientBuilder::new()
            .timeout(Duration::from_secs(config.timeout_secs))
            .user_agent(format!("vltview/{}", env!("CARGO_PKG_VERSION")));

        // Handle TLS verification
        if config.insecure {
            eprintln!("\n⚠️  WARNING: TLS CERTIFICATE VERIFICATION DISABLED ⚠️");
            eprintln!("⚠️  This is INSECURE and should ONLY be used for development!");
            eprintln!("⚠️  Your Vault credentials may be intercepted by attackers!");
            eprintln!("⚠️  NEVER use --insecure in production environments!\n");
            tracing::warn!("TLS certificate verification disabled - INSECURE MODE");
            client_builder = client_builder.danger_accept_invalid_certs(true);
        }

        let client = client_builder.build()?;

        let base_url = config.base_url();
        let namespace = config.vault_namespace.clone();

        // Initialize auth manager
        let auth = AuthManager::new(config.vault_token.clone());

        // Initialize KV engine
        let kv = KvEngine::new(
            client.clone(),
            base_url.clone(),
            auth.clone(),
            namespace.clone(),
        );

        // Initialize policy API
        let policy = PolicyApi::new(
            client.clone(),
            base_url.clone(),
            auth.clone(),
            namespace.clone(),
        );

        // Create rate limiter to prevent API flooding
        let rate_limiter = Arc::new(Semaphore::new(MAX_CONCURRENT_REQUESTS));

        tracing::debug!(
            "Vault client initialized with max {} concurrent requests",
            MAX_CONCURRENT_REQUESTS
        );

        // Log successful authentication (without exposing tokens)
        audit_log::log_auth_success(&config.vault_addr);

        Ok(Self {
            client,
            base_url,
            auth,
            kv,
            policy,
            namespace,
            rate_limiter,
        })
    }

    /// Get reference to KV Engine API
    pub fn kv(&self) -> &KvEngine {
        &self.kv
    }

    /// Get reference to Policy API
    pub fn policy(&self) -> &PolicyApi {
        &self.policy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vault_client_creation() {
        let mut config = Config::default();
        config.vault_addr = "https://vault.example.com".to_string();
        config.vault_token = "test-token".to_string();

        let result = VaultClient::new(config);
        assert!(result.is_ok());
    }
}
