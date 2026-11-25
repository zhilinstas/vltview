use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarOrientation,
        ScrollbarState,
    },
};

use crate::tui::action::{Action, ScrollDirection};
use crate::tui::component::Component;
use crate::tui::components::popup::spinner_frame;
use crate::tui::context::{ComponentContext, RenderContext};
use crate::tui::keymap::{self, Context};

/// Policy screen component for viewing Vault ACL policies
///
/// This component has two modes:
/// - List mode: Shows all policies, navigate with j/k, Enter to view
/// - Content mode: Shows selected policy's HCL content, scrollable
pub struct PolicyScreen {
    /// List of all policy names
    policies: Vec<String>,

    /// Currently selected policy index (in list mode)
    selected_index: usize,

    /// Currently viewed policy name (in content mode)
    current_policy_name: Option<String>,

    /// Currently viewed policy content (in content mode)
    policy_content: Option<String>,

    /// Scroll offset for policy content
    scroll_offset: usize,

    /// Filter text (not yet implemented, placeholder for future)
    filter: String,

    /// Filtered policies (same as policies for now)
    filtered_policies: Vec<String>,

    /// Loading state
    is_loading: bool,

    /// Error message
    error: Option<String>,

    /// Result of the last copy, shown in the footer until the next key
    status: Option<String>,

    /// Scroll state of the policy list
    list_state: ListState,

    /// Rows of policy content shown the last time it was drawn
    viewport_height: usize,
}

impl PolicyScreen {
    /// Create a new policy screen
    pub fn new() -> Self {
        Self {
            policies: Vec::new(),
            selected_index: 0,
            current_policy_name: None,
            policy_content: None,
            scroll_offset: 0,
            filter: String::new(),
            filtered_policies: Vec::new(),
            is_loading: false,
            error: None,
            status: None,
            list_state: ListState::default(),
            viewport_height: 20,
        }
    }

    /// Check if viewing policy content (content mode)
    pub fn is_viewing_content(&self) -> bool {
        self.policy_content.is_some()
    }

    /// Get selected policy name (in list mode)
    pub fn selected_policy(&self) -> Option<&str> {
        self.filtered_policies
            .get(self.selected_index)
            .map(|s| s.as_str())
    }

    /// Move selection up (list mode)
    fn move_up(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    /// Move selection down (list mode)
    fn move_down(&mut self) {
        let max = self.filtered_policies.len().saturating_sub(1);
        if self.selected_index < max {
            self.selected_index += 1;
        }
    }

    /// Scroll content up (content mode)
    fn scroll_up(&mut self) {
        if self.scroll_offset > 0 {
            self.scroll_offset = self.scroll_offset.saturating_sub(5);
        }
    }

    /// Scroll content down (content mode)
    fn scroll_down(&mut self, viewport_height: usize) {
        if let Some(content) = &self.policy_content {
            let total_lines = content.lines().count();
            let max_offset = total_lines.saturating_sub(viewport_height);
            if self.scroll_offset < max_offset {
                self.scroll_offset = (self.scroll_offset + 5).min(max_offset);
            }
        }
    }

    /// Clear content view and return to list mode
    fn clear_content(&mut self) {
        self.current_policy_name = None;
        self.policy_content = None;
        self.scroll_offset = 0;
    }

    // ═══════════════════════════════════════════════════════════════
    // RENDERING METHODS (preserved from original PolicyView)
    // ═══════════════════════════════════════════════════════════════

    fn render_header(&self, frame: &mut Frame, area: Rect) {
        let title = Paragraph::new(vec![
            Line::from(vec![Span::styled(
                "🔐 Vault Policies",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(vec![
                Span::styled(
                    format!("{} policies", self.filtered_policies.len()),
                    Style::default().fg(Color::Gray),
                ),
                if !self.filter.is_empty() {
                    Span::styled(
                        format!(" (filtered: \"{}\")", self.filter),
                        Style::default().fg(Color::Yellow),
                    )
                } else {
                    Span::raw("")
                },
            ]),
        ])
        .block(Block::default().borders(Borders::ALL));

        frame.render_widget(title, area);
    }

    fn render_list(&mut self, frame: &mut Frame, area: Rect) {
        if self.is_loading {
            let loading = Paragraph::new(format!("{} Loading policies…", spinner_frame()))
                .style(Style::default().fg(Color::Yellow))
                .block(Block::default().borders(Borders::ALL).title("Policies"));
            frame.render_widget(loading, area);
            return;
        }

        if let Some(error) = &self.error {
            let error_widget = Paragraph::new(error.as_str())
                .style(Style::default().fg(Color::Red))
                .block(Block::default().borders(Borders::ALL).title("Error"));
            frame.render_widget(error_widget, area);
            return;
        }

        if self.filtered_policies.is_empty() {
            let empty = Paragraph::new("No policies found")
                .style(Style::default().fg(Color::Yellow))
                .block(Block::default().borders(Borders::ALL).title("Policies"));
            frame.render_widget(empty, area);
            return;
        }

        let items: Vec<ListItem> = self
            .filtered_policies
            .iter()
            .enumerate()
            .map(|(i, policy)| {
                let _ = i;
                let style = Style::default().fg(Color::White);

                // Add icon based on common policy types
                let icon = match policy.as_str() {
                    "root" => "👑",
                    "default" => "📋",
                    p if p.contains("admin") => "🔧",
                    p if p.contains("readonly") || p.contains("read") => "👁️",
                    _ => "📄",
                };

                ListItem::new(Line::from(vec![
                    Span::raw(format!(" {} ", icon)),
                    Span::styled(policy, style),
                ]))
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!("Policies ({})", self.filtered_policies.len())),
            )
            .highlight_style(
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            );

        // Stateful, so a long list scrolls with the cursor
        self.list_state.select(Some(self.selected_index));
        frame.render_stateful_widget(list, area, &mut self.list_state);
    }

    /// One footer line: the result of the last copy, or the key hints
    fn render_hints(&self, frame: &mut Frame, area: Rect, context: Context, extra: String) {
        let mut line = match &self.status {
            Some(status) => Line::from(Span::styled(
                status.clone(),
                Style::default().fg(Color::Green),
            )),
            None => keymap::footer_line(&[context], area.width.saturating_sub(2)),
        };
        line.spans.insert(0, Span::raw(" "));
        line.spans
            .push(Span::styled(extra, Style::default().fg(Color::DarkGray)));
        frame.render_widget(Paragraph::new(line), area);
    }

    fn render_footer(&self, frame: &mut Frame, area: Rect) {
        self.render_hints(frame, area, Context::Policies, String::new());
    }

    fn render_content_header(&self, frame: &mut Frame, area: Rect) {
        let policy_name = self.current_policy_name.as_deref().unwrap_or("Unknown");

        // Determine policy type for color
        let (icon, color) = match policy_name {
            "root" => ("👑", Color::Red),
            "default" => ("📋", Color::Blue),
            p if p.contains("admin") => ("🔧", Color::Magenta),
            p if p.contains("readonly") || p.contains("read") => ("👁️", Color::Green),
            _ => ("📄", Color::Cyan),
        };

        let header = Paragraph::new(vec![
            Line::from(vec![Span::styled(
                format!("{} Policy Details", icon),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(vec![
                Span::styled("Name: ", Style::default().fg(Color::Gray)),
                Span::styled(
                    policy_name,
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
            ]),
        ])
        .block(Block::default().borders(Borders::ALL));

        frame.render_widget(header, area);
    }

    fn render_content(&self, frame: &mut Frame, area: Rect) {
        let content = self.policy_content.as_deref().unwrap_or("");

        // Split content into lines and apply scroll offset
        let lines: Vec<&str> = content.lines().collect();
        let visible_lines: Vec<Line> = lines
            .iter()
            .skip(self.scroll_offset)
            .take(area.height.saturating_sub(2) as usize)
            .map(|line| {
                // Simple syntax highlighting for HCL
                if line.trim().starts_with('#') {
                    // Comment
                    Line::from(Span::styled(*line, Style::default().fg(Color::DarkGray)))
                } else if line.contains("path") || line.contains("capabilities") {
                    // Keywords
                    Line::from(Span::styled(*line, Style::default().fg(Color::Yellow)))
                } else if line.contains('=') || line.contains('{') || line.contains('}') {
                    // Structural elements
                    Line::from(Span::styled(*line, Style::default().fg(Color::Cyan)))
                } else if line.trim().starts_with('"') || line.contains('[') || line.contains(']') {
                    // Strings and arrays
                    Line::from(Span::styled(*line, Style::default().fg(Color::Green)))
                } else {
                    Line::from(*line)
                }
            })
            .collect();

        let content_widget = Paragraph::new(visible_lines)
            .style(Style::default().fg(Color::White))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("HCL Policy Content")
                    .border_style(Style::default().fg(Color::Green)),
            );

        frame.render_widget(content_widget, area);

        // Render scrollbar if content is longer than viewport
        let total_lines = lines.len();
        let viewport_height = area.height.saturating_sub(2) as usize;

        if total_lines > viewport_height {
            let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(Some("↑"))
                .end_symbol(Some("↓"));

            let mut scrollbar_state =
                ScrollbarState::new(total_lines.saturating_sub(viewport_height))
                    .position(self.scroll_offset);

            let scrollbar_area = Rect {
                x: area.x + area.width - 1,
                y: area.y + 1,
                width: 1,
                height: area.height.saturating_sub(2),
            };

            frame.render_stateful_widget(scrollbar, scrollbar_area, &mut scrollbar_state);
        }
    }

    fn render_content_footer(&self, frame: &mut Frame, area: Rect) {
        let content_lines = self
            .policy_content
            .as_ref()
            .map(|c| c.lines().count())
            .unwrap_or(0);
        let scroll_info = if content_lines > self.viewport_height {
            format!("  line {}/{}", self.scroll_offset + 1, content_lines)
        } else {
            String::new()
        };

        self.render_hints(frame, area, Context::PolicyContent, scroll_info);
    }

    fn render_list_view(&mut self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Header
                Constraint::Min(0),    // List
                Constraint::Length(1), // Footer
            ])
            .split(area);

        self.render_header(frame, chunks[0]);
        self.render_list(frame, chunks[1]);
        self.render_footer(frame, chunks[2]);
    }

    fn render_content_view(&mut self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Header
                Constraint::Min(0),    // Content
                Constraint::Length(1), // Footer
            ])
            .split(area);

        // Scrolling works in rows of the content area as it is on screen
        self.viewport_height = chunks[1].height.saturating_sub(2) as usize;
        let max_offset = self
            .policy_content
            .as_ref()
            .map(|c| c.lines().count())
            .unwrap_or(0)
            .saturating_sub(self.viewport_height);
        self.scroll_offset = self.scroll_offset.min(max_offset);

        self.render_content_header(frame, chunks[0]);
        self.render_content(frame, chunks[1]);
        self.render_content_footer(frame, chunks[2]);
    }
}

impl Component for PolicyScreen {
    fn handle_key(&mut self, key: KeyEvent, _ctx: &ComponentContext) -> Result<Option<Action>> {
        // A status line stays until the next key
        self.status = None;

        let scroll = |direction| Ok(Some(Action::ScrollPolicy { direction }));

        // Different key handling based on mode
        if self.is_viewing_content() {
            // Content view mode
            match (key.code, key.modifiers) {
                // Scroll
                (KeyCode::Char('j'), KeyModifiers::NONE) | (KeyCode::Down, KeyModifiers::NONE) => {
                    Ok(Some(Action::ScrollPolicy {
                        direction: ScrollDirection::Down,
                    }))
                }
                (KeyCode::Char('k'), KeyModifiers::NONE) | (KeyCode::Up, KeyModifiers::NONE) => {
                    Ok(Some(Action::ScrollPolicy {
                        direction: ScrollDirection::Up,
                    }))
                }

                (KeyCode::PageDown, _) => scroll(ScrollDirection::PageDown),
                (KeyCode::PageUp, _) => scroll(ScrollDirection::PageUp),
                (KeyCode::Home, _) | (KeyCode::Char('g'), KeyModifiers::NONE) => {
                    scroll(ScrollDirection::Top)
                }
                (KeyCode::End, _) | (KeyCode::Char('G'), _) => scroll(ScrollDirection::Bottom),

                // Copy policy content
                (KeyCode::Char('y'), KeyModifiers::NONE) => {
                    Ok(self.policy_content.clone().map(|text| {
                        self.status = Some("Policy copied to clipboard".to_string());
                        Action::CopyToClipboard { text }
                    }))
                }

                // Back to list
                (KeyCode::Esc, KeyModifiers::NONE) => {
                    self.clear_content();
                    Ok(None)
                }

                // Quit
                (KeyCode::Char('q'), KeyModifiers::NONE) => Ok(Some(Action::Quit)),

                // Help
                (KeyCode::Char('?'), KeyModifiers::NONE) => Ok(Some(Action::ToggleHelp)),

                _ => Ok(None),
            }
        } else {
            // List view mode
            match (key.code, key.modifiers) {
                // Navigation
                (KeyCode::Char('j'), KeyModifiers::NONE) | (KeyCode::Down, KeyModifiers::NONE) => {
                    self.move_down();
                    Ok(None)
                }
                (KeyCode::Char('k'), KeyModifiers::NONE) | (KeyCode::Up, KeyModifiers::NONE) => {
                    self.move_up();
                    Ok(None)
                }

                (KeyCode::PageDown, _) => {
                    let last = self.filtered_policies.len().saturating_sub(1);
                    self.selected_index = (self.selected_index + 10).min(last);
                    Ok(None)
                }
                (KeyCode::PageUp, _) => {
                    self.selected_index = self.selected_index.saturating_sub(10);
                    Ok(None)
                }
                (KeyCode::Home, _) | (KeyCode::Char('g'), KeyModifiers::NONE) => {
                    self.selected_index = 0;
                    Ok(None)
                }
                (KeyCode::End, _) | (KeyCode::Char('G'), _) => {
                    self.selected_index = self.filtered_policies.len().saturating_sub(1);
                    Ok(None)
                }

                // View selected policy
                (KeyCode::Enter, KeyModifiers::NONE) => {
                    if let Some(policy_name) = self.selected_policy() {
                        Ok(Some(Action::ViewPolicy {
                            name: policy_name.to_string(),
                        }))
                    } else {
                        Ok(None)
                    }
                }

                // Back to main screen
                (KeyCode::Esc, KeyModifiers::NONE) => Ok(Some(Action::ScreenBack)),

                // Quit
                (KeyCode::Char('q'), KeyModifiers::NONE) => Ok(Some(Action::Quit)),

                // Help
                (KeyCode::Char('?'), KeyModifiers::NONE) => Ok(Some(Action::ToggleHelp)),

                _ => Ok(None),
            }
        }
    }

    fn update(&mut self, action: Action, _ctx: &mut ComponentContext) -> Result<Option<Action>> {
        match action {
            // Policies loaded
            Action::PoliciesLoaded { policies } => {
                self.policies = policies.clone();
                self.filtered_policies = policies;
                self.selected_index = 0;
                self.is_loading = false;
                self.error = None;
                Ok(None)
            }

            // Policy content loaded
            Action::PolicyLoaded { name, content } => {
                self.current_policy_name = Some(name);
                self.policy_content = Some(content);
                self.scroll_offset = 0;
                Ok(None)
            }

            // Scroll policy content
            Action::ScrollPolicy { direction } => {
                let viewport_height = self.viewport_height;
                let page_steps = (viewport_height / 5).max(1);

                match direction {
                    ScrollDirection::Up => self.scroll_up(),
                    ScrollDirection::Down => self.scroll_down(viewport_height),
                    ScrollDirection::PageUp => {
                        for _ in 0..page_steps {
                            self.scroll_up();
                        }
                    }
                    ScrollDirection::PageDown => {
                        for _ in 0..page_steps {
                            self.scroll_down(viewport_height);
                        }
                    }
                    ScrollDirection::Top => {
                        self.scroll_offset = 0;
                    }
                    ScrollDirection::Bottom => {
                        if let Some(content) = &self.policy_content {
                            let total_lines = content.lines().count();
                            self.scroll_offset = total_lines.saturating_sub(viewport_height);
                        }
                    }
                }
                Ok(None)
            }

            // Set loading state
            Action::SetLoading { loading } => {
                self.is_loading = loading;
                Ok(None)
            }

            // Set error
            Action::SetError { message } => {
                self.error = Some(message);
                self.is_loading = false;
                Ok(None)
            }

            _ => Ok(None),
        }
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect, _ctx: &RenderContext) {
        if self.is_viewing_content() {
            self.render_content_view(frame, area);
        } else {
            self.render_list_view(frame, area);
        }
    }
}

impl Default for PolicyScreen {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_policy_screen_creation() {
        let screen = PolicyScreen::new();
        assert!(screen.policies.is_empty());
        assert_eq!(screen.selected_index, 0);
        assert!(!screen.is_viewing_content());
    }

    #[test]
    fn test_navigation() {
        let mut screen = PolicyScreen::new();
        screen.policies = vec!["p1".to_string(), "p2".to_string(), "p3".to_string()];
        screen.filtered_policies = screen.policies.clone();

        assert_eq!(screen.selected_index, 0);

        screen.move_down();
        assert_eq!(screen.selected_index, 1);

        screen.move_down();
        assert_eq!(screen.selected_index, 2);

        screen.move_down(); // Should not go beyond
        assert_eq!(screen.selected_index, 2);

        screen.move_up();
        assert_eq!(screen.selected_index, 1);
    }

    #[test]
    fn test_content_view() {
        let mut screen = PolicyScreen::new();
        assert!(!screen.is_viewing_content());

        screen.current_policy_name = Some("test".to_string());
        screen.policy_content = Some("path \"secret/*\" {}".to_string());
        assert!(screen.is_viewing_content());

        screen.clear_content();
        assert!(!screen.is_viewing_content());
    }
}
