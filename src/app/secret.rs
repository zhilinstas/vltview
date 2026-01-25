use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;

use crate::state::AppState;
use crate::vault::VaultClient;

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

    pub async fn get_version(&self, path: &str, version: u64) -> Result<HashMap<String, String>> {
        let secret = self.vault.kv().get_version(path, Some(version)).await?;
        let mut result = HashMap::new();

        for (key, value) in secret.data {
            if let Some(s) = value.as_str() {
                result.insert(key, s.to_string());
            }
        }

        Ok(result)
    }

    pub async fn save(&self, path: &str, data: HashMap<String, String>) -> Result<()> {
        self.vault.kv().put(path, data).await?;
        self.state.invalidate_cache(path);
        Ok(())
    }

    pub async fn delete(&self, path: &str) -> Result<()> {
        self.vault.kv().delete(path).await?;
        self.state.invalidate_cache(path);
        Ok(())
    }
}
