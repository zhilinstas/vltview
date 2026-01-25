use anyhow::Result;
use std::sync::Arc;

use crate::vault::VaultClient;

/// Policy service for viewing ACL policies
#[derive(Clone)]
pub struct PolicyService {
    vault: Arc<VaultClient>,
}

impl PolicyService {
    pub fn new(vault: Arc<VaultClient>) -> Self {
        Self { vault }
    }

    pub async fn list(&self) -> Result<Vec<String>> {
        self.vault.policy().list().await
    }

    pub async fn read(&self, name: &str) -> Result<String> {
        self.vault.policy().read(name).await
    }
}
