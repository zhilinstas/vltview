use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};
use std::collections::HashMap;

use crate::tui::action::Action;
use crate::tui::component::Component;
use crate::tui::context::{ComponentContext, RenderContext};

/// Detail view mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailMode {
    /// Viewing secret data
    Data,
    /// Viewing version history
    Versions,
}

/// Secret version information
#[derive(Debug, Clone)]
pub struct VersionInfo {
    pub version: u64,
    pub created_time: String,
    pub is_deleted: bool,
    pub is_current: bool,
}

/// Detail view component for displaying secret contents
///
/// This component has two modes:
/// - Data mode: Shows key-value pairs with masking toggle
/// - Versions mode: Shows version history with status indicators
pub struct DetailViewComponent {
    /// Current secret data
    secret: HashMap<String, String>,

    /// Selected key index (for data mode)
    selected_index: usize,

    /// Whether to show values or mask them
    show_values: bool,

    /// Secret path for display
    path: String,

    /// Current view mode
    mode: DetailMode,

    /// Secret metadata
    created_time: Option<String>,
    updated_time: Option<String>,
    current_version: Option<u64>,

    /// Version history
    versions: Vec<VersionInfo>,

    /// Selected version index (for version mode)
    selected_version_index: usize,

    /// Version currently being viewed (None = current version)
    viewing_version: Option<u64>,

    /// The actual current/latest version number from Vault
    actual_current_version: Option<u64>,
}

impl DetailViewComponent {
    /// Create a new detail view component
    pub fn new(secret: HashMap<String, String>, path: String) -> Self {
        Self {
            secret,
            selected_index: 0,
            show_values: false,
            path,
            mode: DetailMode::Data,
            created_time: None,
            updated_time: None,
            current_version: None,
            versions: Vec::new(),
            selected_version_index: 0,
            viewing_version: None,
            actual_current_version: None,
        }
    }

    /// Set metadata information
    pub fn set_metadata(
        &mut self,
        created_time: String,
        updated_time: String,
        current_version: u64,
    ) {
        self.created_time = Some(created_time);
        self.updated_time = Some(updated_time);
        self.current_version = Some(current_version);
        self.actual_current_version = Some(current_version);
    }

    /// Set version history
    pub fn set_versions(&mut self, versions: Vec<VersionInfo>) {
        self.versions = versions;
        self.selected_version_index = 0;
    }

    /// Get current mode
    pub fn mode(&self) -> DetailMode {
        self.mode
    }

    /// Get the secret path
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Get a clone of the secret data (for editing)
    pub fn secret_data(&self) -> HashMap<String, String> {
        self.secret.clone()
    }

    /// Update secret data (when loading a different version)
    pub fn update_secret_data(&mut self, data: HashMap<String, String>) {
        self.secret = data;
        self.selected_index = 0;
    }

    /// Get selected key (using sorted order to match rendering)
    pub fn selected_key(&self) -> Option<String> {
        let keys = self.sorted_keys();
        keys.get(self.selected_index).cloned()
    }

    /// Get selected value
    pub fn selected_value(&self) -> Option<String> {
        self.selected_key()
            .and_then(|key| self.secret.get(&key).cloned())
    }

    /// Get selected version
    pub fn selected_version(&self) -> Option<u64> {
        self.versions
            .get(self.selected_version_index)
            .map(|v| v.version)
    }

    /// Get selected version index
    pub fn selected_version_index(&self) -> usize {
        self.selected_version_index
    }

    /// Set selected version index (used to restore selection)
    pub fn set_selected_version_index(&mut self, index: usize) {
        if index < self.versions.len() {
            self.selected_version_index = index;
        }
    }

    /// Load data for a specific version
    pub fn load_version(&mut self, version: u64, data: HashMap<String, String>) {
        self.secret = data;
        self.viewing_version = Some(version);
        self.selected_index = 0;

        // Update metadata to reflect the version being viewed
        self.current_version = Some(version);

        // Find the version in the versions list to get its timestamp
        if let Some(version_info) = self.versions.iter().find(|v| v.version == version) {
            self.created_time = Some(version_info.created_time.clone());
            self.updated_time = Some(version_info.created_time.clone());
        }

        // Switch back to data mode to show the loaded version
        self.mode = DetailMode::Data;
    }

    /// Get the version currently being viewed
    pub fn viewing_version(&self) -> Option<u64> {
        self.viewing_version
    }

    /// Toggle between data and version modes
    fn toggle_mode(&mut self) {
        self.mode = match self.mode {
            DetailMode::Data => DetailMode::Versions,
            DetailMode::Versions => DetailMode::Data,
        };
    }

    /// Toggle show/hide values
    fn toggle_values(&mut self) {
        self.show_values = !self.show_values;
    }

    /// Move selection up
    fn move_up(&mut self) {
        match self.mode {
            DetailMode::Data => {
                if self.selected_index > 0 {
                    self.selected_index -= 1;
                }
            }
            DetailMode::Versions => {
                if self.selected_version_index > 0 {
                    self.selected_version_index -= 1;
                }
            }
        }
    }

    /// Move selection down
    fn move_down(&mut self) {
        match self.mode {
            DetailMode::Data => {
                let max = self.secret.len().saturating_sub(1);
                if self.selected_index < max {
                    self.selected_index += 1;
                }
            }
            DetailMode::Versions => {
                let max = self.versions.len().saturating_sub(1);
                if self.selected_version_index < max {
                    self.selected_version_index += 1;
                }
            }
        }
    }

    /// Get all keys sorted
    fn sorted_keys(&self) -> Vec<String> {
        let mut keys: Vec<_> = self.secret.keys().cloned().collect();
        keys.sort();
        keys
    }

    /// Mask a value for display
    fn mask_value(&self, value: &str) -> String {
        if value.is_empty() {
            return "(empty)".to_string();
        }

        // Show first and last character for short values
        if value.len() <= 4 {
            "•".repeat(value.len())
        } else {
            format!(
                "{}{}{}",
                &value[..1],
                "•".repeat(value.len() - 2),
                &value[value.len() - 1..]
            )
        }
    }

    // ═══════════════════════════════════════════════════════════════
    // RENDERING METHODS (preserved from original DetailView)
    // ═══════════════════════════════════════════════════════════════

    fn render_header(&self, frame: &mut Frame, area: Rect) {
        let title = Paragraph::new(Line::from(vec![
            Span::styled(
                "🔑 Secret Details: ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                &self.path,
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
        ]))
        .block(Block::default().borders(Borders::ALL));

        frame.render_widget(title, area);
    }

    fn render_metadata(&self, frame: &mut Frame, area: Rect) {
        let version_str = if let Some(v) = self.current_version {
            format!("v{}", v)
        } else {
            "unknown".to_string()
        };

        // Check if we're viewing the current version
        let is_current = self.current_version == self.actual_current_version;

        let created = self
            .created_time
            .as_ref()
            .map(|t| format_timestamp(t))
            .unwrap_or_else(|| "Unknown".to_string());

        let updated = self
            .updated_time
            .as_ref()
            .map(|t| format_timestamp(t))
            .unwrap_or_else(|| "Unknown".to_string());

        let mut spans = vec![
            Span::styled("Version: ", Style::default().fg(Color::Gray)),
            Span::styled(
                &version_str,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        ];

        // Add (current) label if viewing the current version
        if is_current {
            spans.push(Span::styled(
                " (current)",
                Style::default().fg(Color::Green),
            ));
        }

        spans.extend(vec![
            Span::styled("  │  Created: ", Style::default().fg(Color::Gray)),
            Span::styled(&created, Style::default().fg(Color::White)),
            Span::styled("  │  Modified: ", Style::default().fg(Color::Gray)),
            Span::styled(&updated, Style::default().fg(Color::White)),
        ]);

        let metadata = Paragraph::new(Line::from(spans)).block(
            Block::default()
                .borders(Borders::ALL)
                .title("Metadata")
                .title_style(Style::default().fg(Color::Cyan)),
        );

        frame.render_widget(metadata, area);
    }

    fn render_data_content(&self, frame: &mut Frame, area: Rect) {
        if self.secret.is_empty() {
            let empty = Paragraph::new("No data in this secret")
                .style(Style::default().fg(Color::Yellow))
                .block(Block::default().borders(Borders::ALL).title("Empty Secret"));
            frame.render_widget(empty, area);
            return;
        }

        let keys = self.sorted_keys();

        let items: Vec<ListItem> = keys
            .iter()
            .enumerate()
            .map(|(i, key)| {
                let value = self.secret.get(key).unwrap();

                // Handle multiline values and masking
                let display_value = if self.show_values {
                    // Show first line with indicator if multiline
                    let lines: Vec<&str> = value.split('\n').collect();
                    if lines.len() > 1 {
                        format!("{} [↵ {} lines]", lines[0], lines.len())
                    } else {
                        value.clone()
                    }
                } else {
                    self.mask_value(value)
                };

                let style = if i == self.selected_index {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };

                let key_style = if i == self.selected_index {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                };

                let line = Line::from(vec![
                    Span::styled(format!("{:<20}", key), key_style),
                    Span::styled(" │ ", Style::default().fg(Color::DarkGray)),
                    Span::styled(display_value, style),
                ]);

                ListItem::new(line)
            })
            .collect();

        let list = List::new(items).block(Block::default().borders(Borders::ALL).title(format!(
            "Data ({} {})",
            self.secret.len(),
            if self.secret.len() == 1 {
                "field"
            } else {
                "fields"
            }
        )));

        frame.render_widget(list, area);
    }

    fn render_versions_content(&self, frame: &mut Frame, area: Rect) {
        if self.versions.is_empty() {
            let empty = Paragraph::new("No version history available")
                .style(Style::default().fg(Color::Yellow))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Version History"),
                );
            frame.render_widget(empty, area);
            return;
        }

        let items: Vec<ListItem> = self
            .versions
            .iter()
            .enumerate()
            .map(|(i, version)| {
                let is_selected = i == self.selected_version_index;

                let style = if is_selected {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };

                // Arrow shows which version is currently being viewed
                let is_viewing = self
                    .viewing_version
                    .map(|v| v == version.version)
                    .unwrap_or(version.is_current);

                let icon = if is_viewing {
                    "→"
                } else if version.is_deleted {
                    "✗"
                } else {
                    " "
                };

                let status_style = if is_viewing {
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD)
                } else if version.is_deleted {
                    Style::default().fg(Color::Red)
                } else {
                    Style::default().fg(Color::DarkGray)
                };

                let time = format_timestamp(&version.created_time);

                let line = Line::from(vec![
                    Span::styled(format!("{} ", icon), status_style),
                    Span::styled(format!("v{:<5}", version.version), style),
                    Span::styled(" │ ", Style::default().fg(Color::DarkGray)),
                    Span::styled(time, style),
                    if version.is_current {
                        Span::styled("  (current)", Style::default().fg(Color::Green))
                    } else if version.is_deleted {
                        Span::styled("  (deleted)", Style::default().fg(Color::Red))
                    } else {
                        Span::raw("")
                    },
                ]);

                ListItem::new(line)
            })
            .collect();

        let list = List::new(items).block(Block::default().borders(Borders::ALL).title(format!(
            "Version History ({} versions)",
            self.versions.len()
        )));

        frame.render_widget(list, area);
    }

    fn render_footer(&self, frame: &mut Frame, area: Rect) {
        let status = if self.show_values {
            Span::styled("Values VISIBLE", Style::default().fg(Color::Red))
        } else {
            Span::styled("Values MASKED", Style::default().fg(Color::Green))
        };

        let help = match self.mode {
            DetailMode::Data => Line::from(vec![
                Span::styled(
                    "v",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(": Toggle | "),
                Span::styled(
                    "c",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(": Copy | "),
                Span::styled(
                    "e",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(": Edit | "),
                Span::styled(
                    "l",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(": History | "),
                status,
            ]),
            DetailMode::Versions => Line::from(vec![
                Span::styled(
                    "Enter",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(": View version | "),
                Span::styled(
                    "n",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(": New from version | "),
                Span::styled(
                    "l",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(": Back to data | "),
                Span::styled(
                    "Esc",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
                Span::raw(": Close"),
            ]),
        };

        let footer = Paragraph::new(help).block(Block::default().borders(Borders::ALL));

        frame.render_widget(footer, area);
    }
}

impl Component for DetailViewComponent {
    fn handle_key(&mut self, key: KeyEvent, _ctx: &ComponentContext) -> Result<Option<Action>> {
        match self.mode {
            DetailMode::Data => {
                // Data mode key handling
                match (key.code, key.modifiers) {
                    // Navigation
                    (KeyCode::Char('j'), KeyModifiers::NONE)
                    | (KeyCode::Down, KeyModifiers::NONE) => {
                        self.move_down();
                        Ok(None)
                    }
                    (KeyCode::Char('k'), KeyModifiers::NONE)
                    | (KeyCode::Up, KeyModifiers::NONE) => {
                        self.move_up();
                        Ok(None)
                    }

                    // Toggle value visibility
                    (KeyCode::Char('v'), KeyModifiers::NONE) => {
                        Ok(Some(Action::ToggleValueVisibility))
                    }

                    // Copy selected value
                    (KeyCode::Char('c'), KeyModifiers::NONE)
                    | (KeyCode::Char('y'), KeyModifiers::NONE) => {
                        Ok(Some(Action::CopySelectedValue))
                    }

                    // Edit secret
                    (KeyCode::Char('e'), KeyModifiers::NONE) => Ok(Some(Action::EditSecret)),

                    // Switch to version history
                    (KeyCode::Char('l'), KeyModifiers::NONE) => Ok(Some(Action::ToggleDetailMode)),

                    // Help
                    (KeyCode::Char('?'), KeyModifiers::NONE) => Ok(Some(Action::ToggleHelp)),

                    // Back (handled by parent)
                    (KeyCode::Esc, KeyModifiers::NONE) => Ok(Some(Action::ScreenBack)),

                    _ => Ok(None),
                }
            }
            DetailMode::Versions => {
                // Versions mode key handling
                match (key.code, key.modifiers) {
                    // Navigation
                    (KeyCode::Char('j'), KeyModifiers::NONE)
                    | (KeyCode::Down, KeyModifiers::NONE) => {
                        self.move_down();
                        Ok(None)
                    }
                    (KeyCode::Char('k'), KeyModifiers::NONE)
                    | (KeyCode::Up, KeyModifiers::NONE) => {
                        self.move_up();
                        Ok(None)
                    }

                    // View selected version
                    (KeyCode::Enter, KeyModifiers::NONE) => {
                        if let Some(version) = self.selected_version() {
                            Ok(Some(Action::ViewSecretVersion {
                                path: self.path.clone(),
                                version,
                            }))
                        } else {
                            Ok(None)
                        }
                    }

                    // Create new version from selected
                    (KeyCode::Char('n'), KeyModifiers::NONE) => {
                        if let Some(version) = self.selected_version() {
                            Ok(Some(Action::CreateFromVersion {
                                path: self.path.clone(),
                                version,
                            }))
                        } else {
                            Ok(None)
                        }
                    }

                    // Back to data mode
                    (KeyCode::Char('l'), KeyModifiers::NONE) => Ok(Some(Action::ToggleDetailMode)),

                    // Help
                    (KeyCode::Char('?'), KeyModifiers::NONE) => Ok(Some(Action::ToggleHelp)),

                    // ESC goes back to data mode, not close detail view
                    (KeyCode::Esc, KeyModifiers::NONE) => Ok(Some(Action::ToggleDetailMode)),

                    _ => Ok(None),
                }
            }
        }
    }

    fn update(&mut self, action: Action, _ctx: &mut ComponentContext) -> Result<Option<Action>> {
        match action {
            // Toggle value visibility
            Action::ToggleValueVisibility => {
                self.toggle_values();
                Ok(None)
            }

            // Toggle between data and version modes
            Action::ToggleDetailMode => {
                self.toggle_mode();
                Ok(None)
            }

            _ => Ok(None),
        }
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect, _ctx: &RenderContext) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Header with path
                Constraint::Length(3), // Metadata section
                Constraint::Min(0),    // Content
                Constraint::Length(3), // Footer
            ])
            .split(area);

        self.render_header(frame, chunks[0]);
        self.render_metadata(frame, chunks[1]);

        match self.mode {
            DetailMode::Data => self.render_data_content(frame, chunks[2]),
            DetailMode::Versions => self.render_versions_content(frame, chunks[2]),
        }

        self.render_footer(frame, chunks[3]);
    }
}

/// Format timestamp to readable format
fn format_timestamp(timestamp: &str) -> String {
    // Parse ISO 8601 timestamp and format it nicely
    // For now, just take the date and time portion
    if let Some(datetime) = timestamp.split('.').next() {
        datetime.replace('T', " ")
    } else {
        timestamp.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detail_view_creation() {
        let mut secret = HashMap::new();
        secret.insert("username".to_string(), "admin".to_string());
        secret.insert("password".to_string(), "secret123".to_string());

        let view = DetailViewComponent::new(secret, "/secret/test".to_string());
        assert_eq!(view.selected_index, 0);
        assert!(!view.show_values);
        assert_eq!(view.mode, DetailMode::Data);
    }

    #[test]
    fn test_navigation() {
        let mut secret = HashMap::new();
        secret.insert("key1".to_string(), "value1".to_string());
        secret.insert("key2".to_string(), "value2".to_string());
        secret.insert("key3".to_string(), "value3".to_string());

        let mut view = DetailViewComponent::new(secret, "/secret/test".to_string());

        assert_eq!(view.selected_index, 0);

        view.move_down();
        assert_eq!(view.selected_index, 1);

        view.move_down();
        assert_eq!(view.selected_index, 2);

        view.move_down(); // Should not go beyond
        assert_eq!(view.selected_index, 2);

        view.move_up();
        assert_eq!(view.selected_index, 1);
    }

    #[test]
    fn test_toggle_values() {
        let mut secret = HashMap::new();
        secret.insert("key".to_string(), "value".to_string());

        let mut view = DetailViewComponent::new(secret, "/secret/test".to_string());

        assert!(!view.show_values);
        view.toggle_values();
        assert!(view.show_values);
        view.toggle_values();
        assert!(!view.show_values);
    }

    #[test]
    fn test_mask_value() {
        let secret = HashMap::new();
        let view = DetailViewComponent::new(secret, "/secret/test".to_string());

        assert_eq!(view.mask_value(""), "(empty)");
        assert_eq!(view.mask_value("a"), "•");
        assert_eq!(view.mask_value("ab"), "••");
        assert_eq!(view.mask_value("abc"), "•••");
        assert_eq!(view.mask_value("abcd"), "••••");
        assert_eq!(view.mask_value("abcde"), "a•••e");
        assert_eq!(view.mask_value("password123"), "p•••••••••3");
    }

    #[test]
    fn test_toggle_mode() {
        let mut secret = HashMap::new();
        secret.insert("key".to_string(), "value".to_string());

        let mut view = DetailViewComponent::new(secret, "/secret/test".to_string());

        assert_eq!(view.mode, DetailMode::Data);
        view.toggle_mode();
        assert_eq!(view.mode, DetailMode::Versions);
        view.toggle_mode();
        assert_eq!(view.mode, DetailMode::Data);
    }
}
