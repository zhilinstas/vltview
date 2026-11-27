use anyhow::Result;
use std::sync::Arc;

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

/// Lists folders in Vault.
///
/// It only reads: what is on screen changes in `AppState`, on the UI side,
/// when a listing that is still wanted arrives (see `Root`).
#[derive(Clone)]
pub struct NavigationController {
    vault: Arc<VaultClient>,
}

impl NavigationController {
    pub fn new(vault: Arc<VaultClient>) -> Self {
        Self { vault }
    }

    /// Names in the folder `path`; folders end in `/`
    pub async fn list(&self, path: &str) -> Result<Vec<String>> {
        self.vault.kv().list(path).await
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
