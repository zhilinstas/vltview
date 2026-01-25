use anyhow::Result;
use std::sync::Arc;

use crate::state::AppState;
use crate::vault::VaultClient;

/// Navigation controller for path traversal
#[derive(Clone)]
pub struct NavigationController {
    state: Arc<AppState>,
    vault: Arc<VaultClient>,
}

impl NavigationController {
    pub fn new(state: Arc<AppState>, vault: Arc<VaultClient>) -> Self {
        Self { state, vault }
    }

    pub async fn navigate_to(&self, path: String) -> Result<()> {
        // Fetch items for the new path first
        // If this fails, we don't update the path, keeping state consistent
        let items = self.vault.kv().list(&path).await?;

        // Only update state if fetch succeeded
        self.state.set_path(path.clone());
        self.state.set_list_items(items);

        Ok(())
    }

    pub async fn refresh(&self) -> Result<()> {
        let path = self.state.current_path();
        // Fetch items for current path
        let items = self.vault.kv().list(&path).await?;
        self.state.set_list_items(items);
        Ok(())
    }
}
