use tokio::sync::mpsc::UnboundedSender;

use super::action::Action;
use crate::app::App;

/// Context available during event handling
///
/// Provides read-only access to application state and services needed
/// for handling keyboard events and making routing decisions.
pub struct ComponentContext<'a> {
    /// Reference to App layer for reading state and accessing services
    pub app: &'a App,

    /// Action sender for triggering async operations
    pub action_tx: &'a ActionSender,
}

impl<'a> ComponentContext<'a> {
    /// Create a new component context
    pub fn new(app: &'a App, action_tx: &'a ActionSender) -> Self {
        Self { app, action_tx }
    }
}

/// Context for rendering components (no mutation)
pub struct RenderContext<'a> {
    /// App state for reading data
    pub app: &'a App,

    /// Whether this specific component is focused. Which pane has focus is
    /// state of the screen that owns the panes; it tells each one here.
    pub is_focused: bool,
}

impl<'a> RenderContext<'a> {
    /// Create a new render context
    pub fn new(app: &'a App, is_focused: bool) -> Self {
        Self { app, is_focused }
    }
}

/// Channel for sending actions (typically from async tasks)
pub type ActionSender = UnboundedSender<Action>;
