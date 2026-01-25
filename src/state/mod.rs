use std::collections::{HashMap, VecDeque};
use std::sync::RwLock;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Application state manager
pub struct AppState {
    /// Current path in Vault
    path: RwLock<PathState>,

    /// Current selection in UI
    #[allow(dead_code)]
    selection: RwLock<SelectionState>,

    /// Current view mode
    #[allow(dead_code)]
    view: RwLock<ViewState>,

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
            selection: RwLock::new(SelectionState::new()),
            view: RwLock::new(ViewState::new()),
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

    /// Get current selection index
    pub fn selected_index(&self) -> Option<usize> {
        self.selection.read().unwrap().selected
    }

    /// Set selection index
    pub fn set_selection(&self, index: Option<usize>) {
        self.selection.write().unwrap().selected = index;
    }

    /// Move selection up
    pub fn move_selection_up(&self, max: usize) {
        self.selection.write().unwrap().move_up(max);
    }

    /// Move selection down
    pub fn move_selection_down(&self, max: usize) {
        self.selection.write().unwrap().move_down(max);
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

/// Selection state for UI lists
#[derive(Debug, Clone)]
pub struct SelectionState {
    /// Currently selected index
    pub selected: Option<usize>,
}

impl SelectionState {
    pub fn new() -> Self {
        Self { selected: Some(0) }
    }

    /// Move selection up
    pub fn move_up(&mut self, max: usize) {
        if max == 0 {
            self.selected = None;
            return;
        }

        self.selected = Some(match self.selected {
            Some(i) if i > 0 => i - 1,
            Some(_) => max - 1,
            None => 0,
        });
    }

    /// Move selection down
    pub fn move_down(&mut self, max: usize) {
        if max == 0 {
            self.selected = None;
            return;
        }

        self.selected = Some(match self.selected {
            Some(i) if i < max - 1 => i + 1,
            Some(_) => 0,
            None => 0,
        });
    }
}

/// View state for different screens
#[derive(Debug, Clone)]
pub struct ViewState {
    /// Current view mode
    #[allow(dead_code)]
    pub mode: ViewMode,

    /// Whether secrets are shown (vs hidden)
    #[allow(dead_code)]
    pub secrets_visible: bool,
}

impl ViewState {
    pub fn new() -> Self {
        Self {
            mode: ViewMode::List,
            secrets_visible: false,
        }
    }
}

/// View modes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ViewMode {
    /// List view
    List,
    /// Detail view
    Detail,
    /// Edit view
    Edit,
    /// Policy view
    Policy,
    /// Help view
    Help,
}

/// Cache manager for secrets
#[derive(Debug)]
pub struct CacheManager {
    /// Cache storage
    cache: HashMap<String, CachedItem>,

    /// Cache TTL
    #[allow(dead_code)]
    ttl: Duration,

    /// Max cache size
    #[allow(dead_code)]
    max_size: usize,
}

/// Cached item with timestamp
#[derive(Debug, Clone)]
struct CachedItem {
    #[allow(dead_code)]
    data: serde_json::Value,
    #[allow(dead_code)]
    cached_at: Instant,
}

impl CacheManager {
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
            ttl: Duration::from_secs(300), // 5 minutes default
            max_size: 100,
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
    fn test_breadcrumbs() {
        let state = PathState::new("/test/path/deep".to_string());
        let crumbs = state.breadcrumbs();
        assert_eq!(crumbs.len(), 4);
        assert_eq!(crumbs[0], "/");
        assert_eq!(crumbs[1], "/test");
        assert_eq!(crumbs[2], "/test/path");
        assert_eq!(crumbs[3], "/test/path/deep");
    }

    #[test]
    fn test_selection_movement() {
        let mut sel = SelectionState::new();
        sel.move_down(5);
        assert_eq!(sel.selected, Some(1));

        sel.move_up(5);
        assert_eq!(sel.selected, Some(0));

        sel.move_up(5);
        assert_eq!(sel.selected, Some(4));
    }

    #[test]
    fn test_cache() {
        let mut cache = CacheManager::new();
        let data = serde_json::json!({"key": "value"});

        cache.insert("/test".to_string(), data.clone());
        let cached = cache.get("/test");
        assert!(cached.is_some());
        assert_eq!(cached.unwrap(), data);

        cache.invalidate("/test");
        assert!(cache.get("/test").is_none());
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
