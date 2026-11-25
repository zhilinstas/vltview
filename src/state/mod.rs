use std::sync::RwLock;

/// How many folders the history remembers
const MAX_HISTORY: usize = 50;

/// Application state manager
pub struct AppState {
    /// The folder on screen and the folders visited, behind one lock so
    /// that the path, its items and the place in the history can only
    /// change together
    path: RwLock<PathState>,
}

impl AppState {
    /// Create new application state
    pub fn new(initial_path: String) -> Self {
        Self {
            path: RwLock::new(PathState::new(initial_path)),
        }
    }

    /// Get current path
    pub fn current_path(&self) -> String {
        self.path.read().unwrap().current().to_string()
    }

    /// Show another folder: the path and its items change in one step, so
    /// nothing can read one folder's path with another folder's items.
    ///
    /// The folder becomes the newest entry of the history; whatever was
    /// ahead of the old place is dropped, as in a browser.
    pub fn show_folder(&self, path: String, items: Vec<String>) {
        let mut state = self.path.write().unwrap();
        if state.current() != path {
            let ahead = state.position + 1;
            state.visited.truncate(ahead);
            state.visited.push(path);
            if state.visited.len() > MAX_HISTORY {
                state.visited.remove(0);
            }
            state.position = state.visited.len() - 1;
        }
        state.items = items;
    }

    /// Replace the items of the folder on screen (a reload of `path`).
    /// Returns false, and changes nothing, if another folder is on screen.
    pub fn reload_folder(&self, path: &str, items: Vec<String>) -> bool {
        let mut state = self.path.write().unwrap();
        if state.current() != path {
            return false;
        }
        state.items = items;
        true
    }

    /// Where the folder on screen is in the history; 0 is the oldest entry
    pub fn history_position(&self) -> usize {
        self.path.read().unwrap().position
    }

    /// The folder at `position` in the history, if there is one
    pub fn history_entry(&self, position: usize) -> Option<String> {
        self.path.read().unwrap().visited.get(position).cloned()
    }

    /// Show the folder at `position` in the history. The history itself is
    /// not changed: going back and forward only moves along it.
    ///
    /// Returns false, and changes nothing, if `path` is not what the history
    /// holds there (it was rewritten since the step was asked for).
    pub fn show_history_entry(&self, position: usize, path: &str, items: Vec<String>) -> bool {
        let mut state = self.path.write().unwrap();
        if state.visited.get(position).map(String::as_str) != Some(path) {
            return false;
        }
        state.position = position;
        state.items = items;
        true
    }

    /// Get current list items
    pub fn list_items(&self) -> Vec<String> {
        self.path.read().unwrap().items.clone()
    }
}

/// The folder on screen and how the user got there
#[derive(Debug, Clone)]
pub struct PathState {
    /// Folders visited, oldest first. Never empty.
    visited: Vec<String>,

    /// Which of them is on screen
    position: usize,

    /// Items of the folder on screen
    items: Vec<String>,
}

impl PathState {
    pub fn new(initial_path: String) -> Self {
        Self {
            visited: vec![Self::normalize_path(initial_path)],
            position: 0,
            items: Vec::new(),
        }
    }

    /// Path of the folder on screen
    pub fn current(&self) -> &str {
        &self.visited[self.position]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_normalization() {
        let state = PathState::new("/test/path/".to_string());
        assert_eq!(state.current(), "/test/path");

        let state2 = PathState::new("test/path".to_string());
        assert_eq!(state2.current(), "/test/path");

        let state3 = PathState::new("/".to_string());
        assert_eq!(state3.current(), "/");
    }

    fn show(state: &AppState, path: &str) {
        state.show_folder(path.to_string(), vec![]);
    }

    /// Step along the history by `steps`, as `Root` does once the folder
    /// has been listed
    fn step(state: &AppState, steps: isize) -> bool {
        let Some(position) = state.history_position().checked_add_signed(steps) else {
            return false;
        };
        match state.history_entry(position) {
            Some(path) => state.show_history_entry(position, &path, vec![]),
            None => false,
        }
    }

    #[test]
    fn history_goes_back_and_forward_like_a_browser() {
        // Regression: going back pushed the folder just left onto the same
        // stack, so `back` alternated between two folders and the forward
        // stack was emptied by every step.
        let state = AppState::new("/a".to_string());
        show(&state, "/b");
        show(&state, "/c");

        assert!(step(&state, -1));
        assert!(step(&state, -1));
        assert_eq!(state.current_path(), "/a");
        assert!(!step(&state, -1), "nothing before the first folder");
        assert_eq!(state.current_path(), "/a");

        assert!(step(&state, 1));
        assert!(step(&state, 1));
        assert_eq!(state.current_path(), "/c");
        assert!(!step(&state, 1), "nothing after the last folder");
        assert_eq!(state.current_path(), "/c");
    }

    #[test]
    fn a_new_folder_drops_what_was_ahead() {
        let state = AppState::new("/a".to_string());
        show(&state, "/b");
        show(&state, "/c");
        step(&state, -1);
        step(&state, -1);

        show(&state, "/d");
        assert!(!step(&state, 1));
        assert!(step(&state, -1));
        assert_eq!(state.current_path(), "/a");
        assert_eq!(state.history_entry(1).as_deref(), Some("/d"));
        assert_eq!(state.history_entry(2), None);
    }

    #[test]
    fn showing_the_same_folder_again_adds_no_entry() {
        let state = AppState::new("/a".to_string());
        show(&state, "/a");
        assert_eq!(state.history_position(), 0);
        assert_eq!(state.history_entry(1), None);
    }

    #[test]
    fn a_history_step_that_no_longer_matches_is_refused() {
        let state = AppState::new("/a".to_string());
        show(&state, "/b");
        assert!(!state.show_history_entry(0, "/other", vec![]));
        assert_eq!(state.current_path(), "/b");
    }

    #[test]
    fn the_history_is_bounded() {
        let state = AppState::new("/0".to_string());
        for i in 1..=MAX_HISTORY + 10 {
            show(&state, &format!("/{i}"));
        }
        assert_eq!(state.history_position(), MAX_HISTORY - 1);
        assert_eq!(state.history_entry(0).as_deref(), Some("/11"));
        assert_eq!(state.current_path(), format!("/{}", MAX_HISTORY + 10));
    }

    #[test]
    fn path_and_items_change_together() {
        let state = AppState::new("/a".to_string());
        assert!(state.reload_folder("/a", vec!["one".to_string()]));

        state.show_folder("/b".to_string(), vec!["two".to_string()]);
        assert_eq!(state.current_path(), "/b");
        assert_eq!(state.list_items(), ["two"]);

        // A listing of a folder that is not on screen changes nothing
        assert!(!state.reload_folder("/a", vec!["late".to_string()]));
        assert_eq!(state.current_path(), "/b");
        assert_eq!(state.list_items(), ["two"]);
    }
}
