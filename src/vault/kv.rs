use anyhow::Result;
use reqwest::{Client, RequestBuilder, StatusCode};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Semaphore;

use super::audit_log;
use super::auth::AuthManager;
use super::request::{authorize, ensure_success};
use super::response::{FullSecretMetadata, ListResponse, VaultResponse, VaultSecret};
use super::sanitize::{sanitize_vault_error, sanitize_vault_path, vault_url};

/// Secret payload as stored in Vault: field name to JSON value.
///
/// KV v2 values are arbitrary JSON, not only strings. Keeping them as
/// `serde_json::Value` end to end is what lets a read-modify-write cycle
/// preserve numbers, booleans, nulls and nested structures.
pub type SecretData = HashMap<String, serde_json::Value>;

/// What a write is allowed to replace
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteMode {
    /// A new secret. Vault is asked to write only if nothing exists at the
    /// path (check-and-set 0), so a create can never add a version to a
    /// secret that is already there, wherever the name came from.
    Create,
    /// A new version of an existing secret
    Update,
}

/// Mount point of the KV v2 engine
const KV_MOUNT: &str = "secret";

/// KV Engine API client for KV2 secrets with rate limiting
#[derive(Clone)]
pub struct KvEngine {
    client: Client,
    base_url: String,
    auth: Arc<AuthManager>,
    namespace: Option<String>,
    rate_limiter: Arc<Semaphore>,
    auth_audit: audit_log::AuthAudit,
}

impl KvEngine {
    /// Create a new KV engine client
    pub fn new(
        client: Client,
        base_url: String,
        auth: Arc<AuthManager>,
        namespace: Option<String>,
        rate_limiter: Arc<Semaphore>,
        auth_audit: audit_log::AuthAudit,
    ) -> Self {
        Self {
            client,
            base_url,
            auth,
            namespace,
            rate_limiter,
            auth_audit,
        }
    }

    /// The owner of the token, for the test that nothing else holds it
    #[cfg(test)]
    pub(super) fn token_owner(&self) -> &Arc<AuthManager> {
        &self.auth
    }

    /// Add the token and namespace headers
    fn authorize(&self, req: RequestBuilder) -> RequestBuilder {
        authorize(req, &self.auth, self.namespace.as_deref())
    }

    /// List secrets at a path
    ///
    /// Vault answers 404 for a path with nothing under it, which is reported
    /// as an empty list. Every other failure (403, 5xx, ...) is an error, so
    /// a permission problem is not mistaken for an empty folder.
    pub async fn list(&self, path: &str) -> Result<Vec<String>> {
        // Acquire rate limit permit
        let _permit = self.rate_limiter.acquire().await?;

        let sanitized_path = sanitize_vault_path(path)?;
        let url = vault_url(
            &self.base_url,
            &["v1", KV_MOUNT, "metadata"],
            &sanitized_path,
        )?;

        let req = self.authorize(self.client.get(url).query(&[("list", "true")]));
        let response = req.send().await?;

        if response.status() == StatusCode::NOT_FOUND {
            audit_log::log_secret_list(&sanitized_path, 0);
            return Ok(Vec::new());
        }

        let response = ensure_success(response).await?;
        let vault_resp: VaultResponse<ListResponse> = response.json().await?;
        self.auth_audit.accepted();
        let count = vault_resp.data.keys.len();
        audit_log::log_secret_list(&sanitized_path, count);
        Ok(vault_resp.data.keys)
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
        let url = vault_url(&self.base_url, &["v1", KV_MOUNT, "data"], &sanitized_path)?;

        let mut req = self.client.get(url);

        // Add version query parameter if specified
        if let Some(v) = version {
            req = req.query(&[("version", v.to_string())]);
        }

        let response = self.authorize(req).send().await?;
        let response = ensure_success(response).await?;

        let vault_resp: VaultResponse<VaultSecret> = response.json().await?;

        self.auth_audit.accepted();
        audit_log::log_secret_read(&sanitized_path);
        Ok(vault_resp.data)
    }

    /// Write a secret: a new one (`WriteMode::Create`, refused by Vault if
    /// the path is taken) or a new version of an existing one
    pub async fn put(&self, path: &str, data: SecretData, mode: WriteMode) -> Result<()> {
        // Acquire rate limit permit
        let _permit = self.rate_limiter.acquire().await?;

        let sanitized_path = sanitize_vault_path(path)?;
        let url = vault_url(&self.base_url, &["v1", KV_MOUNT, "data"], &sanitized_path)?;

        // Redact secret data for logging
        let redacted_data = audit_log::redact_secret_data(&data);
        tracing::debug!("Writing secret with keys: {:?}", redacted_data.keys());

        let body = match mode {
            WriteMode::Create => serde_json::json!({ "options": { "cas": 0 }, "data": data }),
            WriteMode::Update => serde_json::json!({ "data": data }),
        };

        let response = self
            .authorize(self.client.post(url).json(&body))
            .send()
            .await?;

        // Vault refuses a check-and-set write with 400 and this text. Say
        // what it means; "Invalid request" would not tell the user that the
        // name is taken.
        if mode == WriteMode::Create && response.status() == StatusCode::BAD_REQUEST {
            let error_text = response.text().await.unwrap_or_default();
            if error_text.contains("check-and-set") {
                anyhow::bail!("a secret already exists at this path; nothing was written");
            }
            anyhow::bail!("{}", sanitize_vault_error(400, &error_text));
        }

        // A rejected write (403, 400, ...) must surface as an error: reporting
        // it as success makes the UI say "Saved" and drop the user's edits.
        ensure_success(response).await?;

        self.auth_audit.accepted();
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
        let url = vault_url(
            &self.base_url,
            &["v1", KV_MOUNT, "metadata"],
            &sanitized_path,
        )?;

        let response = self.authorize(self.client.delete(url)).send().await?;
        ensure_success(response).await?;

        self.auth_audit.accepted();
        audit_log::log_secret_delete(&sanitized_path);
        Ok(())
    }

    /// Get metadata for a secret (all versions)
    pub async fn get_metadata(&self, path: &str) -> Result<FullSecretMetadata> {
        // Acquire rate limit permit
        let _permit = self.rate_limiter.acquire().await?;

        let sanitized_path = sanitize_vault_path(path)?;
        let url = vault_url(
            &self.base_url,
            &["v1", KV_MOUNT, "metadata"],
            &sanitized_path,
        )?;

        let response = self.authorize(self.client.get(url)).send().await?;
        let response = ensure_success(response).await?;

        let vault_resp: VaultResponse<FullSecretMetadata> = response.json().await?;

        self.auth_audit.accepted();
        Ok(vault_resp.data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::{Matcher, Server, ServerGuard};
    use serde_json::json;

    fn engine(server: &ServerGuard) -> KvEngine {
        KvEngine::new(
            Client::new(),
            server.url(),
            Arc::new(AuthManager::new("test-token".to_string().into())),
            None,
            Arc::new(Semaphore::new(4)),
            audit_log::AuthAudit::new(&server.url()),
        )
    }

    fn data(value: serde_json::Value) -> SecretData {
        serde_json::from_value(value).unwrap()
    }

    #[tokio::test]
    async fn put_reports_rejected_write_as_error() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("POST", "/v1/secret/data/app/db")
            .with_status(403)
            .with_body(r#"{"errors":["permission denied"]}"#)
            .create_async()
            .await;

        let result = engine(&server)
            .put("/app/db", data(json!({"k": "v"})), WriteMode::Update)
            .await;

        mock.assert_async().await;
        let err = result.expect_err("a 403 on write must not be reported as success");
        assert_eq!(err.to_string(), "Access denied - insufficient permissions");
    }

    #[tokio::test]
    async fn put_sends_token_and_keeps_value_types() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("POST", "/v1/secret/data/app/db")
            .match_header("X-Vault-Token", "test-token")
            .match_body(Matcher::Json(json!({
                "data": {"host": "db", "port": 5432, "tls": true, "tags": ["a", "b"], "note": null}
            })))
            .with_status(200)
            .with_body(r#"{"data":{"version":2}}"#)
            .create_async()
            .await;

        engine(&server)
            .put(
                "/app/db",
                data(json!({
                    "host": "db", "port": 5432, "tls": true, "tags": ["a", "b"], "note": null
                })),
                WriteMode::Update,
            )
            .await
            .unwrap();

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn get_returns_non_string_values() {
        let mut server = Server::new_async().await;
        server
            .mock("GET", "/v1/secret/data/app/db")
            .with_status(200)
            .with_body(
                r#"{"data":{"data":{"host":"db","port":5432,"tls":true},
                    "metadata":{"version":1}}}"#,
            )
            .create_async()
            .await;

        let secret = engine(&server).get("/app/db").await.unwrap();

        assert_eq!(secret.data["host"], json!("db"));
        assert_eq!(secret.data["port"], json!(5432));
        assert_eq!(secret.data["tls"], json!(true));
    }

    #[tokio::test]
    async fn list_reports_forbidden_as_error() {
        let mut server = Server::new_async().await;
        server
            .mock("GET", "/v1/secret/metadata/app")
            .match_query(Matcher::UrlEncoded("list".into(), "true".into()))
            .with_status(403)
            .create_async()
            .await;

        let err = engine(&server)
            .list("/app")
            .await
            .expect_err("a 403 on list must not look like an empty folder");
        assert_eq!(err.to_string(), "Access denied - insufficient permissions");
    }

    #[tokio::test]
    async fn list_reports_server_error_as_error() {
        let mut server = Server::new_async().await;
        server
            .mock("GET", "/v1/secret/metadata/app")
            .match_query(Matcher::Any)
            .with_status(503)
            .create_async()
            .await;

        assert!(engine(&server).list("/app").await.is_err());
    }

    #[tokio::test]
    async fn list_treats_not_found_as_empty() {
        let mut server = Server::new_async().await;
        server
            .mock("GET", "/v1/secret/metadata/empty")
            .match_query(Matcher::Any)
            .with_status(404)
            .with_body(r#"{"errors":[]}"#)
            .create_async()
            .await;

        assert!(engine(&server).list("/empty").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn list_root_returns_keys() {
        let mut server = Server::new_async().await;
        server
            .mock("GET", "/v1/secret/metadata")
            .match_query(Matcher::UrlEncoded("list".into(), "true".into()))
            .with_status(200)
            .with_body(r#"{"data":{"keys":["myapp/","prod/"]}}"#)
            .create_async()
            .await;

        let keys = engine(&server).list("/").await.unwrap();
        assert_eq!(keys, vec!["myapp/", "prod/"]);
    }

    #[tokio::test]
    async fn encoded_dot_segments_stay_inside_the_mount() {
        let mut server = Server::new_async().await;
        // If the client resolved "%2e%2e" as "..", this is where it would land
        let escaped = server
            .mock("DELETE", "/v1/sys/policies/acl/admin")
            .expect(0)
            .create_async()
            .await;
        let contained = server
            .mock(
                "DELETE",
                "/v1/secret/metadata/%252e%252e/%252e%252e/sys/policies/acl/admin",
            )
            .with_status(204)
            .create_async()
            .await;

        engine(&server)
            .delete("/%2e%2e/%2e%2e/sys/policies/acl/admin")
            .await
            .unwrap();

        escaped.assert_async().await;
        contained.assert_async().await;
    }

    #[tokio::test]
    async fn create_asks_vault_to_write_only_if_nothing_is_there() {
        // Regression: a create was an unconditional write, so creating a
        // secret under a name that existed wrote a new version over it.
        let mut server = Server::new_async().await;
        let mock = server
            .mock("POST", "/v1/secret/data/app/new")
            .match_body(Matcher::Json(json!({
                "options": {"cas": 0},
                "data": {"k": "v"}
            })))
            .with_status(200)
            .with_body(r#"{"data":{"version":1}}"#)
            .create_async()
            .await;

        engine(&server)
            .put("/app/new", data(json!({"k": "v"})), WriteMode::Create)
            .await
            .unwrap();

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn create_over_an_existing_secret_is_refused_and_says_so() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("POST", "/v1/secret/data/app/db")
            .with_status(400)
            .with_body(
                r#"{"errors":["check-and-set parameter did not match the current version"]}"#,
            )
            .create_async()
            .await;

        let result = engine(&server)
            .put("/app/db", data(json!({"k": "v"})), WriteMode::Create)
            .await;

        mock.assert_async().await;
        let err = result.expect_err("an existing secret must not be overwritten");
        assert_eq!(
            err.to_string(),
            "a secret already exists at this path; nothing was written"
        );
    }

    #[tokio::test]
    async fn another_rejected_create_is_not_mistaken_for_an_existing_secret() {
        let mut server = Server::new_async().await;
        server
            .mock("POST", "/v1/secret/data/app/db")
            .with_status(400)
            .with_body(r#"{"errors":["no data provided"]}"#)
            .create_async()
            .await;

        let err = engine(&server)
            .put("/app/db", data(json!({"k": "v"})), WriteMode::Create)
            .await
            .unwrap_err();
        assert_eq!(err.to_string(), "Invalid request");
    }

    #[tokio::test]
    async fn an_update_is_written_without_a_condition() {
        // Saving an edit of an existing secret is not check-and-set
        let mut server = Server::new_async().await;
        let mock = server
            .mock("POST", "/v1/secret/data/app/db")
            .match_body(Matcher::Json(json!({"data": {"k": "v2"}})))
            .with_status(200)
            .with_body(r#"{"data":{"version":2}}"#)
            .create_async()
            .await;

        engine(&server)
            .put("/app/db", data(json!({"k": "v2"})), WriteMode::Update)
            .await
            .unwrap();

        mock.assert_async().await;
    }
}
