use anyhow::Result;
use reqwest::ClientBuilder;
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

pub use auth::{AuthManager, Token};
pub use kv::{KvEngine, SecretData, WriteMode};
pub use policy::PolicyApi;

/// Maximum concurrent API requests to Vault
const MAX_CONCURRENT_REQUESTS: usize = 10;

/// Vault API client with rate limiting and security features
pub struct VaultClient {
    /// Base URL for Vault API
    base_url: String,

    /// KV Engine API
    kv: KvEngine,

    /// Policy API
    policy: PolicyApi,

    /// Optional namespace for Enterprise
    namespace: Option<String>,
}

impl VaultClient {
    /// Create a new Vault client
    ///
    /// Takes the configuration for good: the token moves out of it into the
    /// `AuthManager`, and nothing else keeps one.
    pub fn new(config: Config) -> Result<Self> {
        // Build HTTP client with appropriate settings
        let mut client_builder = ClientBuilder::new()
            .timeout(Duration::from_secs(config.timeout_secs))
            .user_agent(format!("vltview/{}", env!("CARGO_PKG_VERSION")));

        // Handle TLS verification
        if config.insecure {
            tracing::warn!("TLS certificate verification disabled - INSECURE MODE");
            client_builder = client_builder.danger_accept_invalid_certs(true);
        }

        let client = client_builder.build()?;

        let base_url = config.base_url();

        // Sanitize namespace if provided (Enterprise feature)
        let namespace = if let Some(ns) = &config.vault_namespace {
            Some(sanitize::sanitize_namespace(ns)?)
        } else {
            None
        };

        // The token leaves the configuration here
        let auth = Arc::new(AuthManager::new(config.vault_token));

        // Create rate limiter to prevent API flooding
        let rate_limiter = Arc::new(Semaphore::new(MAX_CONCURRENT_REQUESTS));

        // "Authenticated" is logged when the first request succeeds, not here
        let auth_audit = audit_log::AuthAudit::new(&base_url);

        // Initialize KV engine with rate limiter
        let kv = KvEngine::new(
            client.clone(),
            base_url.clone(),
            Arc::clone(&auth),
            namespace.clone(),
            Arc::clone(&rate_limiter),
            auth_audit.clone(),
        );

        // Initialize policy API with rate limiter
        let policy = PolicyApi::new(
            client,
            base_url.clone(),
            auth,
            namespace.clone(),
            rate_limiter,
            auth_audit,
        );

        tracing::debug!(
            "Vault client initialized with max {} concurrent requests",
            MAX_CONCURRENT_REQUESTS
        );

        // The HTTP client, the owner of the token and the rate limiter live
        // on in the two APIs that share them
        Ok(Self {
            base_url,
            kv,
            policy,
            namespace,
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

    /// Get base URL
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Get namespace
    pub fn namespace(&self) -> Option<&str> {
        self.namespace.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;

    fn config(vault_addr: String) -> Config {
        Config {
            vault_addr,
            vault_token: "one-token".to_string().into(),
            ..Config::default()
        }
    }

    #[test]
    fn test_vault_client_creation() {
        let result = VaultClient::new(config("https://vault.example.com".to_string()));
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn secret_and_policy_requests_carry_the_token_of_the_one_owner() {
        let mut server = Server::new_async().await;
        let secrets = server
            .mock("GET", "/v1/secret/metadata?list=true")
            .match_header("X-Vault-Token", "one-token")
            .with_status(200)
            .with_body(r#"{"data":{"keys":["app/"]}}"#)
            .create_async()
            .await;
        let policies = server
            .mock("LIST", "/v1/sys/policies/acl")
            .match_header("X-Vault-Token", "one-token")
            .with_status(200)
            .with_body(r#"{"data":{"keys":["default"]}}"#)
            .create_async()
            .await;

        let client = VaultClient::new(config(server.url())).unwrap();
        client.kv().list("/").await.unwrap();
        client.policy().list().await.unwrap();

        secrets.assert_async().await;
        policies.assert_async().await;
    }

    #[test]
    fn nothing_holds_the_token_once_the_client_is_gone() {
        // The token used to be cloned into the client, the KV engine and the
        // policy API, and to stay in the configuration as well
        let client = VaultClient::new(config("https://vault.example.com".to_string())).unwrap();
        let owner = Arc::downgrade(client.kv().token_owner());

        drop(client);

        assert!(
            owner.upgrade().is_none(),
            "the token outlived the client: something else still holds it"
        );
    }
}
