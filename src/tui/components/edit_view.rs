use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
};
use ratatui_textarea::{Input, TextArea};

use crate::app::secret::{display_value, edited_value, json_type_name};
use crate::tui::action::Action;
use crate::tui::component::Component;
use crate::tui::components::popup::{
    border_color, centered, display_width, selection_style, truncate, wrap,
};
use crate::tui::context::{ComponentContext, RenderContext};
use crate::vault::SecretData;

/// Widest the key column gets, in columns
const MAX_KEY_WIDTH: usize = 32;

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
///
/// Nothing reaches Vault until Ctrl+S. Enter keeps what was typed in a field
/// and Esc discards it; Ctrl+S saves the secret from any mode.
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

    /// Error message, shown under the fields until the next key
    error: Option<String>,

    /// Delete confirmation - stores the field name to be deleted
    delete_confirmation: Option<String>,

    /// Shown above the fields when the edit starts from an old version, so
    /// that saving it is not a rollback by accident
    restore_note: Option<String>,

    /// Scroll state of the field list
    list_state: ListState,

    /// Rows the field list had the last time it was drawn, for paging
    page: usize,
}

fn styled_textarea(lines: Vec<String>) -> TextArea<'static> {
    let mut textarea = TextArea::new(lines);
    textarea.set_cursor_style(Style::default().bg(Color::Green).fg(Color::Black));
    textarea.set_cursor_line_style(Style::default());
    textarea.move_cursor(ratatui_textarea::CursorMove::End);
    textarea
}

impl EditViewComponent {
    fn with_fields(path: String, fields: Vec<(String, String)>, typed: SecretData) -> Self {
        Self {
            path,
            fields,
            typed_originals: typed,
            selected_index: 0,
            mode: EditMode::Navigate,
            key_textarea: styled_textarea(Vec::new()),
            value_textarea: styled_textarea(Vec::new()),
            is_new: false,
            has_changes: false,
            error: None,
            delete_confirmation: None,
            restore_note: None,
            list_state: ListState::default(),
            page: 10,
        }
    }

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

        Self::with_fields(path, fields, typed_originals)
    }

    /// Create a new edit view component for creating a new secret
    pub fn new_secret(path: String) -> Self {
        let mut view = Self::with_fields(
            path,
            vec![("".to_string(), "".to_string())],
            SecretData::new(),
        );
        view.mode = EditMode::EditKey;
        view.is_new = true;
        view
    }

    /// Say that the fields are a copy of an old version. Saving such an edit
    /// replaces the current version, so the view says so the whole time.
    pub fn with_restore_note(mut self, version: u64, current: u64) -> Self {
        self.restore_note = Some(format!(
            "Copy of v{version}: saving it replaces the current v{current}"
        ));
        // The fields already differ from what is in Vault
        self.has_changes = true;
        self
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
            if *key != new_key {
                self.has_changes = true;
            }
            *key = new_key;
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

    /// Move the cursor to `index`, clamped (Navigate mode only)
    fn select(&mut self, index: usize) {
        if self.mode == EditMode::Navigate {
            self.selected_index = index.min(self.fields.len().saturating_sub(1));
        }
    }

    /// Move selection up (Navigate mode only)
    fn move_up(&mut self) {
        self.select(self.selected_index.saturating_sub(1));
    }

    /// Move selection down (Navigate mode only)
    fn move_down(&mut self) {
        self.select(self.selected_index + 1);
    }

    /// Start editing the selected field's value
    fn start_edit_value(&mut self) {
        if let Some((_, value)) = self.fields.get(self.selected_index) {
            self.value_textarea = styled_textarea(value.split('\n').map(str::to_string).collect());
            self.mode = EditMode::EditValue;
        }
    }

    /// Start editing the selected field's key
    fn start_edit_key(&mut self) {
        if let Some((key, _)) = self.fields.get(self.selected_index) {
            self.key_textarea = styled_textarea(vec![key.clone()]);
            self.mode = EditMode::EditKey;
        }
    }

    /// Cancel current edit (Esc in edit mode)
    fn cancel_edit(&mut self) {
        self.mode = EditMode::Navigate;
    }

    /// Keep the text of the field being edited, without leaving the mode
    fn commit_field(&mut self) {
        match self.mode {
            EditMode::EditValue => {
                let text = self.value_textarea.lines().join("\n");
                if let Some((_, value)) = self.fields.get_mut(self.selected_index)
                    && *value != text
                {
                    *value = text;
                    self.has_changes = true;
                }
            }
            EditMode::EditKey => {
                // A key is one line
                let new_key = self.key_textarea.lines().join("");
                self.rename_selected_key(new_key);
            }
            EditMode::Navigate => {}
        }
    }

    /// Confirm current edit (Enter in edit mode)
    fn confirm_edit(&mut self) {
        self.commit_field();
        self.mode = EditMode::Navigate;
    }

    /// Toggle between EditKey and EditValue modes (Tab key)
    fn toggle_edit_mode(&mut self) {
        match self.mode {
            EditMode::EditKey => {
                self.commit_field();
                self.start_edit_value();
            }
            EditMode::EditValue => {
                self.commit_field();
                self.start_edit_key();
            }
            EditMode::Navigate => {}
        }
    }

    /// Handle input for the textarea (when in edit mode)
    fn handle_textarea_input(&mut self, input: Input) -> bool {
        match self.mode {
            EditMode::EditKey => {
                self.key_textarea.input(input);
                // Whatever the key binding, a key never gets a line break
                if self.key_textarea.lines().len() > 1 {
                    let joined = self.key_textarea.lines().join("");
                    self.key_textarea = styled_textarea(vec![joined]);
                }
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

    /// Ctrl+S: send the secret to Vault, or say why it cannot be sent.
    ///
    /// A problem with a value is shown here, next to the fields, so the edit
    /// stays on screen while it is fixed.
    fn save(&mut self) -> Option<Action> {
        match self.secret_data() {
            Ok(data) => Some(Action::SaveSecret {
                path: self.path.clone(),
                data,
            }),
            Err(message) => {
                self.error = Some(message);
                None
            }
        }
    }

    // ═══════════════════════════════════════════════════════════════
    // RENDERING
    // ═══════════════════════════════════════════════════════════════

    fn render_fields(&mut self, frame: &mut Frame, area: Rect, focused: bool) {
        let widest = self
            .fields
            .iter()
            .map(|(key, _)| display_width(key))
            .max()
            .unwrap_or(0);
        let key_width = widest
            .min(MAX_KEY_WIDTH)
            .min((area.width as usize / 2).max(8))
            .max(5);

        let items: Vec<ListItem> = self
            .fields
            .iter()
            .map(|(key, value)| {
                let (key_text, key_style) = if key.is_empty() {
                    ("<key>".to_string(), Style::default().fg(Color::DarkGray))
                } else {
                    (
                        truncate(key, key_width),
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    )
                };
                let padding = key_width.saturating_sub(display_width(&key_text));

                // Show value preview (first line only)
                let (value_text, value_style) = if value.is_empty() {
                    ("<value>".to_string(), Style::default().fg(Color::DarkGray))
                } else {
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

                ListItem::new(Line::from(vec![
                    Span::styled(format!(" {key_text}{}", " ".repeat(padding)), key_style),
                    Span::styled(" │ ", Style::default().fg(Color::DarkGray)),
                    Span::styled(value_text, value_style),
                ]))
            })
            .collect();

        // While a field is being typed the cursor is in the editor below
        let highlight = selection_style(focused && self.mode == EditMode::Navigate);

        self.list_state.select(Some(self.selected_index));
        let list = List::new(items).highlight_style(highlight);
        frame.render_stateful_widget(list, area, &mut self.list_state);
    }

    fn render_editor(&self, frame: &mut Frame, area: Rect) {
        let (textarea, title) = match self.mode {
            EditMode::EditKey => (&self.key_textarea, " Key "),
            EditMode::EditValue => (&self.value_textarea, " Value "),
            EditMode::Navigate => return,
        };

        let mut textarea = textarea.clone();
        textarea.set_block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(Style::default().fg(Color::Green)),
        );
        frame.render_widget(&textarea, area);
    }

    fn render_delete_confirmation(&self, frame: &mut Frame, area: Rect, field_name: &str) {
        let popup = centered(46, 7, area);
        let inner_width = popup.width.saturating_sub(4) as usize;

        let name = if field_name.is_empty() {
            "<empty key>".to_string()
        } else {
            truncate(field_name, inner_width)
        };

        let key_style = Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD);

        let text = vec![
            Line::from(""),
            Line::from(Span::styled(
                name,
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Takes effect when the secret is saved.",
                Style::default().fg(Color::Gray),
            )),
            Line::from(vec![
                Span::styled("y", key_style),
                Span::raw(" delete   "),
                Span::styled("n/Esc", key_style),
                Span::raw(" cancel"),
            ]),
        ];

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Red))
            .title(Span::styled(
                " Delete field ",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ));

        frame.render_widget(Clear, popup);
        frame.render_widget(Paragraph::new(text).centered().block(block), popup);
    }
}

impl Component for EditViewComponent {
    fn handle_key(&mut self, key: KeyEvent, _ctx: &ComponentContext) -> Result<Option<Action>> {
        // Delete confirmation dialog captures input
        if self.has_delete_confirmation() {
            match (key.code, key.modifiers) {
                // Only `y`, as the dialog says and as for deleting a secret
                (KeyCode::Char('y'), KeyModifiers::NONE) => {
                    self.confirm_delete_field();
                }
                (KeyCode::Char('n'), KeyModifiers::NONE) | (KeyCode::Esc, KeyModifiers::NONE) => {
                    self.cancel_delete_field();
                }
                _ => {}
            }
            return Ok(None);
        }

        // An error stays until the next key
        self.error = None;

        let action = match self.mode {
            EditMode::Navigate => {
                let page = self.page.max(1);
                match (key.code, key.modifiers) {
                    // Navigation
                    (KeyCode::Char('j'), KeyModifiers::NONE)
                    | (KeyCode::Down, KeyModifiers::NONE) => {
                        self.move_down();
                        None
                    }
                    (KeyCode::Char('k'), KeyModifiers::NONE)
                    | (KeyCode::Up, KeyModifiers::NONE) => {
                        self.move_up();
                        None
                    }
                    (KeyCode::PageDown, _) => {
                        self.select(self.selected_index + page);
                        None
                    }
                    (KeyCode::PageUp, _) => {
                        self.select(self.selected_index.saturating_sub(page));
                        None
                    }
                    (KeyCode::Home, _) | (KeyCode::Char('g'), KeyModifiers::NONE) => {
                        self.select(0);
                        None
                    }
                    (KeyCode::End, _) | (KeyCode::Char('G'), _) => {
                        self.select(usize::MAX);
                        None
                    }

                    // Start editing value
                    (KeyCode::Char('e'), KeyModifiers::NONE)
                    | (KeyCode::Enter, KeyModifiers::NONE) => {
                        self.start_edit_value();
                        None
                    }

                    // Rename the key
                    (KeyCode::Char('r'), KeyModifiers::NONE) => {
                        self.start_edit_key();
                        None
                    }

                    // Add field
                    (KeyCode::Char('a'), KeyModifiers::NONE) => {
                        self.add_field();
                        None
                    }

                    // Delete field
                    (KeyCode::Char('d'), KeyModifiers::NONE) => {
                        self.delete_field();
                        None
                    }

                    // Save secret. A value whose type would change is
                    // refused rather than sent.
                    (KeyCode::Char('s'), KeyModifiers::CONTROL) => self.save(),

                    // Cancel/Exit
                    (KeyCode::Esc, KeyModifiers::NONE) => Some(Action::CancelEdit),

                    _ => None,
                }
            }
            EditMode::EditKey | EditMode::EditValue => {
                match (key.code, key.modifiers) {
                    // Save from here too: keep the text, then send the secret
                    (KeyCode::Char('s'), KeyModifiers::CONTROL) => {
                        self.confirm_edit();
                        self.save()
                    }

                    // A line break in a value. Plain Enter means "done", so
                    // it needs its own keys.
                    (KeyCode::Enter, KeyModifiers::ALT)
                    | (KeyCode::Char('j'), KeyModifiers::CONTROL)
                        if self.mode == EditMode::EditValue =>
                    {
                        self.value_textarea.insert_newline();
                        None
                    }

                    // Done with this field. From a key, go on to its value.
                    (KeyCode::Enter, _) => {
                        if self.mode == EditMode::EditKey {
                            self.toggle_edit_mode();
                        } else {
                            self.confirm_edit();
                        }
                        None
                    }

                    // Toggle between key/value
                    (KeyCode::Tab, KeyModifiers::NONE) => {
                        self.toggle_edit_mode();
                        None
                    }

                    // Discard what was typed in this field
                    (KeyCode::Esc, KeyModifiers::NONE) => {
                        self.cancel_edit();
                        None
                    }

                    // All other keys are passed to TextArea
                    _ => {
                        let input = crossterm_to_textarea_input(key);
                        self.handle_textarea_input(input);
                        None
                    }
                }
            }
        };

        Ok(action)
    }

    fn handle_paste(&mut self, text: &str, _ctx: &ComponentContext) -> Result<Option<Action>> {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        match self.mode {
            EditMode::EditKey => {
                let line: String = text.chars().filter(|c| !c.is_control()).collect();
                self.key_textarea.insert_str(line);
            }
            EditMode::EditValue => {
                self.value_textarea.insert_str(text);
            }
            // Outside a text field a paste is ignored: it must never run as
            // a series of commands
            EditMode::Navigate => {}
        }
        Ok(None)
    }

    fn update(&mut self, action: Action, _ctx: &mut ComponentContext) -> Result<Option<Action>> {
        match action {
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

            _ => Ok(None),
        }
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect, ctx: &RenderContext) {
        let focused = ctx.is_focused;

        // Clear the background
        frame.render_widget(Clear, area);

        let verb = if self.is_new { "New" } else { "Edit" };
        let title_width = area.width.saturating_sub(18) as usize;
        let mut title = vec![Span::styled(
            format!(" {verb} {} ", truncate(&self.path, title_width)),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )];
        if self.has_changes {
            title.push(Span::styled(
                "● unsaved ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ));
        }

        let count = if self.fields.len() == 1 {
            " 1 field ".to_string()
        } else {
            format!(" {} fields ", self.fields.len())
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color(focused)))
            .title(Line::from(title))
            .title_bottom(Span::styled(count, Style::default().fg(Color::Gray)));
        let inner = block.inner(area);
        frame.render_widget(block, area);

        // The error is wrapped, not cut: it says how to fix the value
        let error_lines = self
            .error
            .as_deref()
            .map(|error| {
                wrap(
                    &format!("✗ {error}"),
                    inner.width.saturating_sub(2) as usize,
                )
            })
            .unwrap_or_default();
        let error_height = error_lines.len().min(4) as u16;

        let editor_height = match self.mode {
            EditMode::Navigate => 0,
            EditMode::EditKey => 3,
            EditMode::EditValue => {
                let wanted = self.value_textarea.lines().len() as u16 + 2;
                wanted.clamp(3, (inner.height / 2).max(3))
            }
        };

        // Wrapped rather than cut, like the error: it is a warning
        let note_lines = self
            .restore_note
            .as_deref()
            .map(|note| wrap(note, inner.width.saturating_sub(2) as usize))
            .unwrap_or_default();

        let chunks = Layout::vertical([
            Constraint::Length(note_lines.len().min(2) as u16),
            Constraint::Min(1),
            Constraint::Length(editor_height),
            Constraint::Length(error_height),
        ])
        .split(inner);

        if !note_lines.is_empty() {
            let lines: Vec<Line> = note_lines
                .iter()
                .take(2)
                .map(|line| Line::from(format!(" {line}")))
                .collect();
            let banner = Paragraph::new(lines).style(
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            );
            frame.render_widget(banner, chunks[0]);
        }

        self.page = chunks[1].height as usize;
        self.render_fields(frame, chunks[1], focused);
        self.render_editor(frame, chunks[2]);

        if !error_lines.is_empty() {
            let lines: Vec<Line> = error_lines
                .iter()
                .take(error_height as usize)
                .map(|line| Line::from(format!(" {line}")))
                .collect();
            frame.render_widget(
                Paragraph::new(lines).style(Style::default().fg(Color::Red)),
                chunks[3],
            );
        }

        // Render delete confirmation dialog if active
        if let Some(ref field_name) = self.delete_confirmation {
            self.render_delete_confirmation(frame, area, field_name);
        }
    }

    fn captures_input(&self) -> bool {
        // A dialog or a text field takes every key
        self.has_delete_confirmation() || self.is_editing()
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
    use crate::tui::context::FocusState;
    use ratatui::{Terminal, backend::TestBackend};
    use serde_json::json;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    fn test_app() -> crate::app::App {
        let config = crate::config::Config {
            vault_addr: "http://127.0.0.1:1".to_string(),
            vault_token: "test-token".to_string(),
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

    fn type_text(view: &mut EditViewComponent, ctx: &ComponentContext, text: &str) {
        for c in text.chars() {
            view.handle_key(key(KeyCode::Char(c)), ctx).unwrap();
        }
    }

    /// Text on screen after drawing the view at the given size
    fn rendered(view: &mut EditViewComponent, width: u16, height: u16) -> String {
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

        // Ctrl+S reports the problem instead of sending the write, and
        // reports it here, with the fields still on screen
        with_ctx(|ctx| {
            let action = view.handle_key(ctrl('s'), ctx).unwrap();
            assert!(action.is_none(), "{action:?}");
            let error = view.error.as_deref().expect("the problem is shown");
            assert!(error.contains("'tags'"), "{error}");

            let screen = rendered(&mut view, 70, 16);
            assert!(screen.contains("'tags'"), "{screen}");
            assert!(
                screen.contains("double quotes"),
                "the advice is cut off:\n{screen}"
            );
            assert!(screen.contains("host"), "{screen}");
        });

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

    #[test]
    fn ctrl_s_while_typing_saves_in_one_step() {
        // Regression: Ctrl+S in a field only confirmed the field, and the
        // secret needed a second Ctrl+S.
        with_ctx(|ctx| {
            let mut view = EditViewComponent::new(
                "/secret/test".to_string(),
                serde_json::from_value(json!({"host": "db"})).unwrap(),
            );

            view.handle_key(key(KeyCode::Enter), ctx).unwrap();
            assert_eq!(view.mode, EditMode::EditValue);
            type_text(&mut view, ctx, "2");

            let action = view.handle_key(ctrl('s'), ctx).unwrap();
            match action {
                Some(Action::SaveSecret { path, data }) => {
                    assert_eq!(path, "/secret/test");
                    assert_eq!(data["host"], json!("db2"));
                }
                other => panic!("expected a save, got {other:?}"),
            }
            assert_eq!(view.mode, EditMode::Navigate);
        });
    }

    #[test]
    fn enter_finishes_a_field_and_escape_discards_it() {
        with_ctx(|ctx| {
            let mut view = EditViewComponent::new(
                "/secret/test".to_string(),
                serde_json::from_value(json!({"host": "db"})).unwrap(),
            );

            // Enter keeps the text and goes back to the fields
            view.handle_key(key(KeyCode::Enter), ctx).unwrap();
            type_text(&mut view, ctx, "-1");
            view.handle_key(key(KeyCode::Enter), ctx).unwrap();
            assert_eq!(view.mode, EditMode::Navigate);
            assert_eq!(view.fields[0].1, "db-1");
            assert!(view.has_changes);

            // Esc drops what was typed
            view.handle_key(key(KeyCode::Enter), ctx).unwrap();
            type_text(&mut view, ctx, "xyz");
            view.handle_key(key(KeyCode::Esc), ctx).unwrap();
            assert_eq!(view.mode, EditMode::Navigate);
            assert_eq!(view.fields[0].1, "db-1");
        });
    }

    #[test]
    fn opening_and_closing_a_field_is_not_a_change() {
        with_ctx(|ctx| {
            let mut view = EditViewComponent::new(
                "/secret/test".to_string(),
                serde_json::from_value(json!({"host": "db"})).unwrap(),
            );

            view.handle_key(key(KeyCode::Enter), ctx).unwrap();
            view.handle_key(key(KeyCode::Enter), ctx).unwrap();
            view.handle_key(key(KeyCode::Char('r')), ctx).unwrap();
            assert_eq!(view.mode, EditMode::EditKey);
            view.handle_key(key(KeyCode::Tab), ctx).unwrap();
            view.handle_key(key(KeyCode::Enter), ctx).unwrap();

            assert!(!view.has_changes);
            assert!(!rendered(&mut view, 60, 10).contains("unsaved"));
        });
    }

    #[test]
    fn a_new_field_flows_from_key_to_value() {
        with_ctx(|ctx| {
            let mut view = EditViewComponent::new_secret("/secret/new".to_string());
            assert_eq!(view.mode, EditMode::EditKey);

            type_text(&mut view, ctx, "token");
            view.handle_key(key(KeyCode::Enter), ctx).unwrap();
            assert_eq!(view.mode, EditMode::EditValue);

            type_text(&mut view, ctx, "abc");
            view.handle_key(key(KeyCode::Enter), ctx).unwrap();
            assert_eq!(view.mode, EditMode::Navigate);
            assert_eq!(view.fields, [("token".to_string(), "abc".to_string())]);
        });
    }

    #[test]
    fn line_breaks_go_into_values_only() {
        with_ctx(|ctx| {
            let mut view = EditViewComponent::new_secret("/secret/new".to_string());

            // A key never gets one, however it is typed or pasted
            type_text(&mut view, ctx, "ce");
            view.handle_key(ctrl('m'), ctx).unwrap();
            view.handle_paste("rt\nificate", ctx).unwrap();
            view.handle_key(key(KeyCode::Tab), ctx).unwrap();
            assert_eq!(view.fields[0].0, "certificate");

            // A value does: Alt+Enter, Ctrl+J, or a paste
            assert_eq!(view.mode, EditMode::EditValue);
            type_text(&mut view, ctx, "one");
            view.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT), ctx)
                .unwrap();
            type_text(&mut view, ctx, "two");
            view.handle_key(ctrl('j'), ctx).unwrap();
            view.handle_paste("three\r\nfour", ctx).unwrap();
            view.handle_key(key(KeyCode::Enter), ctx).unwrap();

            assert_eq!(view.mode, EditMode::Navigate);
            assert_eq!(view.fields[0].1, "one\ntwo\nthree\nfour");
        });
    }

    #[test]
    fn a_paste_outside_a_field_is_ignored() {
        with_ctx(|ctx| {
            let mut view = EditViewComponent::new(
                "/secret/test".to_string(),
                serde_json::from_value(json!({"host": "db"})).unwrap(),
            );

            // "d" then "y" would delete a field if this ran as keys
            view.handle_paste("dy", ctx).unwrap();
            assert_eq!(view.fields.len(), 1);
            assert!(!view.has_delete_confirmation());
            assert!(!view.has_changes);
        });
    }

    #[test]
    fn long_field_lists_scroll_with_the_cursor() {
        // Regression: the list did not scroll, so fields below the bottom
        // edge could not be reached.
        with_ctx(|ctx| {
            let fields: serde_json::Map<String, serde_json::Value> = (0..30)
                .map(|i| (format!("field_{i:02}"), json!("v")))
                .collect();
            let mut view = EditViewComponent::new(
                "/secret/test".to_string(),
                serde_json::from_value(serde_json::Value::Object(fields)).unwrap(),
            );

            let screen = rendered(&mut view, 60, 12);
            assert!(screen.contains("field_00"), "{screen}");
            assert!(!screen.contains("field_29"), "{screen}");

            view.handle_key(key(KeyCode::End), ctx).unwrap();
            let screen = rendered(&mut view, 60, 12);
            assert!(screen.contains("field_29"), "{screen}");
            assert!(!screen.contains("field_00"), "{screen}");
        });
    }

    #[test]
    fn restore_note_is_on_screen_and_counts_as_unsaved() {
        let mut view = EditViewComponent::new(
            "/secret/test".to_string(),
            serde_json::from_value(json!({"host": "db"})).unwrap(),
        )
        .with_restore_note(3, 7);

        let screen = rendered(&mut view, 90, 10);
        assert!(screen.contains("Copy of v3"), "{screen}");
        assert!(screen.contains("replaces the current v7"), "{screen}");

        // Regression: at the width of an 80-column terminal's content pane
        // the end of the sentence, with the version it replaces, was cut off
        let narrow = rendered(&mut view, 50, 10);
        assert!(narrow.contains("replaces the current v7"), "{narrow}");
        assert!(screen.contains("unsaved"), "{screen}");
    }

    #[test]
    fn field_delete_dialog_fits_a_small_pane() {
        with_ctx(|ctx| {
            let mut view = EditViewComponent::new(
                "/secret/test".to_string(),
                serde_json::from_value(json!({"host": "db", "port": "1"})).unwrap(),
            );

            view.handle_key(key(KeyCode::Char('d')), ctx).unwrap();
            let screen = rendered(&mut view, 52, 12);
            assert!(screen.contains("Delete field"), "{screen}");
            assert!(screen.contains("host"), "{screen}");
            assert!(screen.contains("y delete"), "{screen}");

            view.handle_key(key(KeyCode::Char('y')), ctx).unwrap();
            assert_eq!(view.fields.len(), 1);
            assert!(view.has_changes);
        });
    }

    #[test]
    fn only_y_confirms_a_field_delete() {
        // Regression: Enter deleted the field too, while the dialog offered
        // only `y`.
        with_ctx(|ctx| {
            let mut view = EditViewComponent::new(
                "/secret/test".to_string(),
                serde_json::from_value(json!({"host": "db", "port": "1"})).unwrap(),
            );

            view.handle_key(key(KeyCode::Char('d')), ctx).unwrap();
            view.handle_key(key(KeyCode::Enter), ctx).unwrap();

            assert_eq!(view.fields.len(), 2);
            assert!(view.has_delete_confirmation(), "the question is still open");
        });
    }
}
