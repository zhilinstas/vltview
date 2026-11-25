use anyhow::Result;
use serde_json::Value;
use std::sync::Arc;

use crate::state::AppState;
use crate::vault::{SecretData, VaultClient, WriteMode};

/// Text shown (and edited) for a secret value.
///
/// Strings are shown as they are. Every other JSON type is shown as compact
/// JSON, so `5432`, `true`, `null` and `{"a":1}` stay recognisable.
pub fn display_value(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// What every masked value looks like.
///
/// It is the same for all of them: a mask that keeps a character, or has
/// one dot per character, tells someone looking at the screen how long the
/// value is and how it starts and ends.
pub const MASK: &str = "••••••••";

/// How a value is shown while it is masked. An empty value has nothing to
/// hide and stays empty, so that "no value" can be told from "hidden".
pub fn masked(text: &str) -> &'static str {
    if text.is_empty() { "" } else { MASK }
}

/// Name of a value's JSON type, for messages
pub fn json_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Turn edited text back into the value to store.
///
/// `original` is the value the field had when it was loaded, if it was not a
/// string. The rules keep a read-modify-write cycle lossless and never change
/// a value's type behind the user's back:
/// - a field that was a string, or is new, is always saved as a string;
/// - a typed field whose text is unchanged is saved exactly as it was;
/// - a typed field that was edited must still be valid JSON (`5433`, `false`,
///   `[1,2]`, or `"text"` in quotes to turn it into a string). Anything else
///   is an error, so a typo cannot silently turn an array into a string.
pub fn edited_value(text: &str, original: Option<&Value>) -> Result<Value, serde_json::Error> {
    match original {
        Some(original) if display_value(original) == text => Ok(original.clone()),
        Some(_) => serde_json::from_str(text),
        None => Ok(Value::String(text.to_string())),
    }
}

/// Secret service for CRUD operations on secrets
#[derive(Clone)]
pub struct SecretService {
    vault: Arc<VaultClient>,
    state: Arc<AppState>,
}

impl SecretService {
    pub fn new(vault: Arc<VaultClient>, state: Arc<AppState>) -> Self {
        Self { vault, state }
    }

    pub async fn get_version(&self, path: &str, version: u64) -> Result<SecretData> {
        let secret = self.vault.kv().get_version(path, Some(version)).await?;
        Ok(secret.data)
    }

    /// Write a secret. A create is refused if the path is already taken.
    pub async fn save(&self, path: &str, data: SecretData, mode: WriteMode) -> Result<()> {
        self.vault.kv().put(path, data, mode).await?;
        self.state.invalidate_cache(path);
        Ok(())
    }

    pub async fn delete(&self, path: &str) -> Result<()> {
        self.vault.kv().delete(path).await?;
        self.state.invalidate_cache(path);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn display_shows_strings_raw_and_other_types_as_json() {
        assert_eq!(display_value(&json!("plain")), "plain");
        assert_eq!(display_value(&json!(5432)), "5432");
        assert_eq!(display_value(&json!(true)), "true");
        assert_eq!(display_value(&json!(null)), "null");
        assert_eq!(display_value(&json!({"a": [1, 2]})), r#"{"a":[1,2]}"#);
    }

    #[test]
    fn strings_and_new_fields_are_always_saved_as_strings() {
        // Text that looks like JSON, or like broken JSON, stays text
        for text in ["5432", "true", "null", "[oops"] {
            assert_eq!(edited_value(text, None).unwrap(), json!(text));
        }
    }

    #[test]
    fn untouched_typed_fields_round_trip_exactly() {
        for original in [
            json!(5432),
            json!(1.50),
            json!(true),
            json!(null),
            json!(["a", 1]),
            json!({"nested": {"k": false}}),
        ] {
            let text = display_value(&original);
            assert_eq!(edited_value(&text, Some(&original)).unwrap(), original);
        }
    }

    #[test]
    fn edited_typed_fields_must_stay_valid_json() {
        let port = json!(5432);
        let edit = |text, original| edited_value(text, Some(original));

        assert_eq!(edit("5433", &port).unwrap(), json!(5433));
        assert_eq!(edit("false", &json!(true)).unwrap(), json!(false));
        // Quoting is how a typed field is deliberately turned into a string
        assert_eq!(edit(r#""5432""#, &port).unwrap(), json!("5432"));

        // A typo must not silently turn the value into a string
        assert!(edit("5433a", &port).is_err());
        assert!(edit(r#"["a","b"3"#, &json!(["a", "b"])).is_err());
    }

    #[test]
    fn a_masked_value_gives_away_neither_characters_nor_length() {
        // Regression: the mask kept the first and last character and had
        // one dot per character in between.
        let values = [
            "a",
            "abcd",
            "password123",
            "a-very-long-value-that-goes-on-and-on-0123456789",
            "line one\nline two\nline three",
            "пароль✓",
            "🔑secret🔑",
            " ",
        ];
        for value in values {
            assert_eq!(masked(value), MASK, "{value:?}");
        }
        assert!(MASK.chars().all(|c| c == '•'));

        // Nothing to hide, and "no value" is worth seeing
        assert_eq!(masked(""), "");
    }
}
