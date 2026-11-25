use anyhow::Result;
use reqwest::Client;
use std::sync::Arc;
use tokio::sync::Semaphore;

use super::audit_log;
use super::auth::AuthManager;
use super::sanitize::{sanitize_vault_error, vault_url};

/// Policy API client for ACL policies with rate limiting
#[derive(Clone)]
pub struct PolicyApi {
    client: Client,
    base_url: String,
    auth: AuthManager,
    namespace: Option<String>,
    rate_limiter: Arc<Semaphore>,
}

impl PolicyApi {
    /// Create a new policy API client
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

    /// List all policies
    pub async fn list(&self) -> Result<Vec<String>> {
        // Acquire rate limit permit
        let _permit = self.rate_limiter.acquire().await?;

        let url = format!("{}/v1/sys/policies/acl", self.base_url);

        // Vault LIST endpoint requires the "LIST" HTTP method
        let mut req = self
            .client
            .request(reqwest::Method::from_bytes(b"LIST")?, &url);
        req = self.auth.add_auth_header(req);

        if let Some(ns) = &self.namespace {
            req = req.header("X-Vault-Namespace", ns);
        }

        let response = req.send().await?;

        if response.status().is_success() {
            let data: serde_json::Value = response.json().await?;

            // Extract policies list from response (LIST endpoints return "keys")
            if let Some(policies) = data
                .get("data")
                .and_then(|d| d.get("keys"))
                .and_then(|p| p.as_array())
            {
                let policy_names: Vec<String> = policies
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect();
                audit_log::log_policy_list(policy_names.len());
                Ok(policy_names)
            } else {
                Ok(Vec::new())
            }
        } else {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            let sanitized_error = sanitize_vault_error(status.as_u16(), &error_text);
            anyhow::bail!("{}", sanitized_error);
        }
    }

    /// Read a specific policy
    pub async fn read(&self, name: &str) -> Result<String> {
        // Acquire rate limit permit
        let _permit = self.rate_limiter.acquire().await?;

        // The name is a single, percent-encoded URL segment
        let url = vault_url(&self.base_url, &["v1", "sys", "policies", "acl", name], "/")?;

        let mut req = self.client.get(url);
        req = self.auth.add_auth_header(req);

        if let Some(ns) = &self.namespace {
            req = req.header("X-Vault-Namespace", ns);
        }

        let response = req.send().await?;

        if response.status().is_success() {
            let data: serde_json::Value = response.json().await?;

            // Extract policy content from response
            if let Some(policy) = data
                .get("data")
                .and_then(|d| d.get("policy"))
                .and_then(|p| p.as_str())
            {
                audit_log::log_policy_read(name);
                Ok(policy.to_string())
            } else {
                Ok(String::new())
            }
        } else {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            let sanitized_error = sanitize_vault_error(status.as_u16(), &error_text);
            anyhow::bail!("{}", sanitized_error);
        }
    }
}
