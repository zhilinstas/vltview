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
use crate::tui::keymap::{self, Context};
use crate::tui::log_buffer::{clear_logs, get_logs};

/// Log viewer screen component
///
/// Displays application logs with scrolling and auto-scroll functionality.
pub struct LogScreen {
    /// Scroll offset for viewing older logs
    scroll_offset: usize,
    /// Whether to automatically scroll to newest logs
    auto_scroll: bool,
    /// Rows of log shown the last time the screen was drawn
    viewport_height: usize,
}

impl LogScreen {
    /// Create a new log screen
    pub fn new() -> Self {
        Self {
            scroll_offset: 0,
            auto_scroll: true,
            viewport_height: 20,
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

    fn render_content(&mut self, frame: &mut Frame, area: Rect) {
        let logs = get_logs();
        self.viewport_height = area.height.saturating_sub(2) as usize;

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
        let mut line = keymap::footer_line(&[Context::Logs], area.width.saturating_sub(2));
        line.spans.insert(0, Span::raw(" "));
        frame.render_widget(Paragraph::new(line), area);
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

            (KeyCode::PageDown, _) => Ok(Some(Action::ScrollLogs {
                direction: ScrollDirection::PageDown,
            })),
            (KeyCode::PageUp, _) => Ok(Some(Action::ScrollLogs {
                direction: ScrollDirection::PageUp,
            })),
            (KeyCode::Home, _) | (KeyCode::Char('g'), KeyModifiers::NONE) => {
                Ok(Some(Action::ScrollLogs {
                    direction: ScrollDirection::Top,
                }))
            }
            (KeyCode::End, _) | (KeyCode::Char('G'), _) => Ok(Some(Action::ScrollLogs {
                direction: ScrollDirection::Bottom,
            })),

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
                let viewport_height = self.viewport_height;
                let page_steps = (viewport_height / 5).max(1);

                // While following, the offset is not kept up to date. Start
                // from what is on screen, so the first scroll moves from
                // there instead of jumping to the oldest entry.
                if self.auto_scroll {
                    self.scroll_offset = logs.len().saturating_sub(viewport_height);
                }

                match direction {
                    ScrollDirection::Up => self.scroll_up(),
                    ScrollDirection::Down => self.scroll_down(logs.len(), viewport_height),
                    ScrollDirection::PageUp => {
                        for _ in 0..page_steps {
                            self.scroll_up();
                        }
                    }
                    ScrollDirection::PageDown => {
                        for _ in 0..page_steps {
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
                Constraint::Length(1), // Footer
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

    #[test]
    fn first_scroll_starts_from_what_is_on_screen() {
        // Regression: while following, the offset stayed at 0, so the first
        // `k` jumped from the newest entries to the oldest.
        use crate::tui::log_buffer::add_log;

        for i in 0..40 {
            add_log("INFO", "test", format!("entry {i}"));
        }
        let total = get_logs().len();

        let app = crate::app::App::new(
            crate::config::Config {
                vault_addr: "http://127.0.0.1:1".to_string(),
                vault_token: "test-token".to_string(),
                ..Default::default()
            },
            "/".to_string(),
        )
        .unwrap();
        let focus = crate::tui::context::FocusState::new();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut ctx = ComponentContext::new(&app, &focus, &tx);

        let mut screen = LogScreen::new();
        screen.viewport_height = 10;
        assert!(screen.auto_scroll);

        let up = Action::ScrollLogs {
            direction: ScrollDirection::Up,
        };
        screen.update(up, &mut ctx).unwrap();

        // One step (five lines) above the last page, not at the top
        assert_eq!(screen.scroll_offset, total - 10 - 5);
        assert!(!screen.auto_scroll);
    }
}
