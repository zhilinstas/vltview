use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
};
use ratatui_textarea::{Input, TextArea};

use crate::app::secret::{display_value, edited_value, json_type_name};
use crate::tui::action::Action;
use crate::tui::component::Component;
use crate::tui::context::{ComponentContext, RenderContext};
use crate::vault::SecretData;

/// Edit mode for the form
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditMode {
    /// Navigating fields
    Navigate,
    /// Editing a field value
    EditValue,
    /// Editing a field key (for new fields)
    EditKey,
}

/// Edit view component for modifying secrets
///
/// This component has three modes:
/// - Navigate: Browse fields, add/delete, save
/// - EditKey: Edit the key name using TextArea
/// - EditValue: Edit the value using TextArea (supports multiline)
pub struct EditViewComponent {
    /// Secret path
    path: String,

    /// Fields (key-value pairs) - stored as Vec for ordering.
    /// Values are held as editable text; see `typed_originals`.
    fields: Vec<(String, String)>,

    /// Original values of the fields that were not strings when loaded
    /// (numbers, booleans, null, arrays, objects), by key. Used on save so
    /// those fields keep their JSON type instead of being stringified.
    typed_originals: SecretData,

    /// Selected field index
    selected_index: usize,

    /// Current edit mode
    mode: EditMode,

    /// TextArea for editing keys
    key_textarea: TextArea<'static>,

    /// TextArea for editing values
    value_textarea: TextArea<'static>,

    /// Whether the secret is new (create mode)
    is_new: bool,

    /// Whether there are unsaved changes
    has_changes: bool,

    /// Error message
    error: Option<String>,

    /// Delete confirmation - stores the field name to be deleted
    delete_confirmation: Option<String>,
}

impl EditViewComponent {
    /// Create a new edit view component for an existing secret
    pub fn new(path: String, secret: SecretData) -> Self {
        let mut fields: Vec<(String, String)> = secret
            .iter()
            .map(|(key, value)| (key.clone(), display_value(value)))
            .collect();
        fields.sort_by(|a, b| a.0.cmp(&b.0));

        let typed_originals: SecretData = secret
            .into_iter()
            .filter(|(_, value)| !value.is_string())
            .collect();

        let mut key_textarea = TextArea::default();
        let mut value_textarea = TextArea::default();

        // Style the textareas with green cursor
        key_textarea.set_cursor_style(Style::default().bg(Color::Green).fg(Color::Black));
        value_textarea.set_cursor_style(Style::default().bg(Color::Green).fg(Color::Black));

        Self {
            path,
            fields,
            typed_originals,
            selected_index: 0,
            mode: EditMode::Navigate,
            key_textarea,
            value_textarea,
            is_new: false,
            has_changes: false,
            error: None,
            delete_confirmation: None,
        }
    }

    /// Create a new edit view component for creating a new secret
    pub fn new_secret(path: String) -> Self {
        let mut key_textarea = TextArea::default();
        let mut value_textarea = TextArea::default();

        // Style the textareas with green cursor
        key_textarea.set_cursor_style(Style::default().bg(Color::Green).fg(Color::Black));
        value_textarea.set_cursor_style(Style::default().bg(Color::Green).fg(Color::Black));

        Self {
            path,
            fields: vec![("".to_string(), "".to_string())],
            typed_originals: SecretData::new(),
            selected_index: 0,
            mode: EditMode::EditKey,
            key_textarea,
            value_textarea,
            is_new: true,
            has_changes: false,
            error: None,
            delete_confirmation: None,
        }
    }

    /// Get the path
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Get secret data for saving, with value types restored.
    ///
    /// Fails with a message for the user if a field that held a non-string
    /// value was edited into something that is not valid JSON.
    pub fn secret_data(&self) -> Result<SecretData, String> {
        self.fields
            .iter()
            .map(|(key, text)| Ok((key.clone(), self.value_for_save(key, text)?)))
            .collect()
    }

    /// Value to store for a field: see `edited_value` for the rules
    fn value_for_save(&self, key: &str, text: &str) -> Result<serde_json::Value, String> {
        let original = self.typed_originals.get(key);
        edited_value(text, original).map_err(|_| {
            let kind = original.map(json_type_name).unwrap_or("value");
            format!(
                "Field '{key}' holds a JSON {kind}, and its new value is not valid JSON. \
                 Fix it, or put it in double quotes to save it as text."
            )
        })
    }

    /// Rename the selected field's key, keeping its original type attached
    fn rename_selected_key(&mut self, new_key: String) {
        if let Some((key, _)) = self.fields.get_mut(self.selected_index) {
            if *key != new_key
                && let Some(original) = self.typed_originals.remove(key.as_str())
            {
                self.typed_originals.insert(new_key.clone(), original);
            }
            *key = new_key;
            self.has_changes = true;
        }
    }

    /// Check if in edit mode
    pub fn is_editing(&self) -> bool {
        self.mode != EditMode::Navigate
    }

    /// Get fields as HashMap for saving
    pub fn get_data(&self) -> Result<SecretData, String> {
        self.fields
            .iter()
            .filter(|(k, _)| !k.is_empty())
            .map(|(key, text)| Ok((key.clone(), self.value_for_save(key, text)?)))
            .collect()
    }

    /// Set error message
    pub fn set_error(&mut self, error: Option<String>) {
        self.error = error;
    }

    /// Check if delete confirmation is active
    pub fn has_delete_confirmation(&self) -> bool {
        self.delete_confirmation.is_some()
    }

    /// Move selection up (Navigate mode only)
    fn move_up(&mut self) {
        if self.mode == EditMode::Navigate && self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    /// Move selection down (Navigate mode only)
    fn move_down(&mut self) {
        if self.mode == EditMode::Navigate {
            let max = self.fields.len().saturating_sub(1);
            if self.selected_index < max {
                self.selected_index += 1;
            }
        }
    }

    /// Start editing the selected field's value
    fn start_edit_value(&mut self) {
        if let Some((_, value)) = self.fields.get(self.selected_index) {
            self.value_textarea = TextArea::new(value.split('\n').map(|s| s.to_string()).collect());
            self.value_textarea
                .set_cursor_style(Style::default().bg(Color::Green).fg(Color::Black));
            self.value_textarea
                .move_cursor(ratatui_textarea::CursorMove::End);
            self.mode = EditMode::EditValue;
        }
    }

    /// Start editing the selected field's key
    fn start_edit_key(&mut self) {
        if let Some((key, _)) = self.fields.get(self.selected_index) {
            self.key_textarea = TextArea::new(vec![key.clone()]);
            self.key_textarea
                .set_cursor_style(Style::default().bg(Color::Green).fg(Color::Black));
            self.key_textarea
                .move_cursor(ratatui_textarea::CursorMove::End);
            self.mode = EditMode::EditKey;
        }
    }

    /// Cancel current edit (Esc in edit mode)
    fn cancel_edit(&mut self) {
        self.key_textarea = TextArea::default();
        self.value_textarea = TextArea::default();
        self.key_textarea
            .set_cursor_style(Style::default().bg(Color::Green).fg(Color::Black));
        self.value_textarea
            .set_cursor_style(Style::default().bg(Color::Green).fg(Color::Black));
        self.mode = EditMode::Navigate;
    }

    /// Confirm current edit (Ctrl+s in edit mode)
    fn confirm_edit(&mut self) {
        match self.mode {
            EditMode::EditValue => {
                if let Some((_, value)) = self.fields.get_mut(self.selected_index) {
                    *value = self.value_textarea.lines().join("\n");
                    self.has_changes = true;
                }
            }
            EditMode::EditKey => {
                let new_key = self.key_textarea.lines().join("\n");
                self.rename_selected_key(new_key);
            }
            EditMode::Navigate => {}
        }
        self.key_textarea = TextArea::default();
        self.value_textarea = TextArea::default();
        self.key_textarea
            .set_cursor_style(Style::default().bg(Color::Green).fg(Color::Black));
        self.value_textarea
            .set_cursor_style(Style::default().bg(Color::Green).fg(Color::Black));
        self.mode = EditMode::Navigate;
    }

    /// Toggle between EditKey and EditValue modes (Tab key)
    fn toggle_edit_mode(&mut self) {
        match self.mode {
            EditMode::EditKey => {
                // Save current key edit
                let new_key = self.key_textarea.lines().join("\n");
                self.rename_selected_key(new_key);
                // Start editing the value
                if let Some((_, value)) = self.fields.get(self.selected_index) {
                    self.value_textarea =
                        TextArea::new(value.split('\n').map(|s| s.to_string()).collect());
                    self.value_textarea
                        .set_cursor_style(Style::default().bg(Color::Green).fg(Color::Black));
                    self.value_textarea
                        .move_cursor(ratatui_textarea::CursorMove::End);
                    self.mode = EditMode::EditValue;
                }
            }
            EditMode::EditValue => {
                // Save current value edit
                if let Some((_, value)) = self.fields.get_mut(self.selected_index) {
                    *value = self.value_textarea.lines().join("\n");
                    self.has_changes = true;
                }
                // Start editing the key
                if let Some((key, _)) = self.fields.get(self.selected_index) {
                    self.key_textarea = TextArea::new(vec![key.clone()]);
                    self.key_textarea
                        .set_cursor_style(Style::default().bg(Color::Green).fg(Color::Black));
                    self.key_textarea
                        .move_cursor(ratatui_textarea::CursorMove::End);
                    self.mode = EditMode::EditKey;
                }
            }
            EditMode::Navigate => {}
        }
    }

    /// Handle input for the textarea (when in edit mode)
    fn handle_textarea_input(&mut self, input: Input) -> bool {
        match self.mode {
            EditMode::EditKey => {
                self.key_textarea.input(input);
                true
            }
            EditMode::EditValue => {
                self.value_textarea.input(input);
                true
            }
            EditMode::Navigate => false,
        }
    }

    /// Add a new field (Navigate mode only)
    fn add_field(&mut self) {
        if self.mode == EditMode::Navigate {
            self.fields.push(("".to_string(), "".to_string()));
            self.selected_index = self.fields.len() - 1;
            self.has_changes = true;
            // Start editing the new field's key
            self.start_edit_key();
        }
    }

    /// Delete the selected field (Navigate mode only)
    fn delete_field(&mut self) {
        if self.mode == EditMode::Navigate && !self.fields.is_empty() {
            // Show confirmation dialog
            let field_name = self.fields[self.selected_index].0.clone();
            self.delete_confirmation = Some(field_name);
        }
    }

    /// Confirm and execute field deletion
    fn confirm_delete_field(&mut self) {
        if self.delete_confirmation.is_some()
            && self.mode == EditMode::Navigate
            && !self.fields.is_empty()
        {
            let (removed_key, _) = self.fields.remove(self.selected_index);
            // A field added later under the same name starts as a string
            self.typed_originals.remove(&removed_key);
            self.has_changes = true;
            if self.selected_index >= self.fields.len() && self.selected_index > 0 {
                self.selected_index -= 1;
            }
            // Ensure at least one field exists for new secrets
            if self.is_new && self.fields.is_empty() {
                self.fields.push(("".to_string(), "".to_string()));
                self.selected_index = 0;
            }
            self.delete_confirmation = None;
        }
    }

    /// Cancel field deletion
    fn cancel_delete_field(&mut self) {
        self.delete_confirmation = None;
    }

    // ═══════════════════════════════════════════════════════════════
    // RENDERING METHODS (preserved from original EditView)
    // ═══════════════════════════════════════════════════════════════

    fn render_header(&self, frame: &mut Frame, area: Rect) {
        let mode_str = if self.is_new { "Create New" } else { "Edit" };
        let title = Paragraph::new(vec![
            Line::from(vec![Span::styled(
                format!("✏️  {} Secret", mode_str),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )]),
            Line::from(vec![
                Span::styled("Path: ", Style::default().fg(Color::Gray)),
                Span::styled(&self.path, Style::default().fg(Color::Green)),
            ]),
        ])
        .block(Block::default().borders(Borders::ALL));

        frame.render_widget(title, area);
    }

    fn render_content(&self, frame: &mut Frame, area: Rect) {
        if self.fields.is_empty() {
            let empty = Paragraph::new("Press 'a' to add a field")
                .style(Style::default().fg(Color::Yellow))
                .block(Block::default().borders(Borders::ALL).title("Empty Secret"));
            frame.render_widget(empty, area);
            return;
        }

        // Split area for fields list and textarea editor
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(if self.mode != EditMode::Navigate {
                vec![
                    Constraint::Percentage(50), // Fields list
                    Constraint::Percentage(50), // TextArea editor
                ]
            } else {
                vec![Constraint::Percentage(100)] // Just fields list when not editing
            })
            .split(area);

        // Render fields list
        let items: Vec<ListItem> = self
            .fields
            .iter()
            .enumerate()
            .map(|(i, (key, value))| {
                let is_selected = i == self.selected_index;

                let key_str = if key.is_empty() { "<key>" } else { key };

                // Show value preview (first line only)
                let value_preview = if value.is_empty() {
                    "<value>".to_string()
                } else {
                    let lines: Vec<&str> = value.split('\n').collect();
                    if lines.len() > 1 {
                        format!("{} [↵ {} lines]", lines[0], lines.len())
                    } else {
                        value.clone()
                    }
                };

                let style = if is_selected {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };

                let key_style = if is_selected {
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
                    Span::styled(format!("{:<20}", key_str), key_style),
                    Span::styled(" │ ", Style::default().fg(Color::DarkGray)),
                    Span::styled(value_preview, style),
                ]);

                ListItem::new(line)
            })
            .collect();

        let list = List::new(items).block(Block::default().borders(Borders::ALL).title(format!(
            "Fields ({} {})",
            self.fields.len(),
            if self.fields.len() == 1 {
                "field"
            } else {
                "fields"
            }
        )));

        frame.render_widget(list, chunks[0]);

        // Render textarea editor if in edit mode
        if self.mode != EditMode::Navigate {
            let editor_title = match self.mode {
                EditMode::EditKey => "Editing Key",
                EditMode::EditValue => "Editing Value",
                EditMode::Navigate => "",
            };

            // Clone the textarea with styled block
            match self.mode {
                EditMode::EditKey => {
                    let mut textarea = self.key_textarea.clone();
                    textarea.set_block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title(editor_title)
                            .border_style(Style::default().fg(Color::Green)),
                    );
                    frame.render_widget(&textarea, chunks[1]);
                }
                EditMode::EditValue => {
                    let mut textarea = self.value_textarea.clone();
                    textarea.set_block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title(editor_title)
                            .border_style(Style::default().fg(Color::Green)),
                    );
                    frame.render_widget(&textarea, chunks[1]);
                }
                EditMode::Navigate => {}
            }
        }

        // Show error if any
        if let Some(ref error) = self.error {
            let error_area = Rect {
                x: area.x + 2,
                y: area.y + area.height - 4,
                width: area.width.saturating_sub(4),
                height: 3,
            };

            let error_msg = Paragraph::new(error.as_str())
                .style(Style::default().fg(Color::Red))
                .block(Block::default().borders(Borders::ALL).title("Error"));

            frame.render_widget(Clear, error_area);
            frame.render_widget(error_msg, error_area);
        }
    }

    fn render_footer(&self, frame: &mut Frame, area: Rect) {
        let help = match self.mode {
            EditMode::Navigate => Line::from(vec![
                Span::styled(
                    "e",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(": Edit | "),
                Span::styled(
                    "a",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(": Add field | "),
                Span::styled(
                    "d",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
                Span::raw(": Delete | "),
                Span::styled(
                    "Ctrl+s",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(": Save secret | "),
                Span::styled(
                    "Esc",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(": Cancel"),
            ]),
            EditMode::EditValue | EditMode::EditKey => Line::from(vec![
                Span::styled(
                    "Ctrl+s",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(": Confirm field | "),
                Span::styled(
                    "Tab",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(": Switch key/value | "),
                Span::styled(
                    "Esc",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(": Cancel edit"),
            ]),
        };

        let footer = Paragraph::new(help).block(Block::default().borders(Borders::ALL));

        frame.render_widget(footer, area);
    }

    fn render_delete_confirmation(&self, frame: &mut Frame, area: Rect, field_name: &str) {
        // Calculate centered popup area (smaller than the full screen)
        let popup_area = Self::centered_rect(50, 30, area);

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

        // Title
        let title = Paragraph::new(Line::from(vec![Span::styled(
            " Confirm Delete Field ",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )]))
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Red)),
        );
        frame.render_widget(title, chunks[0]);

        // Content
        let display_field_name = if field_name.is_empty() {
            "<empty key>"
        } else {
            field_name
        };

        let message = vec![
            Line::from(""),
            Line::from(Span::styled(
                "Are you sure you want to delete field:",
                Style::default().fg(Color::White),
            )),
            Line::from(""),
            Line::from(Span::styled(
                display_field_name,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "This action cannot be undone!",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )),
        ];

        let content = Paragraph::new(message).alignment(Alignment::Center).block(
            Block::default()
                .borders(Borders::LEFT | Borders::RIGHT)
                .border_style(Style::default().fg(Color::Red)),
        );
        frame.render_widget(content, chunks[1]);

        // Footer with instructions
        let footer = Paragraph::new(Line::from(vec![
            Span::styled(
                "y / Enter",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(": Confirm | "),
            Span::styled(
                "n / Esc",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(": Cancel"),
        ]))
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Red)),
        );
        frame.render_widget(footer, chunks[2]);
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
}

impl Component for EditViewComponent {
    fn handle_key(&mut self, key: KeyEvent, _ctx: &ComponentContext) -> Result<Option<Action>> {
        // Delete confirmation dialog captures input
        if self.has_delete_confirmation() {
            return match (key.code, key.modifiers) {
                (KeyCode::Char('y'), KeyModifiers::NONE) | (KeyCode::Enter, KeyModifiers::NONE) => {
                    self.confirm_delete_field();
                    Ok(None)
                }
                (KeyCode::Char('n'), KeyModifiers::NONE) | (KeyCode::Esc, KeyModifiers::NONE) => {
                    self.cancel_delete_field();
                    Ok(None)
                }
                _ => Ok(None),
            };
        }

        // Different key handling based on mode
        match self.mode {
            EditMode::Navigate => {
                // Navigate mode key handling
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

                    // Start editing value
                    (KeyCode::Char('e'), KeyModifiers::NONE)
                    | (KeyCode::Enter, KeyModifiers::NONE) => Ok(Some(Action::StartEditValue)),

                    // Add field
                    (KeyCode::Char('a'), KeyModifiers::NONE) => Ok(Some(Action::AddField)),

                    // Delete field
                    (KeyCode::Char('d'), KeyModifiers::NONE) => Ok(Some(Action::DeleteField)),

                    // Save secret
                    (KeyCode::Char('s'), KeyModifiers::CONTROL) => {
                        // Refuse to save rather than change a value's type
                        match self.secret_data() {
                            Ok(data) => Ok(Some(Action::SaveSecret {
                                path: self.path.clone(),
                                data,
                            })),
                            Err(message) => Ok(Some(Action::SetError { message })),
                        }
                    }

                    // Cancel/Exit
                    (KeyCode::Esc, KeyModifiers::NONE) => Ok(Some(Action::CancelEdit)),

                    // Help
                    (KeyCode::Char('?'), KeyModifiers::NONE) => Ok(Some(Action::ToggleHelp)),

                    _ => Ok(None),
                }
            }
            EditMode::EditKey | EditMode::EditValue => {
                // Edit mode key handling
                match (key.code, key.modifiers) {
                    // Confirm edit (save field)
                    (KeyCode::Char('s'), KeyModifiers::CONTROL) => Ok(Some(Action::ConfirmEdit)),

                    // Toggle between key/value
                    (KeyCode::Tab, KeyModifiers::NONE) => Ok(Some(Action::ToggleEditTarget)),

                    // Cancel edit
                    (KeyCode::Esc, KeyModifiers::NONE) => Ok(Some(Action::CancelEdit)),

                    // All other keys are passed to TextArea
                    _ => {
                        // Convert to ratatui_textarea::Input
                        let input = crossterm_to_textarea_input(key);
                        self.handle_textarea_input(input);
                        Ok(None)
                    }
                }
            }
        }
    }

    fn update(&mut self, action: Action, _ctx: &mut ComponentContext) -> Result<Option<Action>> {
        match action {
            // Start editing value
            Action::StartEditValue => {
                self.start_edit_value();
                Ok(None)
            }

            // Start editing key
            Action::StartEditKey => {
                self.start_edit_key();
                Ok(None)
            }

            // Confirm edit
            Action::ConfirmEdit => {
                self.confirm_edit();
                Ok(None)
            }

            // Cancel edit
            Action::CancelEdit => {
                if self.is_editing() {
                    self.cancel_edit();
                    Ok(None)
                } else {
                    // In Navigate mode, this means close the edit view
                    Ok(Some(Action::ScreenBack))
                }
            }

            // Toggle edit target (key/value)
            Action::ToggleEditTarget => {
                self.toggle_edit_mode();
                Ok(None)
            }

            // Add field
            Action::AddField => {
                self.add_field();
                Ok(None)
            }

            // Delete field
            Action::DeleteField => {
                self.delete_field();
                Ok(None)
            }

            _ => Ok(None),
        }
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect, _ctx: &RenderContext) {
        // Clear the background
        frame.render_widget(Clear, area);

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

        // Render delete confirmation dialog if active
        if let Some(ref field_name) = self.delete_confirmation {
            self.render_delete_confirmation(frame, area, field_name);
        }
    }

    fn captures_input(&self) -> bool {
        // Capture input when delete confirmation is active
        self.has_delete_confirmation()
    }
}

/// Convert crossterm KeyEvent to tui-textarea Input
fn crossterm_to_textarea_input(key: KeyEvent) -> Input {
    use crossterm::event::{KeyCode as CKC, KeyModifiers as CKM};
    use ratatui_textarea::{Input, Key};

    let key_code = match key.code {
        CKC::Char(c) => Key::Char(c),
        CKC::Backspace => Key::Backspace,
        CKC::Enter => Key::Enter,
        CKC::Left => Key::Left,
        CKC::Right => Key::Right,
        CKC::Up => Key::Up,
        CKC::Down => Key::Down,
        CKC::Home => Key::Home,
        CKC::End => Key::End,
        CKC::PageUp => Key::PageUp,
        CKC::PageDown => Key::PageDown,
        CKC::Tab => Key::Tab,
        CKC::Delete => Key::Delete,
        CKC::Insert => Key::Null,
        CKC::F(n) => Key::F(n),
        CKC::Esc => Key::Esc,
        _ => Key::Null,
    };

    Input {
        key: key_code,
        ctrl: key.modifiers.contains(CKM::CONTROL),
        alt: key.modifiers.contains(CKM::ALT),
        shift: key.modifiers.contains(CKM::SHIFT),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_edit_view_creation() {
        let mut secret = SecretData::new();
        secret.insert("username".to_string(), "admin".into());
        secret.insert("password".to_string(), "secret123".into());

        let view = EditViewComponent::new("/secret/test".to_string(), secret);
        assert_eq!(view.path(), "/secret/test");
        assert_eq!(view.fields.len(), 2);
        assert_eq!(view.mode, EditMode::Navigate);
        assert!(!view.is_new);
    }

    #[test]
    fn test_new_secret() {
        let view = EditViewComponent::new_secret("/secret/test".to_string());
        assert_eq!(view.path(), "/secret/test");
        assert_eq!(view.fields.len(), 1);
        assert_eq!(view.mode, EditMode::EditKey);
        assert!(view.is_new);
    }

    #[test]
    fn test_navigation() {
        let mut secret = SecretData::new();
        secret.insert("key1".to_string(), "value1".into());
        secret.insert("key2".to_string(), "value2".into());
        secret.insert("key3".to_string(), "value3".into());

        let mut view = EditViewComponent::new("/secret/test".to_string(), secret);

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
    fn test_mode_transitions() {
        let mut secret = SecretData::new();
        secret.insert("key".to_string(), "value".into());

        let mut view = EditViewComponent::new("/secret/test".to_string(), secret);

        assert_eq!(view.mode, EditMode::Navigate);
        assert!(!view.is_editing());

        view.start_edit_value();
        assert_eq!(view.mode, EditMode::EditValue);
        assert!(view.is_editing());

        view.cancel_edit();
        assert_eq!(view.mode, EditMode::Navigate);
        assert!(!view.is_editing());
    }

    #[test]
    fn test_get_data() {
        let mut view = EditViewComponent::new_secret("/secret/test".to_string());
        view.fields = vec![
            ("username".to_string(), "admin".to_string()),
            ("".to_string(), "ignored".to_string()), // Empty key should be filtered
            ("password".to_string(), "secret".to_string()),
        ];

        let data = view.get_data().unwrap();
        assert_eq!(data.len(), 2);
        assert_eq!(data.get("username"), Some(&json!("admin")));
        assert_eq!(data.get("password"), Some(&json!("secret")));
        assert!(!data.contains_key(""));
    }

    fn typed_secret() -> SecretData {
        serde_json::from_value(json!({
            "host": "db",
            "port": 5432,
            "tls": true,
            "tags": ["a", "b"],
            "note": null,
            "numeric_string": "123"
        }))
        .unwrap()
    }

    /// Replace the value of the field named `key`, as the value editor does
    fn set_value(view: &mut EditViewComponent, key: &str, text: &str) {
        view.selected_index = view.fields.iter().position(|(k, _)| k == key).unwrap();
        view.start_edit_value();
        view.value_textarea = TextArea::new(vec![text.to_string()]);
        view.confirm_edit();
    }

    #[test]
    fn test_save_without_edits_is_lossless() {
        // Regression: non-string values were dropped on load, so opening the
        // editor and saving deleted them from Vault.
        let secret = typed_secret();
        let view = EditViewComponent::new("/secret/test".to_string(), secret.clone());

        assert_eq!(view.fields.len(), 6);
        assert_eq!(view.secret_data().unwrap(), secret);
    }

    #[test]
    fn test_editing_one_field_keeps_other_types() {
        let mut view = EditViewComponent::new("/secret/test".to_string(), typed_secret());

        set_value(&mut view, "host", "db2");

        let data = view.secret_data().unwrap();
        assert_eq!(data["host"], json!("db2"));
        assert_eq!(data["port"], json!(5432));
        assert_eq!(data["tls"], json!(true));
        assert_eq!(data["tags"], json!(["a", "b"]));
        assert_eq!(data["note"], json!(null));
        assert_eq!(data["numeric_string"], json!("123"));
    }

    #[test]
    fn test_editing_typed_and_string_fields() {
        let mut view = EditViewComponent::new("/secret/test".to_string(), typed_secret());

        set_value(&mut view, "port", "5433");
        set_value(&mut view, "tls", "false");
        // A string stays a string even when its text looks like a number
        set_value(&mut view, "numeric_string", "456");

        let data = view.secret_data().unwrap();
        assert_eq!(data["port"], json!(5433));
        assert_eq!(data["tls"], json!(false));
        assert_eq!(data["numeric_string"], json!("456"));
    }

    #[test]
    fn test_invalid_json_in_a_typed_field_blocks_the_save() {
        let mut view = EditViewComponent::new("/secret/test".to_string(), typed_secret());

        set_value(&mut view, "tags", r#"["a","b"3"#);

        let message = view.secret_data().unwrap_err();
        assert!(message.contains("'tags'"), "{message}");
        assert!(message.contains("array"), "{message}");

        // Ctrl+S reports the problem instead of sending the write
        let config = crate::config::Config {
            vault_addr: "http://127.0.0.1:1".to_string(),
            vault_token: "test-token".to_string(),
            ..Default::default()
        };
        let app = crate::app::App::new(config, "/".to_string()).unwrap();
        let focus = crate::tui::context::FocusState::new();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let ctx = ComponentContext::new(&app, &focus, &tx);
        let save = KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL);
        let action = view.handle_key(save, &ctx).unwrap();
        assert!(
            matches!(action, Some(Action::SetError { .. })),
            "{action:?}"
        );

        // Quoting it is the explicit way to store text instead
        set_value(&mut view, "tags", r#""a,b""#);
        assert_eq!(view.secret_data().unwrap()["tags"], json!("a,b"));
    }

    #[test]
    fn test_renaming_a_typed_field_keeps_its_type() {
        let mut view = EditViewComponent::new("/secret/test".to_string(), typed_secret());

        view.selected_index = view.fields.iter().position(|(k, _)| k == "port").unwrap();
        view.start_edit_key();
        view.key_textarea = TextArea::new(vec!["db_port".to_string()]);
        view.confirm_edit();

        let data = view.secret_data().unwrap();
        assert!(!data.contains_key("port"));
        assert_eq!(data["db_port"], json!(5432));
    }

    #[test]
    fn test_new_and_readded_fields_are_strings() {
        let mut view = EditViewComponent::new("/secret/test".to_string(), typed_secret());

        // Delete the numeric field, then add a new one with the same name
        view.selected_index = view.fields.iter().position(|(k, _)| k == "port").unwrap();
        view.delete_field();
        view.confirm_delete_field();
        view.fields.push(("port".to_string(), "8080".to_string()));
        view.fields.push(("retries".to_string(), "3".to_string()));

        let data = view.secret_data().unwrap();
        assert_eq!(data["port"], json!("8080"));
        assert_eq!(data["retries"], json!("3"));
    }
}
