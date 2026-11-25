use anyhow::Result;
use std::sync::Arc;

use crate::state::AppState;
use crate::vault::VaultClient;

/// Parent of a Vault path: `/a/b` gives `/a`, `/a` and `/` give `/`
pub fn parent_path(path: &str) -> String {
    match path.trim_end_matches('/').rfind('/') {
        Some(pos) if pos > 0 => path[..pos].to_string(),
        _ => "/".to_string(),
    }
}

/// Path of `name` inside the folder `dir`
pub fn child_path(dir: &str, name: &str) -> String {
    format!(
        "{}/{}",
        dir.trim_end_matches('/'),
        name.trim_end_matches('/')
    )
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parent_and_child_paths() {
        assert_eq!(parent_path("/a/b"), "/a");
        assert_eq!(parent_path("/a"), "/");
        assert_eq!(parent_path("/"), "/");
        assert_eq!(parent_path("a/b"), "a");
        assert_eq!(parent_path("a"), "/");

        assert_eq!(child_path("/", "a"), "/a");
        assert_eq!(child_path("/a", "b/"), "/a/b");
        assert_eq!(parent_path(&child_path("/a", "b")), "/a");
    }
}
