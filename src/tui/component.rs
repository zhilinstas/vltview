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

    /// Handle text pasted into the terminal (bracketed paste)
    ///
    /// Pasted text is data, never commands: a component that has a text
    /// input in focus inserts it there, and everything else ignores it.
    fn handle_paste(&mut self, _text: &str, _ctx: &ComponentContext) -> Result<Option<Action>> {
        Ok(None)
    }

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
}
