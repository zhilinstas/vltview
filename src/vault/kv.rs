use anyhow::Result;
use reqwest::Client;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Semaphore;

use super::audit_log;
use super::auth::AuthManager;
use super::response::{FullSecretMetadata, ListResponse, VaultResponse, VaultSecret};
use super::sanitize::{sanitize_vault_path, sanitize_vault_error};

/// KV Engine API client for KV2 secrets with rate limiting
#[derive(Clone)]
pub struct KvEngine {
    client: Client,
    base_url: String,
    auth: AuthManager,
    namespace: Option<String>,
    rate_limiter: Arc<Semaphore>,
}

impl KvEngine {
    /// Create a new KV engine client
    pub fn new(
        client: Client,
        base_url: String,
        auth: AuthManager,
        namespace: Option<String>,
        rate_limiter: Arc<Semaphore>,
    ) -> Self {
        Self {
            client,
            base_url,
            auth,
            namespace,
            rate_limiter,
        }
    }

    /// List secrets at a path
    pub async fn list(&self, path: &str) -> Result<Vec<String>> {
        // Acquire rate limit permit
        let _permit = self.rate_limiter.acquire().await?;

        let sanitized_path = sanitize_vault_path(path)?;
        let url = format!("{}/v1/secret/metadata{}", self.base_url, sanitized_path);

        let mut req = self.client.get(&url).query(&[("list", "true")]);
        req = self.auth.add_auth_header(req);

        if let Some(ns) = &self.namespace {
            req = req.header("X-Vault-Namespace", ns);
        }

        let response = req.send().await?;

        if response.status().is_success() {
            let vault_resp: VaultResponse<ListResponse> = response.json().await?;
            let count = vault_resp.data.keys.len();
            audit_log::log_secret_list(&sanitized_path, count);
            Ok(vault_resp.data.keys)
        } else {
            Ok(Vec::new()) // TODO: Better error handling
        }
    }

    /// Get a secret at a path (latest version)
    pub async fn get(&self, path: &str) -> Result<VaultSecret> {
        self.get_version(path, None).await
    }

    /// Get a specific version of a secret
    pub async fn get_version(&self, path: &str, version: Option<u64>) -> Result<VaultSecret> {
        // Acquire rate limit permit
        let _permit = self.rate_limiter.acquire().await?;

        let sanitized_path = sanitize_vault_path(path)?;
        let url = format!("{}/v1/secret/data{}", self.base_url, sanitized_path);

        let mut req = self.client.get(&url);

        // Add version query parameter if specified
        if let Some(v) = version {
            req = req.query(&[("version", v.to_string())]);
        }

        req = self.auth.add_auth_header(req);

        if let Some(ns) = &self.namespace {
            req = req.header("X-Vault-Namespace", ns);
        }

        let response = req.send().await?;

        // Check if the response is successful
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            let sanitized_error = sanitize_vault_error(status.as_u16(), &error_text);
            anyhow::bail!("{}", sanitized_error);
        }

        let vault_resp: VaultResponse<VaultSecret> = response.json().await?;

        audit_log::log_secret_read(&sanitized_path);
        Ok(vault_resp.data)
    }

    /// Put (create/update) a secret
    pub async fn put(&self, path: &str, data: HashMap<String, String>) -> Result<()> {
        // Acquire rate limit permit
        let _permit = self.rate_limiter.acquire().await?;

        let sanitized_path = sanitize_vault_path(path)?;
        let url = format!("{}/v1/secret/data{}", self.base_url, sanitized_path);

        // Redact secret data for logging
        let redacted_data = audit_log::redact_secret_data(&data);
        tracing::debug!("Writing secret with keys: {:?}", redacted_data.keys());

        let body = serde_json::json!({
            "data": data
        });

        let mut req = self.client.post(&url).json(&body);
        req = self.auth.add_auth_header(req);

        if let Some(ns) = &self.namespace {
            req = req.header("X-Vault-Namespace", ns);
        }

        req.send().await?;
        audit_log::log_secret_write(&sanitized_path, data.len());
        Ok(())
    }

    /// Delete a secret (permanently removes all versions and metadata)
    pub async fn delete(&self, path: &str) -> Result<()> {
        // Acquire rate limit permit
        let _permit = self.rate_limiter.acquire().await?;

        let sanitized_path = sanitize_vault_path(path)?;
        // Use metadata endpoint to permanently delete the secret
        // This removes all versions and metadata, so it won't appear in lists
        let url = format!("{}/v1/secret/metadata{}", self.base_url, sanitized_path);

        let mut req = self.client.delete(&url);
        req = self.auth.add_auth_header(req);

        if let Some(ns) = &self.namespace {
            req = req.header("X-Vault-Namespace", ns);
        }

        let response = req.send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            let sanitized_error = sanitize_vault_error(status.as_u16(), &error_text);
            anyhow::bail!("{}", sanitized_error);
        }

        audit_log::log_secret_delete(&sanitized_path);
        Ok(())
    }

    /// Get metadata for a secret (all versions)
    pub async fn get_metadata(&self, path: &str) -> Result<FullSecretMetadata> {
        // Acquire rate limit permit
        let _permit = self.rate_limiter.acquire().await?;

        let sanitized_path = sanitize_vault_path(path)?;
        let url = format!("{}/v1/secret/metadata{}", self.base_url, sanitized_path);

        let mut req = self.client.get(&url);
        req = self.auth.add_auth_header(req);

        if let Some(ns) = &self.namespace {
            req = req.header("X-Vault-Namespace", ns);
        }

        let response = req.send().await?;

        // Check if the response is successful
        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            let sanitized_error = sanitize_vault_error(status.as_u16(), &error_text);
            anyhow::bail!("{}", sanitized_error);
        }

        let vault_resp: VaultResponse<FullSecretMetadata> = response.json().await?;

        Ok(vault_resp.data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kv_engine_creation() {
        let client = Client::new();
        let auth = AuthManager::new("test-token".to_string());
        let kv = KvEngine::new(client, "https://vault.example.com".to_string(), auth, None);

        assert_eq!(kv.base_url, "https://vault.example.com");
    }
}
