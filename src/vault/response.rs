use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Vault API response wrapper
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultResponse<T> {
    /// Request ID
    #[serde(default)]
    pub request_id: String,

    /// Lease ID
    #[serde(default)]
    pub lease_id: String,

    /// Whether this is renewable
    #[serde(default)]
    pub renewable: bool,

    /// Lease duration in seconds
    #[serde(default)]
    pub lease_duration: u64,

    /// Response data
    pub data: T,

    /// Warnings
    #[serde(default)]
    pub warnings: Option<Vec<String>>,

    /// Auth information
    #[serde(default)]
    pub auth: Option<AuthInfo>,
}

/// Authentication information in response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthInfo {
    pub client_token: String,
    pub accessor: String,
    pub policies: Vec<String>,
    pub token_policies: Vec<String>,
    pub metadata: Option<HashMap<String, String>>,
    pub lease_duration: u64,
    pub renewable: bool,
}

/// Vault secret data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultSecret {
    /// Secret data (for KV2, this contains another nested data field)
    pub data: HashMap<String, serde_json::Value>,

    /// Metadata about the secret
    #[serde(default)]
    pub metadata: Option<SecretMetadata>,
}

/// Secret metadata (for KV2) - returned with data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretMetadata {
    /// Creation time
    #[serde(default)]
    pub created_time: String,

    /// Deletion time (if deleted)
    #[serde(default)]
    pub deletion_time: String,

    /// Whether this version is destroyed
    #[serde(default)]
    pub destroyed: bool,

    /// Version number
    #[serde(default)]
    pub version: u64,
}

/// Full metadata for a secret (all versions)
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FullSecretMetadata {
    pub created_time: String,
    pub current_version: u64,
    pub max_versions: u64,
    pub oldest_version: u64,
    pub updated_time: String,
    pub versions: HashMap<String, VersionMetadata>,
    // Optional fields that may be present
    #[serde(default)]
    pub cas_required: bool,
    #[serde(default)]
    pub custom_metadata: Option<HashMap<String, String>>,
    #[serde(default)]
    pub delete_version_after: String,
    #[serde(default)]
    pub last_updated_by: Option<serde_json::Value>,
}

/// Individual version metadata
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VersionMetadata {
    pub created_time: String,
    #[serde(default)]
    pub deletion_time: String,
    pub destroyed: bool,
    // Optional fields
    #[serde(default)]
    pub created_by: Option<serde_json::Value>,
    #[serde(default)]
    pub deleted_by: Option<serde_json::Value>,
}

impl FullSecretMetadata {
    /// Get sorted list of version numbers (newest first)
    pub fn version_list(&self) -> Vec<u64> {
        let mut versions: Vec<u64> = self
            .versions
            .keys()
            .filter_map(|k| k.parse::<u64>().ok())
            .collect();
        versions.sort_by(|a, b| b.cmp(a)); // Sort descending (newest first)
        versions
    }

    /// Check if a version is deleted or destroyed
    pub fn is_version_deleted(&self, version: u64) -> bool {
        self.versions
            .get(&version.to_string())
            .map(|v| !v.deletion_time.is_empty() || v.destroyed)
            .unwrap_or(false)
    }
}

/// List response data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListResponse {
    /// List of keys
    pub keys: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vault_response_deserialization() {
        let json = r#"{
            "request_id": "test-id",
            "lease_id": "",
            "renewable": false,
            "lease_duration": 0,
            "data": {
                "key": "value"
            }
        }"#;

        let result: Result<VaultResponse<HashMap<String, String>>, _> = serde_json::from_str(json);
        assert!(result.is_ok());

        let response = result.unwrap();
        assert_eq!(response.request_id, "test-id");
    }

    #[test]
    fn test_list_response() {
        let json = r#"{
            "keys": ["secret1", "secret2", "secret3"]
        }"#;

        let result: Result<ListResponse, _> = serde_json::from_str(json);
        assert!(result.is_ok());

        let list = result.unwrap();
        assert_eq!(list.keys.len(), 3);
        assert_eq!(list.keys[0], "secret1");
    }

    #[test]
    fn test_vault_secret() {
        let json = r#"{
            "data": {
                "username": "admin",
                "password": "secret123"
            },
            "metadata": {
                "created_time": "2025-01-01T00:00:00Z",
                "version": 1,
                "destroyed": false
            }
        }"#;

        let result: Result<VaultSecret, _> = serde_json::from_str(json);
        assert!(result.is_ok());

        let secret = result.unwrap();
        assert_eq!(secret.data.len(), 2);
        assert!(secret.metadata.is_some());
    }

    #[test]
    fn test_full_secret_metadata_deserialization() {
        // Test with actual Vault API response format including all optional fields
        let json = r#"{
            "created_time": "2026-01-20T14:47:07.744998776Z",
            "current_version": 2,
            "custom_metadata": null,
            "delete_version_after": "0s",
            "last_updated_by": {
                "actor": "token",
                "operation": "update",
                "client_id": "abc123"
            },
            "max_versions": 0,
            "oldest_version": 0,
            "updated_time": "2026-01-20T15:06:09.514204046Z",
            "cas_required": false,
            "versions": {
                "1": {
                    "created_by": {
                        "actor": "token",
                        "operation": "create",
                        "client_id": "abc123"
                    },
                    "created_time": "2026-01-20T14:47:07.744998776Z",
                    "deleted_by": null,
                    "deletion_time": "",
                    "destroyed": false
                },
                "2": {
                    "created_by": {
                        "actor": "token",
                        "operation": "update",
                        "client_id": "abc123"
                    },
                    "created_time": "2026-01-20T15:06:09.514204046Z",
                    "deleted_by": null,
                    "deletion_time": "",
                    "destroyed": false
                }
            }
        }"#;

        let result: Result<FullSecretMetadata, _> = serde_json::from_str(json);
        assert!(result.is_ok(), "Failed to deserialize: {:?}", result.err());

        let metadata = result.unwrap();
        assert_eq!(metadata.current_version, 2);
        assert_eq!(metadata.versions.len(), 2);
        assert_eq!(metadata.created_time, "2026-01-20T14:47:07.744998776Z");
        assert_eq!(metadata.updated_time, "2026-01-20T15:06:09.514204046Z");
        assert!(!metadata.cas_required);
        assert_eq!(metadata.delete_version_after, "0s");
        assert!(metadata.last_updated_by.is_some());
        assert!(metadata.custom_metadata.is_none());

        // Test version_list method
        let version_list = metadata.version_list();
        assert_eq!(version_list, vec![2, 1]); // Sorted descending

        // Test is_version_deleted method
        assert!(!metadata.is_version_deleted(1));
        assert!(!metadata.is_version_deleted(2));
    }

    #[test]
    fn test_full_secret_metadata_minimal() {
        // Test with minimal fields (no optional fields)
        let json = r#"{
            "created_time": "2026-01-20T14:47:07.744998776Z",
            "current_version": 1,
            "max_versions": 0,
            "oldest_version": 0,
            "updated_time": "2026-01-20T15:06:09.514204046Z",
            "versions": {
                "1": {
                    "created_time": "2026-01-20T14:47:07.744998776Z",
                    "destroyed": false
                }
            }
        }"#;

        let result: Result<FullSecretMetadata, _> = serde_json::from_str(json);
        assert!(
            result.is_ok(),
            "Failed to deserialize minimal metadata: {:?}",
            result.err()
        );

        let metadata = result.unwrap();
        assert_eq!(metadata.current_version, 1);
        assert!(!metadata.cas_required); // Should default to false
        assert!(metadata.custom_metadata.is_none());
        assert_eq!(metadata.delete_version_after, ""); // Should default to empty string
        assert!(metadata.last_updated_by.is_none());
    }
}
