use anyhow::Result;
use crossterm::event::KeyEvent;
use ratatui::{Frame, layout::Rect};

use super::action::Action;
use super::context::{ComponentContext, RenderContext};

/// Component trait for composable UI elements in VltView
///
/// This trait enables the component architecture pattern where UI elements
/// are self-contained, composable, and communicate via Actions.
pub trait Component: Send {
    /// Called once when component is first mounted
    ///
    /// Use this for initialization that requires access to the application context.
    fn init(&mut self, _ctx: &mut ComponentContext) -> Result<()> {
        Ok(())
    }

    /// Handle keyboard events
    ///
    /// Returns:
    /// - `Some(action)`: Event handled, perform this action
    /// - `None`: Event not handled, propagate to parent
    ///
    /// This pattern allows hierarchical event handling where child components
    /// can handle specific keys and let others bubble up to the parent.
    fn handle_key(&mut self, key: KeyEvent, ctx: &ComponentContext) -> Result<Option<Action>>;

    /// Update component state based on an action
    ///
    /// Returns optional follow-up action. This enables action chaining where
    /// handling one action may trigger another.
    fn update(&mut self, action: Action, ctx: &mut ComponentContext) -> Result<Option<Action>>;

    /// Render the component
    ///
    /// Called on every frame. Components should render themselves within the
    /// provided area and may query context for focus state, app data, etc.
    fn draw(&mut self, frame: &mut Frame, area: Rect, ctx: &RenderContext);

    /// Check if this component should capture all input (modal behavior)
    ///
    /// When true, this component receives all keyboard events before they
    /// propagate to other components. Used for overlays, dialogs, etc.
    fn captures_input(&self) -> bool {
        false
    }

    /// Get child components for composition
    ///
    /// Returns immutable slice of child components. Used for event propagation
    /// and rendering child components.
    fn children(&self) -> &[Box<dyn Component>] {
        &[]
    }

    /// Get mutable child components
    ///
    /// Returns mutable slice of child components. Used for updating child state.
    fn children_mut(&mut self) -> &mut [Box<dyn Component>] {
        &mut []
    }
}

/// Unique identifier for components (used for focus tracking)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ComponentId(pub u32);

impl ComponentId {
    /// Create a new component ID
    pub fn new(id: u32) -> Self {
        Self(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_component_id_equality() {
        let id1 = ComponentId::new(1);
        let id2 = ComponentId::new(1);
        let id3 = ComponentId::new(2);

        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
    }
}
