use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
};
use std::collections::HashSet;

use crate::app::secret::{display_value, masked};
use crate::tui::action::{Action, FocusedPane};
use crate::tui::component::Component;
use crate::tui::components::popup::{
    border_color, display_width, selection_style, truncate, visibility, wrap,
};
use crate::tui::context::{ComponentContext, RenderContext};
use crate::tui::keymap::Context;
use crate::vault::SecretData;

/// Widest the key column gets, in columns
const MAX_KEY_WIDTH: usize = 32;

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

/// One value shown in full, wrapped and scrollable
struct ValueViewer {
    key: String,
    text: String,
    scroll: usize,
    /// Rows shown the last time it was drawn, for paging
    page: usize,
}

/// Detail view component for displaying secret contents
///
/// This component has two modes:
/// - Data mode: Shows key-value pairs with masking toggle
/// - Versions mode: Shows version history with status indicators
///
/// The whole view is one bordered block: the path is its title, the version
/// and timestamps take one line, and the rest is the list. Key hints are in
/// the screen footer, not here.
pub struct DetailViewComponent {
    /// Current secret data, with each value's JSON type preserved
    secret: SecretData,

    /// Selected key index (for data mode)
    selected_index: usize,

    /// Whether every value is shown
    show_values: bool,

    /// Keys whose value is shown although the rest is masked
    revealed: HashSet<String>,

    /// Secret path for display
    path: String,

    /// Current view mode
    mode: DetailMode,

    /// Secret metadata
    created_time: Option<String>,
    updated_time: Option<String>,

    /// Version history
    versions: Vec<VersionInfo>,

    /// Selected version index (for version mode)
    selected_version_index: usize,

    /// Version currently being viewed (None = current version)
    viewing_version: Option<u64>,

    /// The current (latest) version number in Vault
    current_version: Option<u64>,

    /// Version that was asked for, to view or to edit, and has not arrived
    awaited_version: Option<u64>,

    /// Value being shown in full, if any
    viewer: Option<ValueViewer>,

    /// Scroll state of the field list and the version list
    data_state: ListState,
    versions_state: ListState,

    /// Rows the list had the last time it was drawn, for paging
    page: usize,
}

impl DetailViewComponent {
    /// Create a new detail view component
    pub fn new(secret: SecretData, path: String) -> Self {
        Self {
            secret,
            selected_index: 0,
            show_values: false,
            revealed: HashSet::new(),
            path,
            mode: DetailMode::Data,
            created_time: None,
            updated_time: None,
            versions: Vec::new(),
            selected_version_index: 0,
            viewing_version: None,
            current_version: None,
            awaited_version: None,
            viewer: None,
            data_state: ListState::default(),
            versions_state: ListState::default(),
            page: 10,
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
    }

    /// Set version history
    pub fn set_versions(&mut self, versions: Vec<VersionInfo>) {
        self.versions = versions;
        self.selected_version_index = 0;
    }

    /// Carry the view state over from the view this one replaces, when the
    /// same secret was reloaded (after a save, for example): the cursor, what
    /// was revealed, and the place in the version list.
    pub fn adopt_view_state(&mut self, previous: &DetailViewComponent) {
        self.show_values = previous.show_values;
        self.revealed = previous
            .revealed
            .iter()
            .filter(|key| self.secret.contains_key(*key))
            .cloned()
            .collect();
        if let Some(key) = previous.selected_key()
            && let Some(index) = self.sorted_keys().iter().position(|k| *k == key)
        {
            self.selected_index = index;
        }
        if previous.selected_version_index < self.versions.len() {
            self.selected_version_index = previous.selected_version_index;
        }
    }

    /// Get current mode
    pub fn mode(&self) -> DetailMode {
        self.mode
    }

    /// Which set of keys applies right now
    pub fn context(&self) -> Context {
        if self.viewer.is_some() {
            Context::ValueViewer
        } else {
            match self.mode {
                DetailMode::Data => Context::Secret,
                DetailMode::Versions => Context::Versions,
            }
        }
    }

    /// Get the secret path
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Number of versions, when the version history is known
    pub fn version_count(&self) -> Option<usize> {
        (!self.versions.is_empty()).then_some(self.versions.len())
    }

    /// The latest version in Vault, when the metadata is known
    pub fn current_version(&self) -> Option<u64> {
        self.current_version
    }

    /// The version on screen and the current one, when they differ
    pub fn old_version(&self) -> Option<(u64, u64)> {
        match (self.viewing_version, self.current_version) {
            (Some(viewing), Some(current)) if viewing != current => Some((viewing, current)),
            _ => None,
        }
    }

    /// Get a clone of the secret data (for editing)
    pub fn secret_data(&self) -> SecretData {
        self.secret.clone()
    }

    /// Get selected key (using sorted order to match rendering)
    pub fn selected_key(&self) -> Option<String> {
        let keys = self.sorted_keys();
        keys.get(self.selected_index).cloned()
    }

    /// Get selected value as display text
    pub fn selected_value(&self) -> Option<String> {
        self.selected_key()
            .and_then(|key| self.secret.get(&key).map(display_value))
    }

    /// Get selected version
    pub fn selected_version(&self) -> Option<u64> {
        self.versions
            .get(self.selected_version_index)
            .map(|v| v.version)
    }

    /// A version that was asked for has arrived, or could not be loaded.
    ///
    /// Returns whether it is still the one awaited, and stops waiting for
    /// it. A version asked for before the user picked another one is not:
    /// requests are not cancelled, so their results can arrive in any order.
    pub fn version_arrived(&mut self, version: u64) -> bool {
        self.awaited_version
            .take_if(|awaited| *awaited == version)
            .is_some()
    }

    /// Load data for a specific version
    pub fn load_version(&mut self, version: u64, data: SecretData) {
        self.secret = data;
        self.viewing_version = Some(version);
        self.selected_index = 0;
        self.revealed.clear();
        self.viewer = None;

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

    /// Show or hide every value
    fn toggle_values(&mut self) {
        self.show_values = !self.show_values;
        if !self.show_values {
            // Hiding everything hides the peeked values too
            self.revealed.clear();
        }
    }

    /// Show or hide the selected value only
    fn toggle_peek(&mut self) {
        if let Some(key) = self.selected_key()
            && !self.revealed.remove(&key)
        {
            self.revealed.insert(key);
        }
    }

    fn is_revealed(&self, key: &str) -> bool {
        self.show_values || self.revealed.contains(key)
    }

    /// Open the selected value in full
    fn open_viewer(&mut self) {
        let Some(key) = self.selected_key() else {
            return;
        };
        let Some(value) = self.secret.get(&key) else {
            return;
        };
        // Structured values are easier to read indented
        let text = match value {
            serde_json::Value::String(text) => text.clone(),
            other => serde_json::to_string_pretty(other).unwrap_or_else(|_| other.to_string()),
        };
        self.viewer = Some(ValueViewer {
            key,
            text,
            scroll: 0,
            page: 10,
        });
    }

    fn len(&self) -> usize {
        match self.mode {
            DetailMode::Data => self.secret.len(),
            DetailMode::Versions => self.versions.len(),
        }
    }

    fn cursor(&mut self) -> &mut usize {
        match self.mode {
            DetailMode::Data => &mut self.selected_index,
            DetailMode::Versions => &mut self.selected_version_index,
        }
    }

    /// Move the cursor of the current list to `index`, clamped
    fn select(&mut self, index: usize) {
        let last = self.len().saturating_sub(1);
        *self.cursor() = index.min(last);
    }

    /// Move selection up
    fn move_up(&mut self) {
        let index = self.cursor().saturating_sub(1);
        self.select(index);
    }

    /// Move selection down
    fn move_down(&mut self) {
        let index = *self.cursor() + 1;
        self.select(index);
    }

    /// Cursor movement shared by both lists. Returns false for other keys.
    fn handle_movement(&mut self, key: KeyEvent) -> bool {
        let page = self.page.max(1);
        let current = *self.cursor();
        match (key.code, key.modifiers) {
            (KeyCode::Char('j'), KeyModifiers::NONE) | (KeyCode::Down, KeyModifiers::NONE) => {
                self.move_down()
            }
            (KeyCode::Char('k'), KeyModifiers::NONE) | (KeyCode::Up, KeyModifiers::NONE) => {
                self.move_up()
            }
            (KeyCode::PageDown, _) => self.select(current + page),
            (KeyCode::PageUp, _) => self.select(current.saturating_sub(page)),
            (KeyCode::Home, _) | (KeyCode::Char('g'), KeyModifiers::NONE) => self.select(0),
            (KeyCode::End, _) | (KeyCode::Char('G'), _) => self.select(usize::MAX),
            _ => return false,
        }
        true
    }

    /// Get all keys sorted
    fn sorted_keys(&self) -> Vec<String> {
        let mut keys: Vec<_> = self.secret.keys().cloned().collect();
        keys.sort();
        keys
    }

    // ═══════════════════════════════════════════════════════════════
    // RENDERING
    // ═══════════════════════════════════════════════════════════════

    /// Version and timestamps, as much as fits in `width` columns
    fn metadata_line(&self, width: usize) -> Line<'static> {
        let label = Style::default().fg(Color::Gray);
        let value = Style::default().fg(Color::White);
        let version_style = Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD);

        let Some(current) = self.current_version else {
            return Line::from(Span::styled(
                " no version metadata",
                Style::default().fg(Color::DarkGray),
            ));
        };

        let mut parts: Vec<Vec<Span<'static>>> = Vec::new();

        if let Some((viewing, _)) = self.old_version() {
            parts.push(vec![Span::styled(format!(" v{viewing}"), version_style)]);
            if let Some(info) = self.versions.iter().find(|v| v.version == viewing) {
                parts.push(vec![
                    Span::styled("written ", label),
                    Span::styled(format_timestamp(&info.created_time), value),
                ]);
            }
        } else {
            parts.push(vec![
                Span::styled(format!(" v{current}"), version_style),
                Span::styled(" current", Style::default().fg(Color::Green)),
            ]);
            if let Some(updated) = &self.updated_time {
                parts.push(vec![
                    Span::styled("modified ", label),
                    Span::styled(format_timestamp(updated), value),
                ]);
            }
            if let Some(created) = &self.created_time {
                parts.push(vec![
                    Span::styled("created ", label),
                    Span::styled(format_timestamp(created), value),
                ]);
            }
        }

        // Leave out whole parts from the end rather than cut one in half
        let separator = " · ";
        let mut spans: Vec<Span<'static>> = Vec::new();
        let mut used = 0;
        for (i, part) in parts.into_iter().enumerate() {
            let part_width: usize = part.iter().map(|span| span.width()).sum();
            let extra = if i == 0 { 0 } else { separator.chars().count() };
            if i > 0 && used + extra + part_width > width {
                break;
            }
            if i > 0 {
                spans.push(Span::styled(
                    separator,
                    Style::default().fg(Color::DarkGray),
                ));
            }
            used += extra + part_width;
            spans.extend(part);
        }

        Line::from(spans)
    }

    /// Short state shown on the bottom border
    fn state_line(&self) -> Line<'static> {
        match self.mode {
            DetailMode::Data => {
                let fields = if self.secret.len() == 1 {
                    "1 field".to_string()
                } else {
                    format!("{} fields", self.secret.len())
                };
                Line::from(vec![
                    Span::styled(format!(" {fields} · "), Style::default().fg(Color::Gray)),
                    visibility(self.show_values, self.revealed.len()),
                    Span::raw(" "),
                ])
            }
            DetailMode::Versions => {
                let versions = if self.versions.len() == 1 {
                    "1 version".to_string()
                } else {
                    format!("{} versions", self.versions.len())
                };
                Line::from(Span::styled(
                    format!(" History · {versions} "),
                    Style::default().fg(Color::Gray),
                ))
            }
        }
    }

    fn render_data_content(&mut self, frame: &mut Frame, area: Rect, focused: bool) {
        if self.secret.is_empty() {
            let empty =
                Paragraph::new(" No data in this secret").style(Style::default().fg(Color::Yellow));
            frame.render_widget(empty, area);
            return;
        }

        let keys = self.sorted_keys();

        // The key column is as wide as the longest key, within limits, so
        // short keys do not waste space and long ones stay aligned
        let widest = keys.iter().map(|key| display_width(key)).max().unwrap_or(0);
        let key_width = widest
            .min(MAX_KEY_WIDTH)
            .min((area.width as usize / 2).max(8))
            .max(3);

        let items: Vec<ListItem> = keys
            .iter()
            .map(|key| {
                let value = self.secret.get(key).map(display_value).unwrap_or_default();

                let (text, value_style) = if value.is_empty() {
                    ("(empty)".to_string(), Style::default().fg(Color::DarkGray))
                } else if !self.is_revealed(key) {
                    // The same placeholder for every value: neither its
                    // characters nor its length are on screen
                    (
                        masked(&value).to_string(),
                        Style::default().fg(Color::White),
                    )
                } else {
                    // One row per field: the first line, and how much more
                    // there is. Enter shows the whole value.
                    let mut lines = value.split('\n');
                    let first = lines.next().unwrap_or_default();
                    let more = lines.count();
                    let text = if more > 0 {
                        format!("{first} ↵ +{more} lines")
                    } else {
                        first.to_string()
                    };
                    (text, Style::default().fg(Color::White))
                };

                let key_text = truncate(key, key_width);
                let padding = key_width.saturating_sub(display_width(&key_text));

                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!(" {key_text}{}", " ".repeat(padding)),
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" │ ", Style::default().fg(Color::DarkGray)),
                    Span::styled(text, value_style),
                ]))
            })
            .collect();

        self.data_state.select(Some(self.selected_index));
        let list = List::new(items).highlight_style(selection_style(focused));
        frame.render_stateful_widget(list, area, &mut self.data_state);
    }

    fn render_versions_content(&mut self, frame: &mut Frame, area: Rect, focused: bool) {
        if self.versions.is_empty() {
            let empty = Paragraph::new(" No version history available")
                .style(Style::default().fg(Color::Yellow));
            frame.render_widget(empty, area);
            return;
        }

        let items: Vec<ListItem> = self
            .versions
            .iter()
            .map(|version| {
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

                let row = Style::default().fg(Color::White);

                ListItem::new(Line::from(vec![
                    Span::styled(format!(" {icon} "), status_style),
                    Span::styled(format!("v{:<5}", version.version), row),
                    Span::styled(" │ ", Style::default().fg(Color::DarkGray)),
                    Span::styled(format_timestamp(&version.created_time), row),
                    if version.is_current {
                        Span::styled("  (current)", Style::default().fg(Color::Green))
                    } else if version.is_deleted {
                        Span::styled("  (deleted)", Style::default().fg(Color::Red))
                    } else {
                        Span::raw("")
                    },
                ]))
            })
            .collect();

        self.versions_state
            .select(Some(self.selected_version_index));
        let list = List::new(items).highlight_style(selection_style(focused));
        frame.render_stateful_widget(list, area, &mut self.versions_state);
    }

    /// The whole value of one field, over the pane
    fn render_viewer(&mut self, frame: &mut Frame, area: Rect) {
        let Some(viewer) = &mut self.viewer else {
            return;
        };

        // Inside the pane, as tall as the value needs
        let available = Rect {
            x: area.x + 1,
            y: area.y + 1,
            width: area.width.saturating_sub(2),
            height: area.height.saturating_sub(2),
        };
        if available.width < 4 || available.height < 3 {
            return;
        }

        let inner_width = available.width.saturating_sub(4) as usize;
        let lines = wrap(&viewer.text, inner_width);

        let popup = Rect {
            height: (lines.len() as u16 + 2).clamp(3, available.height),
            ..available
        };
        let inner_height = popup.height.saturating_sub(2) as usize;

        viewer.page = inner_height.max(1);
        viewer.scroll = viewer.scroll.min(lines.len().saturating_sub(inner_height));

        let last = (viewer.scroll + inner_height).min(lines.len());
        let position = if lines.len() > inner_height {
            format!(" lines {}-{} of {} ", viewer.scroll + 1, last, lines.len())
        } else {
            String::new()
        };

        let visible: Vec<Line> = lines[viewer.scroll..last]
            .iter()
            .map(|line| Line::from(format!(" {line}")))
            .collect();

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .title(Span::styled(
                format!(" {} ", truncate(&viewer.key, inner_width)),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ))
            .title_bottom(
                Line::from(Span::styled(position, Style::default().fg(Color::DarkGray)))
                    .right_aligned(),
            );

        frame.render_widget(Clear, popup);
        frame.render_widget(Paragraph::new(visible).block(block), popup);
    }

    fn handle_viewer_key(&mut self, key: KeyEvent) -> Option<Action> {
        let viewer = self.viewer.as_mut()?;
        let page = viewer.page.max(1);

        match (key.code, key.modifiers) {
            (KeyCode::Char('j'), KeyModifiers::NONE) | (KeyCode::Down, KeyModifiers::NONE) => {
                viewer.scroll += 1;
            }
            (KeyCode::Char('k'), KeyModifiers::NONE) | (KeyCode::Up, KeyModifiers::NONE) => {
                viewer.scroll = viewer.scroll.saturating_sub(1);
            }
            (KeyCode::PageDown, _) => viewer.scroll += page,
            (KeyCode::PageUp, _) => viewer.scroll = viewer.scroll.saturating_sub(page),
            (KeyCode::Home, _) | (KeyCode::Char('g'), KeyModifiers::NONE) => viewer.scroll = 0,
            // Drawing clamps the offset to the last page
            (KeyCode::End, _) | (KeyCode::Char('G'), _) => viewer.scroll = usize::MAX / 2,
            (KeyCode::Char('c'), KeyModifiers::NONE) | (KeyCode::Char('y'), KeyModifiers::NONE) => {
                return Some(Action::CopyToClipboard {
                    text: viewer.text.clone(),
                });
            }
            (KeyCode::Esc, _) | (KeyCode::Enter, _) | (KeyCode::Char('q'), KeyModifiers::NONE) => {
                self.viewer = None;
            }
            _ => {}
        }
        None
    }
}

impl Component for DetailViewComponent {
    fn handle_key(&mut self, key: KeyEvent, _ctx: &ComponentContext) -> Result<Option<Action>> {
        // The value viewer is modal
        if self.viewer.is_some() {
            return Ok(self.handle_viewer_key(key));
        }

        if self.handle_movement(key) {
            return Ok(None);
        }

        let action = match self.mode {
            DetailMode::Data => match (key.code, key.modifiers) {
                // Show the whole value
                (KeyCode::Enter, KeyModifiers::NONE) => {
                    self.open_viewer();
                    None
                }

                // Reveal the selected value / every value
                (KeyCode::Char(' '), KeyModifiers::NONE) => {
                    self.toggle_peek();
                    None
                }
                (KeyCode::Char('v'), KeyModifiers::NONE) => {
                    self.toggle_values();
                    None
                }

                // Copy selected value
                (KeyCode::Char('c'), KeyModifiers::NONE)
                | (KeyCode::Char('y'), KeyModifiers::NONE) => self
                    .selected_value()
                    .map(|text| Action::CopyToClipboard { text }),

                // Edit secret
                (KeyCode::Char('e'), KeyModifiers::NONE) => Some(Action::EditSecret),

                // Switch to version history
                (KeyCode::Char('l'), KeyModifiers::NONE) => {
                    self.toggle_mode();
                    None
                }

                // Back to the list. The secret stays on screen: the pane
                // shows whatever the cursor is on.
                (KeyCode::Esc, KeyModifiers::NONE) => Some(Action::FocusPane {
                    pane: FocusedPane::Navigation,
                }),

                _ => None,
            },
            DetailMode::Versions => match (key.code, key.modifiers) {
                // View selected version
                (KeyCode::Enter, KeyModifiers::NONE) => {
                    let version = self.selected_version();
                    self.awaited_version = version;
                    version.map(|version| Action::ViewSecretVersion {
                        path: self.path.clone(),
                        version,
                    })
                }

                // Create new version from selected
                (KeyCode::Char('n'), KeyModifiers::NONE) => {
                    let version = self.selected_version();
                    self.awaited_version = version;
                    version.map(|version| Action::CreateFromVersion {
                        path: self.path.clone(),
                        version,
                    })
                }

                // Back to the fields
                (KeyCode::Char('l'), KeyModifiers::NONE) | (KeyCode::Esc, KeyModifiers::NONE) => {
                    self.toggle_mode();
                    None
                }

                _ => None,
            },
        };

        Ok(action)
    }

    fn update(&mut self, _action: Action, _ctx: &mut ComponentContext) -> Result<Option<Action>> {
        Ok(None)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect, ctx: &RenderContext) {
        let focused = ctx.is_focused;

        let title_width = area.width.saturating_sub(4) as usize;
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color(focused)))
            .title(Span::styled(
                format!(" {} ", truncate(&self.path, title_width)),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ))
            .title_bottom(self.state_line());
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let old_version = self.old_version();
        let chunks = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(u16::from(old_version.is_some())),
            Constraint::Min(0),
        ])
        .split(inner);

        frame.render_widget(
            Paragraph::new(self.metadata_line(inner.width as usize)),
            chunks[0],
        );

        // An old version must not be mistaken for the secret as it is now
        if let Some((viewing, current)) = old_version {
            let banner = Paragraph::new(format!(
                " Viewing v{viewing}, not the current version (v{current})"
            ))
            .style(
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            );
            frame.render_widget(banner, chunks[1]);
        }

        self.page = chunks[2].height as usize;
        match self.mode {
            DetailMode::Data => self.render_data_content(frame, chunks[2], focused),
            DetailMode::Versions => self.render_versions_content(frame, chunks[2], focused),
        }

        self.render_viewer(frame, area);
    }

    fn captures_input(&self) -> bool {
        // The value viewer takes every key while it is open
        self.viewer.is_some()
    }
}

/// Format an RFC 3339 timestamp, as Vault writes it
/// (`2026-01-20T14:47:07.123456789Z`), for display: seconds are enough, and
/// the zone is said out loud.
fn format_timestamp(timestamp: &str) -> String {
    if timestamp.is_empty() {
        return "unknown".to_string();
    }

    let seconds = timestamp
        .split(['.', 'Z'])
        .next()
        .unwrap_or(timestamp)
        .replace('T', " ");

    if timestamp.ends_with('Z') {
        format!("{seconds} UTC")
    } else {
        seconds
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::context::FocusState;
    use ratatui::{Terminal, backend::TestBackend};
    use serde_json::json;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn test_app() -> crate::app::App {
        let config = crate::config::Config {
            vault_addr: "http://127.0.0.1:1".to_string(),
            vault_token: "test-token".to_string().into(),
            ..Default::default()
        };
        crate::app::App::new(config, "/".to_string()).unwrap()
    }

    fn with_ctx(body: impl FnOnce(&ComponentContext)) {
        let app = test_app();
        let focus = FocusState::new();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        body(&ComponentContext::new(&app, &focus, &tx));
    }

    fn view_of(value: serde_json::Value) -> DetailViewComponent {
        DetailViewComponent::new(
            serde_json::from_value(value).unwrap(),
            "/secret/test".to_string(),
        )
    }

    fn with_history(mut view: DetailViewComponent, current: u64) -> DetailViewComponent {
        view.set_metadata(
            "2026-01-20T14:47:07.123456789Z".to_string(),
            "2026-02-01T09:30:00.5Z".to_string(),
            current,
        );
        view.set_versions(
            (1..=current)
                .rev()
                .map(|version| VersionInfo {
                    version,
                    created_time: format!("2026-01-{version:02}T10:00:00.5Z"),
                    is_deleted: false,
                    is_current: version == current,
                })
                .collect(),
        );
        view
    }

    /// Text on screen after drawing the view at the given size
    fn rendered(view: &mut DetailViewComponent, width: u16, height: u16) -> String {
        let app = test_app();
        let focus = FocusState::new();
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                let ctx = RenderContext::new(&app, &focus, true, area);
                view.draw(frame, area, &ctx);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
                    + "\n"
            })
            .collect()
    }

    #[test]
    fn test_detail_view_creation() {
        let mut secret = SecretData::new();
        secret.insert("username".to_string(), "admin".into());
        secret.insert("password".to_string(), "secret123".into());

        let view = DetailViewComponent::new(secret, "/secret/test".to_string());
        assert_eq!(view.selected_index, 0);
        assert!(!view.show_values);
        assert_eq!(view.mode, DetailMode::Data);
    }

    #[test]
    fn test_navigation() {
        let mut secret = SecretData::new();
        secret.insert("key1".to_string(), "value1".into());
        secret.insert("key2".to_string(), "value2".into());
        secret.insert("key3".to_string(), "value3".into());

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
        let mut secret = SecretData::new();
        secret.insert("key".to_string(), "value".into());

        let mut view = DetailViewComponent::new(secret, "/secret/test".to_string());

        assert!(!view.show_values);
        view.toggle_values();
        assert!(view.show_values);
        view.toggle_values();
        assert!(!view.show_values);
    }

    /// What is to the right of the key column, for the field named `key`
    fn value_cell<'a>(screen: &'a str, key: &str) -> &'a str {
        let row = screen
            .lines()
            .find(|line| line.contains(&format!(" {key} ")))
            .unwrap_or_else(|| panic!("no row for {key}\n{screen}"));
        let (_, value) = row.split_once(" │ ").expect("a key and a value");
        value.trim_end_matches('│').trim_end()
    }

    #[test]
    fn masked_values_all_look_the_same() {
        // Regression: a masked value showed its first and last character
        // and one dot per character, so its length could be read off the
        // screen. A non-ASCII value crashed the mask (it sliced by byte).
        let mut view = view_of(json!({
            "a_short": "x",
            "b_long": "correct-horse-battery-staple-0123456789",
            "c_lines": "first line\nsecond line",
            "d_unicode": "пароль✓🔑",
            "e_number": 5432,
            "f_empty": "",
        }));

        let screen = rendered(&mut view, 70, 12);
        for key in ["a_short", "b_long", "c_lines", "d_unicode", "e_number"] {
            assert_eq!(value_cell(&screen, key), "••••••••", "{key}\n{screen}");
        }
        assert_eq!(value_cell(&screen, "f_empty"), "(empty)", "{screen}");
        for leaked in [
            "correct",
            "789",
            "first",
            "+1 lines",
            "пароль",
            "🔑",
            "5432",
        ] {
            assert!(
                !screen.contains(leaked),
                "{leaked:?} is on screen\n{screen}"
            );
        }
    }

    #[test]
    fn test_non_string_values_are_kept_and_displayed() {
        let secret: SecretData = serde_json::from_value(serde_json::json!({
            "host": "db", "port": 5432, "tls": true
        }))
        .unwrap();

        let mut view = DetailViewComponent::new(secret.clone(), "/secret/test".to_string());

        // Nothing is dropped on the way to the editor
        assert_eq!(view.secret_data(), secret);
        assert_eq!(view.sorted_keys(), vec!["host", "port", "tls"]);

        view.move_down();
        assert_eq!(view.selected_value().as_deref(), Some("5432"));
        view.move_down();
        assert_eq!(view.selected_value().as_deref(), Some("true"));
    }

    #[test]
    fn test_toggle_mode() {
        let mut secret = SecretData::new();
        secret.insert("key".to_string(), "value".into());

        let mut view = DetailViewComponent::new(secret, "/secret/test".to_string());

        assert_eq!(view.mode, DetailMode::Data);
        view.toggle_mode();
        assert_eq!(view.mode, DetailMode::Versions);
        view.toggle_mode();
        assert_eq!(view.mode, DetailMode::Data);
    }

    #[test]
    fn timestamps_name_their_zone() {
        // Regression: times were shown without saying they are UTC
        assert_eq!(
            format_timestamp("2026-01-20T14:47:07.123456789Z"),
            "2026-01-20 14:47:07 UTC"
        );
        assert_eq!(
            format_timestamp("2026-01-20T14:47:07Z"),
            "2026-01-20 14:47:07 UTC"
        );
        assert_eq!(format_timestamp(""), "unknown");
    }

    #[test]
    fn metadata_drops_whole_parts_when_the_pane_is_narrow() {
        // Regression: the metadata line was cut off mid-word at 80 columns
        let mut view = with_history(view_of(json!({"k": "v"})), 7);

        let wide = rendered(&mut view, 100, 8);
        assert!(wide.contains("v7 current"), "{wide}");
        assert!(wide.contains("modified 2026-02-01 09:30:00 UTC"), "{wide}");
        assert!(wide.contains("created 2026-01-20 14:47:07 UTC"), "{wide}");

        // 52 columns is the content pane of an 80-column terminal
        let narrow = rendered(&mut view, 52, 8);
        assert!(narrow.contains("v7 current"), "{narrow}");
        assert!(
            narrow.contains("modified 2026-02-01 09:30:00 UTC"),
            "{narrow}"
        );
        assert!(!narrow.contains("created"), "{narrow}");
    }

    #[test]
    fn peek_reveals_one_value_and_v_reveals_all() {
        // Regression: reveal was all or nothing
        with_ctx(|ctx| {
            let mut view = view_of(json!({"alpha": "first-secret", "beta": "second-secret"}));

            let screen = rendered(&mut view, 60, 8);
            assert!(!screen.contains("first-secret"), "{screen}");
            assert!(screen.contains("values masked"), "{screen}");

            view.handle_key(key(KeyCode::Char(' ')), ctx).unwrap();
            let screen = rendered(&mut view, 60, 8);
            assert!(screen.contains("first-secret"), "{screen}");
            assert!(!screen.contains("second-secret"), "{screen}");
            assert!(screen.contains("1 revealed"), "{screen}");

            view.handle_key(key(KeyCode::Char('v')), ctx).unwrap();
            let screen = rendered(&mut view, 60, 8);
            assert!(screen.contains("second-secret"), "{screen}");
            assert!(screen.contains("values VISIBLE"), "{screen}");

            // Hiding everything hides the peeked value too
            view.handle_key(key(KeyCode::Char('v')), ctx).unwrap();
            let screen = rendered(&mut view, 60, 8);
            assert!(!screen.contains("first-secret"), "{screen}");
            assert!(screen.contains("values masked"), "{screen}");
        });
    }

    #[test]
    fn enter_shows_the_whole_value() {
        // Regression: a multi-line value showed its first line only, and a
        // long one was cut at the pane edge, with no way to read the rest.
        with_ctx(|ctx| {
            let pem = "-----BEGIN-----\nline-two\nline-three\n-----END-----";
            let mut view = view_of(json!({"cert": pem, "zeta": "z"}));

            view.handle_key(key(KeyCode::Enter), ctx).unwrap();
            assert_eq!(view.context(), Context::ValueViewer);
            assert!(view.captures_input());

            let screen = rendered(&mut view, 60, 12);
            for line in pem.lines() {
                assert!(screen.contains(line), "{line} missing:\n{screen}");
            }

            // Copy takes the whole value
            let action = view.handle_key(key(KeyCode::Char('c')), ctx).unwrap();
            assert!(
                matches!(&action, Some(Action::CopyToClipboard { text }) if text == pem),
                "{action:?}"
            );

            view.handle_key(key(KeyCode::Esc), ctx).unwrap();
            assert_eq!(view.context(), Context::Secret);
            // The list itself stays masked
            assert!(!rendered(&mut view, 60, 12).contains("line-two"));
        });
    }

    #[test]
    fn viewer_wraps_long_values_and_scrolls() {
        with_ctx(|ctx| {
            let long: String = (0..40).map(|i| format!("chunk{i:02}-")).collect();
            let mut view = view_of(json!({"token": long}));

            view.handle_key(key(KeyCode::Enter), ctx).unwrap();
            let screen = rendered(&mut view, 40, 8);
            assert!(screen.contains("chunk00-"), "{screen}");
            assert!(!screen.contains("chunk39-"), "{screen}");
            assert!(screen.contains("lines 1-4 of"), "{screen}");

            view.handle_key(key(KeyCode::End), ctx).unwrap();
            let screen = rendered(&mut view, 40, 8);
            assert!(screen.contains("chunk39-"), "{screen}");
        });
    }

    #[test]
    fn viewer_indents_structured_values() {
        with_ctx(|ctx| {
            let mut view = view_of(json!({"config": {"host": "db", "ports": [1, 2]}}));
            view.handle_key(key(KeyCode::Enter), ctx).unwrap();

            let screen = rendered(&mut view, 60, 14);
            assert!(screen.contains("\"host\": \"db\""), "{screen}");
            assert!(screen.contains("\"ports\": ["), "{screen}");
        });
    }

    #[test]
    fn key_column_fits_the_longest_key() {
        // Regression: the column was fixed at 20, so longer keys pushed
        // their values out of line.
        let mut view = view_of(json!({
            "a": "1",
            "a_key_longer_than_twenty_chars": "2"
        }));
        view.toggle_values();

        let screen = rendered(&mut view, 70, 8);
        let columns: Vec<usize> = screen
            .lines()
            .filter(|line| line.contains(" │ ") && !line.starts_with('┌'))
            .filter_map(|line| line.find(" │ "))
            .collect();
        assert_eq!(columns.len(), 2, "{screen}");
        assert_eq!(columns[0], columns[1], "{screen}");
        assert!(
            screen.contains("a_key_longer_than_twenty_chars"),
            "{screen}"
        );
    }

    #[test]
    fn long_field_lists_scroll_with_the_cursor() {
        with_ctx(|ctx| {
            let fields: serde_json::Map<String, serde_json::Value> = (0..30)
                .map(|i| (format!("field_{i:02}"), json!("v")))
                .collect();
            let mut view = view_of(serde_json::Value::Object(fields));

            let screen = rendered(&mut view, 60, 12);
            assert!(screen.contains("field_00"), "{screen}");
            assert!(!screen.contains("field_29"), "{screen}");

            view.handle_key(KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT), ctx)
                .unwrap();
            let screen = rendered(&mut view, 60, 12);
            assert!(screen.contains("field_29"), "{screen}");
            assert_eq!(view.selected_key().as_deref(), Some("field_29"));

            view.handle_key(key(KeyCode::PageUp), ctx).unwrap();
            assert!(view.selected_index < 29);
            view.handle_key(key(KeyCode::Char('g')), ctx).unwrap();
            assert_eq!(view.selected_index, 0);
        });
    }

    #[test]
    fn an_old_version_is_announced() {
        let mut view = with_history(view_of(json!({"k": "now"})), 7);
        assert!(view.old_version().is_none());
        assert!(!rendered(&mut view, 70, 10).contains("not the current version"));

        view.load_version(3, serde_json::from_value(json!({"k": "then"})).unwrap());
        assert_eq!(view.old_version(), Some((3, 7)));

        let screen = rendered(&mut view, 70, 10);
        assert!(
            screen.contains("Viewing v3, not the current version (v7)"),
            "{screen}"
        );
        assert!(
            screen.contains("written 2026-01-03 10:00:00 UTC"),
            "{screen}"
        );

        // Going back to the current version removes the warning
        view.load_version(7, serde_json::from_value(json!({"k": "now"})).unwrap());
        assert!(view.old_version().is_none());
    }

    #[test]
    fn version_list_is_reached_with_l_and_left_with_escape() {
        with_ctx(|ctx| {
            let mut view = with_history(view_of(json!({"k": "v"})), 3);

            view.handle_key(key(KeyCode::Char('l')), ctx).unwrap();
            assert_eq!(view.context(), Context::Versions);
            let screen = rendered(&mut view, 70, 10);
            assert!(screen.contains("History · 3 versions"), "{screen}");

            view.handle_key(key(KeyCode::Down), ctx).unwrap();
            let action = view.handle_key(key(KeyCode::Enter), ctx).unwrap();
            assert!(
                matches!(action, Some(Action::ViewSecretVersion { version: 2, .. })),
                "{action:?}"
            );

            view.handle_key(key(KeyCode::Esc), ctx).unwrap();
            assert_eq!(view.context(), Context::Secret);

            // From the fields, Esc goes back to the list
            let action = view.handle_key(key(KeyCode::Esc), ctx).unwrap();
            assert!(
                matches!(
                    action,
                    Some(Action::FocusPane {
                        pane: FocusedPane::Navigation
                    })
                ),
                "{action:?}"
            );
        });
    }

    #[test]
    fn a_reload_keeps_the_cursor_and_what_was_revealed() {
        let mut before = view_of(json!({"alpha": "1", "beta": "2", "gamma": "3"}));
        before.select(1);
        before.toggle_peek();
        assert_eq!(before.selected_key().as_deref(), Some("beta"));

        // The same secret after a save that added a field before the cursor
        let mut after = view_of(json!({"aaa": "0", "alpha": "1", "beta": "2", "gamma": "3"}));
        after.adopt_view_state(&before);

        assert_eq!(after.selected_key().as_deref(), Some("beta"));
        assert!(after.is_revealed("beta"));
        assert!(!after.is_revealed("alpha"));
    }

    #[test]
    fn selection_is_dim_when_the_pane_is_not_focused() {
        // Regression: the selected field stayed bright while the list had
        // focus, so both panes looked active.
        let app = test_app();
        let focus = FocusState::new();
        let mut view = view_of(json!({"alpha": "1"}));

        let mut selection_bg = |focused: bool| {
            let mut terminal = Terminal::new(TestBackend::new(40, 6)).unwrap();
            terminal
                .draw(|frame| {
                    let area = frame.area();
                    let ctx = RenderContext::new(&app, &focus, focused, area);
                    view.draw(frame, area, &ctx);
                })
                .unwrap();
            // Row 2 is the first field: border, metadata, then the list
            terminal.backend().buffer()[(3, 2)].bg
        };

        assert_eq!(selection_bg(true), Color::Cyan);
        assert_eq!(selection_bg(false), Color::DarkGray);
    }
}
