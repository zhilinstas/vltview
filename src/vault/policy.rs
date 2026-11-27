use anyhow::{Context, Result};
use reqwest::{Client, RequestBuilder};
use std::sync::Arc;
use tokio::sync::Semaphore;

use super::audit_log;
use super::auth::AuthManager;
use super::request::{authorize, ensure_success};
use super::sanitize::vault_url;

/// Policy API client for ACL policies with rate limiting
#[derive(Clone)]
pub struct PolicyApi {
    client: Client,
    base_url: String,
    auth: Arc<AuthManager>,
    namespace: Option<String>,
    rate_limiter: Arc<Semaphore>,
    auth_audit: audit_log::AuthAudit,
}

impl PolicyApi {
    /// Create a new policy API client
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

    /// Add the token and namespace headers
    fn authorize(&self, req: RequestBuilder) -> RequestBuilder {
        authorize(req, &self.auth, self.namespace.as_deref())
    }

    /// The `data` object of a successful answer
    async fn data(&self, req: RequestBuilder) -> Result<serde_json::Value> {
        let response = self.authorize(req).send().await?;
        let response = ensure_success(response).await?;
        let mut body: serde_json::Value = response.json().await?;
        self.auth_audit.accepted();
        Ok(body["data"].take())
    }

    /// List all policies
    pub async fn list(&self) -> Result<Vec<String>> {
        // Acquire rate limit permit
        let _permit = self.rate_limiter.acquire().await?;

        let url = vault_url(&self.base_url, &["v1", "sys", "policies", "acl"], "/")?;

        // Vault LIST endpoint requires the "LIST" HTTP method
        let req = self
            .client
            .request(reqwest::Method::from_bytes(b"LIST")?, url);
        let data = self.data(req).await?;

        // An answer without the list is not "no policies": saying so would
        // show an empty screen for what is a broken answer
        let names: Vec<String> = data["keys"]
            .as_array()
            .context("Vault's answer holds no policy list")?
            .iter()
            .filter_map(|name| name.as_str().map(String::from))
            .collect();

        audit_log::log_policy_list(names.len());
        Ok(names)
    }

    /// Read a specific policy
    pub async fn read(&self, name: &str) -> Result<String> {
        // Acquire rate limit permit
        let _permit = self.rate_limiter.acquire().await?;

        // The name is a single, percent-encoded URL segment
        let url = vault_url(&self.base_url, &["v1", "sys", "policies", "acl", name], "/")?;
        let data = self.data(self.client.get(url)).await?;

        let policy = data["policy"]
            .as_str()
            .context("Vault's answer holds no policy text")?;

        audit_log::log_policy_read(name);
        Ok(policy.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::{Server, ServerGuard};

    fn api(base_url: String) -> PolicyApi {
        PolicyApi::new(
            Client::new(),
            base_url.clone(),
            Arc::new(AuthManager::new("test-token".to_string().into())),
            None,
            Arc::new(Semaphore::new(4)),
            audit_log::AuthAudit::new(&base_url),
        )
    }

    #[tokio::test]
    async fn list_works_when_the_address_ends_in_a_slash() {
        // The list URL was glued together with `format!` and relied on the
        // config having trimmed the slash. It is built like every other URL.
        let mut server = Server::new_async().await;
        let mock = server
            .mock("LIST", "/v1/sys/policies/acl")
            .match_header("X-Vault-Token", "test-token")
            .with_status(200)
            .with_body(r#"{"data":{"keys":["default","readonly"]}}"#)
            .create_async()
            .await;

        let policies = api(format!("{}/", server.url())).list().await.unwrap();

        mock.assert_async().await;
        assert_eq!(policies, ["default", "readonly"]);
    }

    async fn server_answering(method: &str, path: &str, status: usize, body: &str) -> ServerGuard {
        let mut server = Server::new_async().await;
        server
            .mock(method, path)
            .with_status(status)
            .with_body(body)
            .create_async()
            .await;
        server
    }

    #[tokio::test]
    async fn a_refused_or_failed_list_is_an_error_not_an_empty_list() {
        // An empty result would be shown as "No policies found"
        for (status, expected) in [
            (403, "Access denied - insufficient permissions"),
            (500, "Vault server error - please contact administrator"),
        ] {
            let server = server_answering(
                "LIST",
                "/v1/sys/policies/acl",
                status,
                r#"{"errors":["permission denied"]}"#,
            )
            .await;

            let error = api(server.url()).list().await.unwrap_err();
            assert_eq!(error.to_string(), expected);
        }
    }

    #[tokio::test]
    async fn read_returns_the_policy_and_sends_the_token() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/v1/sys/policies/acl/readonly")
            .match_header("X-Vault-Token", "test-token")
            .with_status(200)
            .with_body(r#"{"data":{"name":"readonly","policy":"path \"secret/*\" {}"}}"#)
            .create_async()
            .await;

        let policy = api(server.url()).read("readonly").await.unwrap();

        mock.assert_async().await;
        assert_eq!(policy, r#"path "secret/*" {}"#);
    }

    #[tokio::test]
    async fn a_refused_or_failed_read_is_an_error_not_an_empty_policy() {
        for (status, expected) in [
            (403, "Access denied - insufficient permissions"),
            (500, "Vault server error - please contact administrator"),
        ] {
            let server = server_answering(
                "GET",
                "/v1/sys/policies/acl/readonly",
                status,
                r#"{"errors":["permission denied"]}"#,
            )
            .await;

            let error = api(server.url()).read("readonly").await.unwrap_err();
            assert_eq!(error.to_string(), expected);
        }
    }

    #[tokio::test]
    async fn an_answer_without_the_expected_data_is_an_error() {
        // 200 with a body that is not what the endpoint returns (a proxy, a
        // login page turned into JSON): not "no policies" and not an empty
        // policy
        let server = server_answering("LIST", "/v1/sys/policies/acl", 200, r#"{"data":{}}"#).await;
        assert!(api(server.url()).list().await.is_err());

        let server = server_answering(
            "GET",
            "/v1/sys/policies/acl/readonly",
            200,
            r#"{"data":{}}"#,
        )
        .await;
        assert!(api(server.url()).read("readonly").await.is_err());
    }

    #[tokio::test]
    async fn a_policy_name_cannot_change_which_endpoint_is_called() {
        // Path and query characters in a name stay inside the one URL
        // segment that names the policy
        let mut server = Server::new_async().await;
        let policy = server
            .mock(
                "GET",
                "/v1/sys/policies/acl/a%2F..%2F..%2Fauth%2Ftoken%3Flist=true%23x",
            )
            .with_status(200)
            .with_body(r#"{"data":{"policy":"ok"}}"#)
            .expect(1)
            .create_async()
            .await;
        let anything_else = server
            .mock(
                "GET",
                mockito::Matcher::Regex("^/v1/(auth|sys/policies/acl\\?)".into()),
            )
            .expect(0)
            .create_async()
            .await;

        let content = api(server.url())
            .read("a/../../auth/token?list=true#x")
            .await
            .unwrap();

        policy.assert_async().await;
        anything_else.assert_async().await;
        assert_eq!(content, "ok");
    }
}
