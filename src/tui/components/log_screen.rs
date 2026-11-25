use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

use crate::tui::action::{Action, ScrollDirection};
use crate::tui::component::Component;
use crate::tui::context::{ComponentContext, RenderContext};
use crate::tui::log_buffer::{clear_logs, get_logs};

/// Log viewer screen component
///
/// Displays application logs with scrolling and auto-scroll functionality.
pub struct LogScreen {
    /// Scroll offset for viewing older logs
    scroll_offset: usize,
    /// Whether to automatically scroll to newest logs
    auto_scroll: bool,
}

impl LogScreen {
    /// Create a new log screen
    pub fn new() -> Self {
        Self {
            scroll_offset: 0,
            auto_scroll: true,
        }
    }

    /// Scroll up by a few lines
    fn scroll_up(&mut self) {
        self.auto_scroll = false;
        if self.scroll_offset > 0 {
            self.scroll_offset = self.scroll_offset.saturating_sub(5);
        }
    }

    /// Scroll down by a few lines
    fn scroll_down(&mut self, total_lines: usize, viewport_height: usize) {
        self.auto_scroll = false;
        let max_offset = total_lines.saturating_sub(viewport_height);
        if self.scroll_offset < max_offset {
            self.scroll_offset = (self.scroll_offset + 5).min(max_offset);
        }
    }

    /// Enable auto-scroll (follow latest logs)
    fn enable_auto_scroll(&mut self) {
        self.auto_scroll = true;
    }

    /// Clear logs and reset state
    fn clear(&mut self) {
        clear_logs();
        self.scroll_offset = 0;
        self.auto_scroll = true;
    }

    fn render_header(&self, frame: &mut Frame, area: Rect) {
        let log_count = get_logs().len();
        let title = Paragraph::new(Line::from(vec![
            Span::styled(
                "📋 Application Logs",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  ({} entries)", log_count),
                Style::default().fg(Color::Gray),
            ),
        ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        );

        frame.render_widget(title, area);
    }

    fn render_content(&self, frame: &mut Frame, area: Rect) {
        let logs = get_logs();

        if logs.is_empty() {
            let empty = Paragraph::new(vec![
                Line::from(""),
                Line::from(Span::styled(
                    "No logs to display",
                    Style::default().fg(Color::DarkGray),
                )),
            ])
            .block(Block::default().borders(Borders::ALL));
            frame.render_widget(empty, area);
            return;
        }

        // Calculate scroll offset for auto-scroll
        let viewport_height = area.height.saturating_sub(2) as usize; // Subtract borders
        let scroll_offset = if self.auto_scroll {
            logs.len().saturating_sub(viewport_height)
        } else {
            self.scroll_offset
        };

        // Create list items
        let items: Vec<ListItem> = logs
            .iter()
            .skip(scroll_offset)
            .take(viewport_height)
            .map(|entry| {
                let level_color = match entry.level.as_str() {
                    "ERROR" => Color::Red,
                    "WARN" => Color::Yellow,
                    "INFO" => Color::Green,
                    "DEBUG" => Color::Cyan,
                    "TRACE" => Color::Gray,
                    _ => Color::White,
                };

                let line = Line::from(vec![
                    Span::styled(
                        format!("[{}] ", entry.timestamp),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(
                        format!("{:<5} ", entry.level),
                        Style::default()
                            .fg(level_color)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("{:<30} ", entry.target),
                        Style::default().fg(Color::Blue),
                    ),
                    Span::styled(&entry.message, Style::default().fg(Color::White)),
                ]);

                ListItem::new(line)
            })
            .collect();

        let scroll_indicator = if self.auto_scroll {
            " [Auto-scroll ON]"
        } else {
            " [Auto-scroll OFF]"
        };

        let list = List::new(items).block(Block::default().borders(Borders::ALL).title(format!(
            "Showing {}-{} of {}{}",
            scroll_offset + 1,
            (scroll_offset + viewport_height).min(logs.len()),
            logs.len(),
            scroll_indicator
        )));

        frame.render_widget(list, area);
    }

    fn render_footer(&self, frame: &mut Frame, area: Rect) {
        let help = Line::from(vec![
            Span::styled(
                "j/k",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(": Scroll | "),
            Span::styled(
                "a",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(": Auto-scroll | "),
            Span::styled(
                "c",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::raw(": Clear | "),
            Span::styled(
                "Esc/~",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(": Close"),
        ]);

        let footer = Paragraph::new(help).block(Block::default().borders(Borders::ALL));

        frame.render_widget(footer, area);
    }
}

impl Component for LogScreen {
    fn handle_key(&mut self, key: KeyEvent, _ctx: &ComponentContext) -> Result<Option<Action>> {
        match (key.code, key.modifiers) {
            // Scrolling
            (KeyCode::Char('j'), KeyModifiers::NONE) | (KeyCode::Down, KeyModifiers::NONE) => {
                Ok(Some(Action::ScrollLogs {
                    direction: ScrollDirection::Down,
                }))
            }
            (KeyCode::Char('k'), KeyModifiers::NONE) | (KeyCode::Up, KeyModifiers::NONE) => {
                Ok(Some(Action::ScrollLogs {
                    direction: ScrollDirection::Up,
                }))
            }

            // Auto-scroll toggle
            (KeyCode::Char('a'), KeyModifiers::NONE) => Ok(Some(Action::ToggleAutoScroll)),

            // Clear logs
            (KeyCode::Char('c'), KeyModifiers::NONE) => Ok(Some(Action::ClearLogs)),

            // Close log screen
            (KeyCode::Esc, KeyModifiers::NONE) | (KeyCode::Char('~'), KeyModifiers::NONE) => {
                Ok(Some(Action::ScreenBack))
            }

            // Quit
            (KeyCode::Char('q'), KeyModifiers::NONE) => Ok(Some(Action::Quit)),

            _ => Ok(None),
        }
    }

    fn update(&mut self, action: Action, _ctx: &mut ComponentContext) -> Result<Option<Action>> {
        match action {
            Action::ScrollLogs { direction } => {
                let logs = get_logs();
                let viewport_height = 30; // Will be calculated from terminal size in practice

                match direction {
                    ScrollDirection::Up => self.scroll_up(),
                    ScrollDirection::Down => self.scroll_down(logs.len(), viewport_height),
                    ScrollDirection::PageUp => {
                        for _ in 0..5 {
                            self.scroll_up();
                        }
                    }
                    ScrollDirection::PageDown => {
                        for _ in 0..5 {
                            self.scroll_down(logs.len(), viewport_height);
                        }
                    }
                    ScrollDirection::Top => {
                        self.scroll_offset = 0;
                        self.auto_scroll = false;
                    }
                    ScrollDirection::Bottom => {
                        self.auto_scroll = true;
                    }
                }
                Ok(None)
            }

            Action::ToggleAutoScroll => {
                self.enable_auto_scroll();
                Ok(None)
            }

            Action::ClearLogs => {
                self.clear();
                Ok(None)
            }

            _ => Ok(None),
        }
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect, _ctx: &RenderContext) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Header
                Constraint::Min(0),    // Content
                Constraint::Length(3), // Footer
            ])
            .split(area);

        self.render_header(frame, chunks[0]);
        self.render_content(frame, chunks[1]);
        self.render_footer(frame, chunks[2]);
    }
}

impl Default for LogScreen {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_screen_creation() {
        let screen = LogScreen::new();
        assert_eq!(screen.scroll_offset, 0);
        assert!(screen.auto_scroll);
    }

    #[test]
    fn test_scroll_up() {
        let mut screen = LogScreen::new();
        screen.scroll_offset = 10;

        screen.scroll_up();
        assert_eq!(screen.scroll_offset, 5);
        assert!(!screen.auto_scroll);
    }

    #[test]
    fn test_scroll_down() {
        let mut screen = LogScreen::new();
        screen.scroll_offset = 0;

        screen.scroll_down(100, 20);
        assert_eq!(screen.scroll_offset, 5);
        assert!(!screen.auto_scroll);
    }

    #[test]
    fn test_auto_scroll() {
        let mut screen = LogScreen::new();
        screen.auto_scroll = false;

        screen.enable_auto_scroll();
        assert!(screen.auto_scroll);
    }
}
