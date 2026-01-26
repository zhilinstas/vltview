use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use crate::tui::action::Action;
use crate::tui::component::Component;
use crate::tui::context::{ComponentContext, RenderContext};

/// Help overlay showing keybindings
///
/// This is a modal overlay that displays all available keyboard shortcuts.
/// It captures all input and dismisses on Esc or ?.
pub struct HelpOverlay {
    /// Whether this overlay is currently visible
    visible: bool,
}

impl HelpOverlay {
    /// Create a new help overlay (hidden by default)
    pub fn new() -> Self {
        Self { visible: false }
    }

    /// Calculate a centered rectangle of given percentage size
    fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
        let popup_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage((100 - percent_y) / 2),
                Constraint::Percentage(percent_y),
                Constraint::Percentage((100 - percent_y) / 2),
            ])
            .split(area);

        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage((100 - percent_x) / 2),
                Constraint::Percentage(percent_x),
                Constraint::Percentage((100 - percent_x) / 2),
            ])
            .split(popup_layout[1])[1]
    }

    fn render_title(&self, frame: &mut Frame, area: Rect) {
        let title = Paragraph::new(Line::from(vec![Span::styled(
            " VltView Help ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )]))
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        );

        frame.render_widget(title, area);
    }

    fn render_content(&self, frame: &mut Frame, area: Rect) {
        let sections = vec![
            (
                "Navigation",
                vec![
                    ("j / Down", "Move down (auto-previews secrets)"),
                    ("k / Up", "Move up (auto-previews secrets)"),
                    ("h / Backspace", "Go to parent directory"),
                    ("Enter", "Navigate into folders"),
                ],
            ),
            (
                "Actions",
                vec![
                    ("r", "Refresh current view"),
                    ("c / y", "Copy selected item to clipboard"),
                    ("e", "Edit selected secret"),
                    ("n", "Create new secret"),
                    ("d", "Delete selected item"),
                    ("/", "Filter items (fuzzy search)"),
                ],
            ),
            (
                "Views",
                vec![
                    ("v", "Toggle secret value visibility"),
                    ("l", "View version history (in secret details)"),
                    ("p", "View policies"),
                    ("~", "View application logs"),
                    ("?", "Show this help"),
                ],
            ),
            (
                "General",
                vec![
                    ("q", "Quit application"),
                    ("Ctrl+c", "Force quit (works anywhere)"),
                    ("Esc", "Clear filter / Close overlay / Cancel"),
                ],
            ),
        ];

        let mut lines: Vec<Line> = Vec::new();

        for (section_name, bindings) in sections {
            // Section header
            lines.push(Line::from(vec![Span::styled(
                format!(" {} ", section_name),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )]));
            lines.push(Line::from(""));

            // Key bindings
            for (key, description) in bindings {
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(
                        format!("{:<14}", key),
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(description, Style::default().fg(Color::White)),
                ]));
            }

            lines.push(Line::from(""));
        }

        let help_text = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::LEFT | Borders::RIGHT)
                .border_style(Style::default().fg(Color::Cyan)),
        );

        frame.render_widget(help_text, area);
    }

    fn render_footer(&self, frame: &mut Frame, area: Rect) {
        let footer = Paragraph::new(Line::from(vec![
            Span::styled(
                "Esc",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" or "),
            Span::styled(
                "?",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" to close"),
        ]))
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        );

        frame.render_widget(footer, area);
    }
}

impl Component for HelpOverlay {
    fn handle_key(&mut self, key: KeyEvent, _ctx: &ComponentContext) -> Result<Option<Action>> {
        // Help overlay dismisses on Esc or ?
        match (key.code, key.modifiers) {
            (KeyCode::Esc, KeyModifiers::NONE) | (KeyCode::Char('?'), KeyModifiers::NONE) => {
                Ok(Some(Action::HideHelp))
            }
            _ => Ok(None), // Ignore all other keys (modal behavior)
        }
    }

    fn update(&mut self, action: Action, _ctx: &mut ComponentContext) -> Result<Option<Action>> {
        match action {
            Action::ShowHelp => {
                self.visible = true;
                Ok(None)
            }
            Action::HideHelp => {
                self.visible = false;
                Ok(None)
            }
            Action::ToggleHelp => {
                self.visible = !self.visible;
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect, _ctx: &RenderContext) {
        if !self.visible {
            return;
        }

        // Calculate centered popup area
        let popup_area = Self::centered_rect(60, 80, area);

        // Clear the background
        frame.render_widget(Clear, popup_area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Title
                Constraint::Min(0),    // Content
                Constraint::Length(3), // Footer
            ])
            .split(popup_area);

        self.render_title(frame, chunks[0]);
        self.render_content(frame, chunks[1]);
        self.render_footer(frame, chunks[2]);
    }

    fn captures_input(&self) -> bool {
        // When visible, capture all input (modal behavior)
        self.visible
    }
}

impl Default for HelpOverlay {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_centered_rect() {
        let area = Rect::new(0, 0, 100, 50);
        let centered = HelpOverlay::centered_rect(50, 50, area);

        // Should be roughly centered
        assert!(centered.x > 0);
        assert!(centered.y > 0);
        assert!(centered.width < area.width);
        assert!(centered.height < area.height);
    }

}
