//! What every request to Vault has in common.

use anyhow::Result;
use reqwest::{RequestBuilder, Response};

use super::auth::AuthManager;
use super::sanitize::sanitize_vault_error;

/// Add the token and, if there is one, the namespace header
pub fn authorize(
    req: RequestBuilder,
    auth: &AuthManager,
    namespace: Option<&str>,
) -> RequestBuilder {
    let req = auth.add_auth_header(req);
    match namespace {
        Some(namespace) => req.header("X-Vault-Namespace", namespace),
        None => req,
    }
}

/// Pass a successful response through; turn any other status into a
/// sanitized error. Every response goes through here: a request that Vault
/// refuses must reach the UI as an error, never as an empty result.
pub async fn ensure_success(response: Response) -> Result<Response> {
    if response.status().is_success() {
        return Ok(response);
    }

    let status = response.status();
    let error_text = response
        .text()
        .await
        .unwrap_or_else(|_| "Unknown error".to_string());
    anyhow::bail!("{}", sanitize_vault_error(status.as_u16(), &error_text));
}
