use ratatui::layout::Rect;
use tokio::sync::mpsc::UnboundedSender;

use super::action::{Action, FocusedPane};
use super::component::ComponentId;
use crate::app::App;

/// Immutable context available during event handling
///
/// Provides read-only access to application state and services needed
/// for handling keyboard events and making routing decisions.
pub struct ComponentContext<'a> {
    /// Reference to App layer for reading state and accessing services
    pub app: &'a App,

    /// Current focus state
    pub focus: &'a FocusState,

    /// Action sender for triggering async operations
    pub action_tx: &'a ActionSender,
}

impl<'a> ComponentContext<'a> {
    /// Create a new component context
    pub fn new(app: &'a App, focus: &'a FocusState, action_tx: &'a ActionSender) -> Self {
        Self {
            app,
            focus,
            action_tx,
        }
    }
}

/// Context for rendering components (no mutation)
///
/// Contains all the information needed to render a component, including
/// focus state and available screen real estate.
pub struct RenderContext<'a> {
    /// App state for reading data
    pub app: &'a App,

    /// Focus state
    pub focus: &'a FocusState,

    /// Whether this specific component is focused
    pub is_focused: bool,

    /// Terminal size (full available area)
    pub terminal_size: Rect,
}

impl<'a> RenderContext<'a> {
    /// Create a new render context
    pub fn new(app: &'a App, focus: &'a FocusState, is_focused: bool, terminal_size: Rect) -> Self {
        Self {
            app,
            focus,
            is_focused,
            terminal_size,
        }
    }

    /// Create a child render context with different focus state
    pub fn with_focus(&self, is_focused: bool) -> Self {
        Self {
            app: self.app,
            focus: self.focus,
            is_focused,
            terminal_size: self.terminal_size,
        }
    }
}

/// Focus state tracking for the application
///
/// Manages which pane is currently focused and maintains a focus path
/// for hierarchical focus management in nested components.
#[derive(Debug, Clone)]
pub struct FocusState {
    /// Currently focused pane (navigation or content)
    pub focused_pane: FocusedPane,

    /// Component focus path (for nested focus)
    /// The last element is the currently focused component
    pub focus_path: Vec<ComponentId>,
}

impl FocusState {
    /// Create a new focus state with default focus on navigation pane
    pub fn new() -> Self {
        Self {
            focused_pane: FocusedPane::Navigation,
            focus_path: Vec::new(),
        }
    }

    /// Switch to the next pane
    pub fn focus_next(&mut self) {
        self.focused_pane = match self.focused_pane {
            FocusedPane::Navigation => FocusedPane::Content,
            FocusedPane::Content => FocusedPane::Navigation,
        };
        self.focus_path.clear();
    }

    /// Focus a specific pane
    pub fn focus_pane(&mut self, pane: FocusedPane) {
        self.focused_pane = pane;
        self.focus_path.clear();
    }

    /// Check if navigation pane is focused
    pub fn is_navigation_focused(&self) -> bool {
        self.focused_pane == FocusedPane::Navigation
    }

    /// Check if content pane is focused
    pub fn is_content_focused(&self) -> bool {
        self.focused_pane == FocusedPane::Content
    }

    /// Push a component onto the focus path
    pub fn push_focus(&mut self, id: ComponentId) {
        self.focus_path.push(id);
    }

    /// Pop a component from the focus path
    pub fn pop_focus(&mut self) -> Option<ComponentId> {
        self.focus_path.pop()
    }

    /// Get the currently focused component ID
    pub fn current_focus(&self) -> Option<ComponentId> {
        self.focus_path.last().copied()
    }
}

impl Default for FocusState {
    fn default() -> Self {
        Self::new()
    }
}

/// Channel for sending actions (typically from async tasks)
pub type ActionSender = UnboundedSender<Action>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_focus_state_default() {
        let focus = FocusState::new();
        assert_eq!(focus.focused_pane, FocusedPane::Navigation);
        assert!(focus.focus_path.is_empty());
    }

    #[test]
    fn test_focus_next() {
        let mut focus = FocusState::new();
        assert!(focus.is_navigation_focused());

        focus.focus_next();
        assert!(focus.is_content_focused());

        focus.focus_next();
        assert!(focus.is_navigation_focused());
    }

    #[test]
    fn test_focus_path() {
        let mut focus = FocusState::new();
        let id1 = ComponentId::new(1);
        let id2 = ComponentId::new(2);

        focus.push_focus(id1);
        focus.push_focus(id2);

        assert_eq!(focus.current_focus(), Some(id2));
        assert_eq!(focus.pop_focus(), Some(id2));
        assert_eq!(focus.current_focus(), Some(id1));
    }
}
