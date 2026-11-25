//! Audit log of the session: which action was performed on what.
//!
//! Every entry carries its `action` and what it applied to as structured
//! fields; the log screen shows them. Tokens and secret values are never
//! logged, only paths, field names and counts.
use std::collections::HashMap;
use std::sync::{Arc, Once};

/// Redact sensitive information from paths and data for logging
pub fn redact_secret_value(value: &str) -> String {
    if value.len() <= 8 {
        "***".to_string()
    } else {
        format!("{}...{}", &value[..4], &value[value.len() - 4..])
    }
}

/// Redact secret data for logging - shows keys but not values
pub fn redact_secret_data<V>(data: &HashMap<String, V>) -> HashMap<String, String> {
    data.keys()
        .map(|k| (k.clone(), "***REDACTED***".to_string()))
        .collect()
}

/// Log successful secret read operation
pub fn log_secret_read(path: &str) {
    tracing::info!(action = "secret.read", path = %path);
}

/// Log secret list operation
pub fn log_secret_list(path: &str, count: usize) {
    tracing::info!(action = "secret.list", path = %path, count = count);
}

/// Log secret write operation
pub fn log_secret_write(path: &str, field_count: usize) {
    tracing::info!(action = "secret.write", path = %path, fields = field_count);
}

/// Log secret delete operation
pub fn log_secret_delete(path: &str) {
    tracing::warn!(action = "secret.delete", path = %path);
}

/// Log policy read operation
pub fn log_policy_read(policy_name: &str) {
    tracing::info!(action = "policy.read", policy = %policy_name);
}

/// Log policy list operation
pub fn log_policy_list(count: usize) {
    tracing::info!(action = "policy.list", count = count);
}

/// Logs "authenticated" once per client, when Vault first accepts a request
/// made with the token. Creating the client proves nothing: it sends nothing.
///
/// Clones share the "already said" state, so the APIs of one client say it
/// once between them.
#[derive(Clone)]
pub struct AuthAudit {
    vault_addr: Arc<str>,
    said: Arc<Once>,
}

impl AuthAudit {
    pub fn new(vault_addr: &str) -> Self {
        Self {
            vault_addr: Arc::from(vault_addr),
            said: Arc::new(Once::new()),
        }
    }

    /// Vault has answered a request with success. The token is never logged,
    /// only that it was accepted.
    pub fn accepted(&self) {
        self.said.call_once(|| {
            tracing::info!(action = "auth.success", vault_addr = %self.vault_addr);
        });
    }
}

/// Log clipboard operations (for audit trail)
pub fn log_clipboard_copy(data_size: usize, timeout_secs: u64) {
    tracing::info!(
        action = "clipboard.copy",
        bytes = data_size,
        timeout_secs = timeout_secs
    );
}

/// Log security-relevant events
pub fn log_security_event(event: &str, details: &str) {
    tracing::warn!(action = %event, "{details}");
}

#[cfg(test)]
mod tests {
    use crate::config::Config;
    use crate::tui::log_buffer::{LogEntry, capture};
    use crate::vault::{SecretData, VaultClient};
    use mockito::{Matcher, Server};
    use serde_json::json;

    const TOKEN: &str = "hvs.TOKEN-in-audit-tests";

    fn client(vault_addr: &str) -> VaultClient {
        VaultClient::new(Config {
            vault_addr: vault_addr.to_string(),
            vault_token: TOKEN.to_string().into(),
            ..Config::default()
        })
        .unwrap()
    }

    fn data(value: serde_json::Value) -> SecretData {
        serde_json::from_value(value).unwrap()
    }

    /// The one entry recording `action`
    fn entry<'a>(entries: &'a [LogEntry], action: &str) -> &'a LogEntry {
        let mut found = entries.iter().filter(|e| e.field("action") == Some(action));
        let entry = found
            .next()
            .unwrap_or_else(|| panic!("no {action} entry in {entries:#?}"));
        assert!(found.next().is_none(), "more than one {action} entry");
        entry
    }

    fn count(entries: &[LogEntry], action: &str) -> usize {
        entries
            .iter()
            .filter(|e| e.field("action") == Some(action))
            .count()
    }

    #[tokio::test]
    async fn secret_operations_are_logged_with_action_full_path_and_count() {
        let mut server = Server::new_async().await;
        server
            .mock("GET", "/v1/secret/metadata/team/billing/prod")
            .match_query(Matcher::UrlEncoded("list".into(), "true".into()))
            .with_body(r#"{"data":{"keys":["db","queue","cache/"]}}"#)
            .create_async()
            .await;
        server
            .mock("GET", "/v1/secret/data/team/billing/prod/db")
            .with_body(r#"{"data":{"data":{"user":"app"},"metadata":{"version":1}}}"#)
            .create_async()
            .await;
        server
            .mock("POST", "/v1/secret/data/team/billing/prod/db")
            .with_body(r#"{"data":{"version":2}}"#)
            .create_async()
            .await;
        server
            .mock("DELETE", "/v1/secret/metadata/team/billing/prod/db")
            .with_status(204)
            .create_async()
            .await;

        let (buffer, _guard) = capture();
        let client = client(&server.url());
        client.kv().list("/team/billing/prod/").await.unwrap();
        client.kv().get("/team/billing/prod/db").await.unwrap();
        client
            .kv()
            .put(
                "/team/billing/prod/db",
                data(json!({"user": "app", "port": 5432})),
                crate::vault::WriteMode::Update,
            )
            .await
            .unwrap();
        client.kv().delete("/team/billing/prod/db").await.unwrap();

        let entries = buffer.entries();

        let listed = entry(&entries, "secret.list");
        assert_eq!(listed.field("path"), Some("/team/billing/prod"));
        assert_eq!(listed.field("count"), Some("3"));

        let read = entry(&entries, "secret.read");
        assert_eq!(read.field("path"), Some("/team/billing/prod/db"));

        let written = entry(&entries, "secret.write");
        assert_eq!(written.field("path"), Some("/team/billing/prod/db"));
        assert_eq!(written.field("fields"), Some("2"));

        let deleted = entry(&entries, "secret.delete");
        assert_eq!(deleted.field("path"), Some("/team/billing/prod/db"));
        assert_eq!(deleted.level, "WARN");
    }

    #[tokio::test]
    async fn policy_operations_are_logged_with_action_name_and_count() {
        let mut server = Server::new_async().await;
        server
            .mock("LIST", "/v1/sys/policies/acl")
            .with_body(r#"{"data":{"keys":["default","readonly"]}}"#)
            .create_async()
            .await;
        server
            .mock("GET", "/v1/sys/policies/acl/readonly")
            .with_body(r#"{"data":{"name":"readonly","policy":"path \"secret/*\" {}"}}"#)
            .create_async()
            .await;

        let (buffer, _guard) = capture();
        let client = client(&server.url());
        client.policy().list().await.unwrap();
        client.policy().read("readonly").await.unwrap();

        let entries = buffer.entries();
        assert_eq!(entry(&entries, "policy.list").field("count"), Some("2"));
        assert_eq!(
            entry(&entries, "policy.read").field("policy"),
            Some("readonly")
        );
    }

    #[tokio::test]
    async fn authenticated_is_logged_once_when_vault_first_accepts_a_request() {
        let mut server = Server::new_async().await;
        server
            .mock("GET", "/v1/secret/metadata/denied")
            .match_query(Matcher::Any)
            .with_status(403)
            .with_body(r#"{"errors":["permission denied"]}"#)
            .create_async()
            .await;
        server
            .mock("GET", "/v1/secret/metadata/app")
            .match_query(Matcher::Any)
            .with_body(r#"{"data":{"keys":["db"]}}"#)
            .create_async()
            .await;
        server
            .mock("LIST", "/v1/sys/policies/acl")
            .with_body(r#"{"data":{"keys":["default"]}}"#)
            .create_async()
            .await;

        let (buffer, _guard) = capture();
        let client = client(&server.url());
        assert_eq!(
            count(&buffer.entries(), "auth.success"),
            0,
            "creating the client sends nothing"
        );

        client.kv().list("/denied").await.unwrap_err();
        assert_eq!(count(&buffer.entries(), "auth.success"), 0);

        client.kv().list("/app").await.unwrap();
        let entries = buffer.entries();
        let authenticated = entry(&entries, "auth.success");
        assert_eq!(
            authenticated.field("vault_addr"),
            Some(server.url().as_str())
        );

        // Said once for the client, whichever API the later requests use
        client.kv().list("/app").await.unwrap();
        client.policy().list().await.unwrap();
        assert_eq!(count(&buffer.entries(), "auth.success"), 1);
    }

    #[tokio::test]
    async fn an_unreachable_vault_is_never_called_authenticated() {
        // Nothing listens on port 1
        let (buffer, _guard) = capture();
        let client = client("http://127.0.0.1:1");

        client.kv().list("/").await.unwrap_err();
        client.policy().list().await.unwrap_err();

        assert_eq!(count(&buffer.entries(), "auth.success"), 0);
    }

    #[tokio::test]
    async fn not_found_does_not_prove_that_the_token_was_accepted() {
        // Any web server answers 404; the listing is empty either way
        let mut server = Server::new_async().await;
        server
            .mock("GET", "/v1/secret/metadata/nothing")
            .match_query(Matcher::Any)
            .with_status(404)
            .create_async()
            .await;

        let (buffer, _guard) = capture();
        let client = client(&server.url());
        client.kv().list("/nothing").await.unwrap();

        let entries = buffer.entries();
        assert_eq!(entry(&entries, "secret.list").field("count"), Some("0"));
        assert_eq!(count(&entries, "auth.success"), 0);
    }

    #[tokio::test]
    async fn no_entry_contains_a_secret_value_or_the_token() {
        const WRITTEN: &str = "written-value-5f1c";
        const NESTED: &str = "nested-value-88ab";
        const STORED: &str = "stored-value-c07d";

        let mut server = Server::new_async().await;
        server
            .mock("POST", "/v1/secret/data/app/db")
            .with_body(r#"{"data":{"version":2}}"#)
            .create_async()
            .await;
        server
            .mock("GET", "/v1/secret/data/app/db")
            .with_body(format!(
                r#"{{"data":{{"data":{{"password":"{STORED}"}},"metadata":{{"version":2}}}}}}"#
            ))
            .create_async()
            .await;
        server
            .mock("POST", "/v1/secret/data/app/readonly")
            .with_status(403)
            .with_body(r#"{"errors":["permission denied"]}"#)
            .create_async()
            .await;

        let (buffer, _guard) = capture();
        let client = client(&server.url());
        let secret = data(json!({"password": WRITTEN, "tls": {"key": NESTED}}));
        client
            .kv()
            .put("/app/db", secret.clone(), crate::vault::WriteMode::Update)
            .await
            .unwrap();
        client
            .kv()
            .put("/app/readonly", secret, crate::vault::WriteMode::Update)
            .await
            .unwrap_err();
        let read = client.kv().get("/app/db").await.unwrap();
        assert_eq!(read.data["password"], json!(STORED));

        let entries = buffer.entries();
        assert_eq!(entry(&entries, "secret.write").field("fields"), Some("2"));
        for entry in &entries {
            let text = format!("{entry:?}");
            for secret in [WRITTEN, NESTED, STORED, TOKEN] {
                assert!(!text.contains(secret), "{secret} is in the log: {text}");
            }
        }
        // Field names are not secret
        assert!(
            entries.iter().any(|e| e.message.contains("password")),
            "{entries:#?}"
        );
    }
}
