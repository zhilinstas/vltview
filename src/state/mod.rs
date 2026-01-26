use std::collections::{HashMap, VecDeque};
use std::sync::RwLock;

/// Application state manager
pub struct AppState {
    /// Current path in Vault
    path: RwLock<PathState>,

    /// Secret cache
    cache: RwLock<CacheManager>,

    /// Navigation history
    history: RwLock<HistoryManager>,
}

impl AppState {
    /// Create new application state
    pub fn new(initial_path: String) -> Self {
        Self {
            path: RwLock::new(PathState::new(initial_path)),
            cache: RwLock::new(CacheManager::new()),
            history: RwLock::new(HistoryManager::new()),
        }
    }

    /// Get current path
    pub fn current_path(&self) -> String {
        self.path.read().unwrap().current.clone()
    }

    /// Set current path
    pub fn set_path(&self, path: String) {
        let mut path_state = self.path.write().unwrap();
        let old_path = path_state.current.clone();
        path_state.current = path;

        // Add to history
        drop(path_state);
        self.history.write().unwrap().push(old_path);
    }

    /// Invalidate cache for path
    pub fn invalidate_cache(&self, path: &str) {
        self.cache.write().unwrap().invalidate(path);
    }

    /// Clear all cache
    pub fn clear_cache(&self) {
        self.cache.write().unwrap().clear();
    }

    /// Go back in history
    pub fn go_back(&self) -> Option<String> {
        self.history.write().unwrap().back()
    }

    /// Go forward in history
    pub fn go_forward(&self) -> Option<String> {
        self.history.write().unwrap().forward()
    }

    /// Get current list items
    pub fn list_items(&self) -> Vec<String> {
        self.path.read().unwrap().items.clone()
    }

    /// Set list items
    pub fn set_list_items(&self, items: Vec<String>) {
        self.path.write().unwrap().items = items;
    }
}

/// Path state tracking
#[derive(Debug, Clone)]
pub struct PathState {
    /// Current path in Vault
    pub current: String,

    /// Items at current path
    pub items: Vec<String>,
}

impl PathState {
    pub fn new(initial_path: String) -> Self {
        Self {
            current: Self::normalize_path(initial_path),
            items: Vec::new(),
        }
    }

    /// Normalize path (ensure leading slash, no trailing slash unless root)
    fn normalize_path(path: String) -> String {
        let mut normalized = path.trim().to_string();

        // Ensure leading slash
        if !normalized.starts_with('/') {
            normalized = format!("/{}", normalized);
        }

        // Remove trailing slash unless it's root
        if normalized.len() > 1 && normalized.ends_with('/') {
            normalized.pop();
        }

        normalized
    }
}


/// Cache manager for secrets
#[derive(Debug)]
pub struct CacheManager {
    /// Cache storage (path -> invalidation marker)
    cache: HashMap<String, ()>,
}

impl CacheManager {
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
        }
    }

    /// Invalidate specific path
    pub fn invalidate(&mut self, path: &str) {
        self.cache.remove(path);
    }

    /// Clear all cache
    pub fn clear(&mut self) {
        self.cache.clear();
    }
}

/// History manager for navigation
#[derive(Debug, Clone)]
pub struct HistoryManager {
    /// History stack (back)
    back_stack: VecDeque<String>,

    /// Forward stack
    forward_stack: VecDeque<String>,

    /// Max history size
    max_size: usize,
}

impl HistoryManager {
    pub fn new() -> Self {
        Self {
            back_stack: VecDeque::new(),
            forward_stack: VecDeque::new(),
            max_size: 50,
        }
    }

    /// Push path to history
    pub fn push(&mut self, path: String) {
        // Clear forward stack on new navigation
        self.forward_stack.clear();

        // Add to back stack
        self.back_stack.push_back(path);

        // Limit size
        if self.back_stack.len() > self.max_size {
            self.back_stack.pop_front();
        }
    }

    /// Go back in history
    pub fn back(&mut self) -> Option<String> {
        if let Some(path) = self.back_stack.pop_back() {
            self.forward_stack.push_back(path.clone());
            Some(path)
        } else {
            None
        }
    }

    /// Go forward in history
    pub fn forward(&mut self) -> Option<String> {
        if let Some(path) = self.forward_stack.pop_back() {
            self.back_stack.push_back(path.clone());
            Some(path)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_normalization() {
        let state = PathState::new("/test/path/".to_string());
        assert_eq!(state.current, "/test/path");

        let state2 = PathState::new("test/path".to_string());
        assert_eq!(state2.current, "/test/path");

        let state3 = PathState::new("/".to_string());
        assert_eq!(state3.current, "/");
    }

    #[test]
    fn test_history() {
        let mut history = HistoryManager::new();

        history.push("/path1".to_string());
        history.push("/path2".to_string());

        let back = history.back();
        assert_eq!(back, Some("/path2".to_string()));

        let forward = history.forward();
        assert_eq!(forward, Some("/path2".to_string()));
    }
}
