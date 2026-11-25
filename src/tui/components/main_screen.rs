use crate::app::navigation::child_path;
use crate::tui::action::{Action, FocusedPane};
use crate::tui::component::Component;
use crate::tui::components::popup::{
    border_color, centered, display_width, spinner_frame, truncate, wrap, wrap_words,
};
use crate::tui::components::{DetailViewComponent, EditViewComponent, NavigationPane};
use crate::tui::context::{ComponentContext, RenderContext};
use crate::tui::keymap::{self, Context};
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
use ratatui_textarea::{Input, Key, TextArea};
use std::collections::VecDeque;

/// A secret waiting for its delete to be confirmed
struct DeleteTarget {
    path: String,
    /// How many versions will be destroyed, when the secret is on screen and
    /// its history is known
    versions: Option<usize>,
}

/// Why a name typed into the new-secret prompt was refused
#[derive(Debug, Clone, PartialEq, Eq)]
enum NameError {
    Empty,
    /// The folder on screen already has this name
    Taken(String),
}

impl NameError {
    /// The message, from the most telling wording to the shortest. The
    /// footer is one line, so it shows the first one that fits whole.
    fn wordings(&self) -> Vec<String> {
        match self {
            NameError::Empty => vec!["a name is needed".to_string(), "name needed".to_string()],
            NameError::Taken(name) => vec![
                format!("'{name}' already exists"),
                "that name already exists".to_string(),
                "name taken".to_string(),
            ],
        }
    }
}

/// What is waiting for the user to agree that unsaved changes may be lost
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Discard {
    /// Esc in the editor
    CloseEditor,
    /// `q` while an editor with changes is open
    Quit,
}

/// Main screen component with split view (navigation + content)
///
/// This is the primary screen of VltView, showing:
/// - Header line: where you are, and which Vault this is
/// - Left pane: NavigationPane for browsing paths/secrets
/// - Right pane: whatever the navigation cursor is on. A secret is shown in
///   the DetailView (or the EditView while it is edited); a secret that is
///   still loading is a placeholder, never the previous secret.
/// - Footer line: status, prompts, and the keys that work right now
pub struct MainScreen {
    /// Navigation pane (left side)
    navigation_pane: NavigationPane,

    /// Detail view component (right side, when viewing)
    detail_view: Option<DetailViewComponent>,

    /// Edit view component (right side, when editing)
    edit_view: Option<EditViewComponent>,

    /// Which pane currently has focus
    focused_pane: FocusedPane,

    /// Name prompt for a new secret, while it is open
    create_input: Option<TextArea<'static>>,

    /// Why the typed name was refused
    create_error: Option<NameError>,

    /// Status message (temporary)
    status_message: Option<String>,

    /// Why the selected secret, or the version asked for, is not shown.
    /// It takes the secret's place in the content pane and goes when the
    /// cursor moves on.
    error_message: Option<String>,

    /// Errors about anything else (a folder that could not be opened, a
    /// delete or a copy that failed). They are shown one at a time over the
    /// screen, never inside an editor or a secret they have nothing to do
    /// with, and each names what it is about.
    notices: VecDeque<String>,

    /// Whether a request to Vault is in flight
    is_loading: bool,

    /// Delete confirmation, while it is open
    delete_confirmation: Option<DeleteTarget>,

    /// Question about unsaved changes, while it is open
    discard_prompt: Option<Discard>,

    /// Selected secret that is not on screen yet
    loading_secret: Option<String>,

    /// Secret the user asked to go into before it had loaded; focus moves to
    /// the content pane when it arrives
    focus_on_load: Option<String>,
}

fn name_prompt() -> TextArea<'static> {
    let mut input = TextArea::default();
    input.set_cursor_style(Style::default().bg(Color::Green).fg(Color::Black));
    input.set_cursor_line_style(Style::default());
    input
}

impl MainScreen {
    /// Create a new main screen
    pub fn new() -> Self {
        Self {
            navigation_pane: NavigationPane::new(),
            detail_view: None,
            edit_view: None,
            focused_pane: FocusedPane::Navigation,
            create_input: None,
            create_error: None,
            status_message: None,
            error_message: None,
            notices: VecDeque::new(),
            is_loading: false,
            delete_confirmation: None,
            discard_prompt: None,
            loading_secret: None,
            focus_on_load: None,
        }
    }

    /// Whether the content pane has something that can take focus
    fn has_content(&self) -> bool {
        self.detail_view.is_some() || self.edit_view.is_some()
    }

    /// Whether an editor is open with changes that have not been saved
    fn has_unsaved_edit(&self) -> bool {
        self.edit_view
            .as_ref()
            .is_some_and(|edit| edit.has_unsaved_changes())
    }

    /// Close the editor; what was underneath is shown again
    fn close_editor(&mut self) {
        self.edit_view = None;
        self.focused_pane = if self.detail_view.is_some() {
            FocusedPane::Content
        } else {
            FocusedPane::Navigation
        };
    }

    /// Keys while the question about unsaved changes is open
    fn handle_discard_prompt(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Char('y') => match self.discard_prompt.take()? {
                Discard::CloseEditor => {
                    self.close_editor();
                    None
                }
                Discard::Quit => Some(Action::QuitNow),
            },
            KeyCode::Char('n') | KeyCode::Esc => {
                self.discard_prompt = None;
                None
            }
            _ => None,
        }
    }

    /// The keys that work right now, for the footer
    fn footer_contexts(&self) -> Vec<Context> {
        if !self.notices.is_empty() {
            return vec![Context::Notice];
        }
        match self.discard_prompt {
            Some(Discard::CloseEditor) => return vec![Context::DiscardConfirm],
            Some(Discard::Quit) => return vec![Context::QuitConfirm],
            None => {}
        }
        if self.delete_confirmation.is_some() {
            return vec![Context::DeleteConfirm];
        }
        if self.create_input.is_some() {
            return vec![Context::CreatePrompt];
        }

        // While the filter is typed it has every key, Esc included
        if self.focused_pane == FocusedPane::Navigation && self.navigation_pane.is_filtering() {
            return vec![Context::Filter];
        }

        let mut contexts = match self.focused_pane {
            FocusedPane::Navigation if !self.navigation_pane.filter_text().is_empty() => {
                vec![Context::Filtered, Context::Navigation, Context::Global]
            }
            FocusedPane::Navigation => vec![Context::Navigation, Context::Global],
            FocusedPane::Content => {
                if let Some(edit) = &self.edit_view {
                    if edit.has_delete_confirmation() {
                        vec![Context::FieldDeleteConfirm]
                    } else if edit.is_editing() {
                        vec![Context::EditField]
                    } else {
                        vec![Context::Edit]
                    }
                } else if let Some(detail) = &self.detail_view {
                    match detail.context() {
                        Context::ValueViewer => vec![Context::ValueViewer],
                        context => vec![context, Context::Global],
                    }
                } else {
                    vec![Context::Global]
                }
            }
        };

        // An error in place of the secret is dismissed with Esc, before
        // Esc means anything else
        if self.error_message.is_some()
            && self.edit_view.is_none()
            && contexts != [Context::ValueViewer]
        {
            contexts.insert(0, Context::LoadError);
        }
        contexts
    }

    /// Handle create secret name input
    fn handle_create_secret_input(
        &mut self,
        key: KeyEvent,
        ctx: &ComponentContext,
    ) -> Result<Option<Action>> {
        let Some(input) = &mut self.create_input else {
            return Ok(None);
        };

        match (key.code, key.modifiers) {
            (KeyCode::Enter, _) => {
                let name = input.lines().join("").trim().trim_matches('/').to_string();
                if name.is_empty() {
                    self.create_error = Some(NameError::Empty);
                    return Ok(None);
                }

                // An existing name is not a new secret: saving the empty
                // editor over it would replace its current version
                if ctx.app.state().list_items().contains(&name) {
                    self.create_error = Some(NameError::Taken(name));
                    return Ok(None);
                }

                let path = child_path(&ctx.app.state().current_path(), &name);

                self.create_input = None;
                self.create_error = None;

                Ok(Some(Action::CreateSecret { name: path }))
            }
            (KeyCode::Esc, _) => {
                self.create_input = None;
                self.create_error = None;
                Ok(None)
            }
            // A name is one line
            (KeyCode::Char('m'), KeyModifiers::CONTROL)
            | (KeyCode::Char('j'), KeyModifiers::CONTROL) => Ok(None),
            _ => {
                self.create_error = None;
                input.input(prompt_input(key));
                Ok(None)
            }
        }
    }

    /// Keys while an error message is over the screen: it is closed, and
    /// nothing else happens
    fn handle_notice(&mut self, key: KeyEvent) {
        if matches!(key.code, KeyCode::Esc | KeyCode::Enter) {
            self.notices.pop_front();
        }
    }

    /// Handle delete confirmation dialog
    fn handle_delete_confirmation(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        match key.code {
            // Only `y` deletes. Enter is too easy to press by habit for
            // something that destroys every version.
            KeyCode::Char('y') => Ok(self
                .delete_confirmation
                .take()
                .map(|target| Action::DeleteSecret { path: target.path })),
            KeyCode::Char('n') | KeyCode::Esc => {
                self.delete_confirmation = None;
                Ok(None)
            }
            _ => Ok(None), // Ignore other keys
        }
    }

    /// Render the header: where you are, and which Vault this is
    fn render_header(&self, frame: &mut Frame, area: Rect, ctx: &RenderContext) {
        let current_path = ctx.app.state().current_path();
        let vault = ctx.app.vault_client();

        let mut right = vec![];
        if self.is_loading {
            right.push(Span::styled(
                format!("{} ", spinner_frame()),
                Style::default().fg(Color::Yellow),
            ));
        }
        right.push(Span::styled(
            vault.base_url().to_string(),
            Style::default().fg(Color::Gray),
        ));
        if let Some(namespace) = vault.namespace() {
            right.push(Span::styled(
                format!("  ns: {namespace}"),
                Style::default().fg(Color::Magenta),
            ));
        }
        right.push(Span::raw(" "));
        let right = Line::from(right);

        // The path keeps its room; the address gets what is left
        let right_width = (right.width() as u16).min(area.width / 2);
        let chunks =
            Layout::horizontal([Constraint::Min(0), Constraint::Length(right_width)]).split(area);

        let path_width = chunks[0].width.saturating_sub(12) as usize;
        let left = Line::from(vec![
            Span::styled(
                " VltView",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" │ ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                truncate(&current_path, path_width),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
        ]);

        frame.render_widget(Paragraph::new(left), chunks[0]);
        frame.render_widget(Paragraph::new(right).alignment(Alignment::Right), chunks[1]);
    }

    /// Render the footer: the name prompt, a status message, or key hints
    fn render_footer(&self, frame: &mut Frame, area: Rect) {
        if let Some(input) = &self.create_input {
            let label = " New secret name: ";
            // The name keeps at least 8 columns. What is said next to it
            // has to fit in the rest whole, or it is left out: an error cut
            // in the middle says nothing.
            let room = area.width.saturating_sub(label.len() as u16 + 8) as usize;
            let hint = match &self.create_error {
                Some(error) => error
                    .wordings()
                    .into_iter()
                    .map(|wording| format!(" {wording} "))
                    .find(|wording| display_width(wording) <= room)
                    .map(|wording| Span::styled(wording, Style::default().fg(Color::Red))),
                None => Some(" Enter create  Esc cancel ")
                    .filter(|hint| hint.len() <= room)
                    .map(|hint| Span::styled(hint, Style::default().fg(Color::Gray))),
            }
            .unwrap_or_default();
            let hint_width = hint.width() as u16;
            let chunks = Layout::horizontal([
                Constraint::Length(label.len() as u16),
                Constraint::Min(8),
                Constraint::Length(hint_width),
            ])
            .split(area);

            frame.render_widget(
                Paragraph::new(Span::styled(
                    label,
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )),
                chunks[0],
            );
            frame.render_widget(input, chunks[1]);
            frame.render_widget(Paragraph::new(hint), chunks[2]);
            return;
        }

        let line = if let Some(status) = &self.status_message {
            Line::from(Span::styled(
                format!(" {status}"),
                Style::default().fg(Color::Green),
            ))
        } else {
            let mut line =
                keymap::footer_line(&self.footer_contexts(), area.width.saturating_sub(2));
            line.spans.insert(0, Span::raw(" "));
            line
        };

        frame.render_widget(Paragraph::new(line), area);
    }

    /// Render the content pane (right side)
    fn render_content_pane(&mut self, frame: &mut Frame, area: Rect, ctx: &RenderContext) {
        let border = Style::default().fg(border_color(ctx.is_focused));

        // Priority: edit view > error > detail view > loading > empty.
        // An edit in progress is never hidden; its errors are shown inside.
        if let Some(view) = &mut self.edit_view {
            view.draw(frame, area, ctx);
        } else if let Some(error) = &self.error_message {
            let mut text = vec![
                Line::from(""),
                Line::from(Span::styled(
                    "Error",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
            ];
            text.push(Line::from(Span::styled(
                error.as_str(),
                Style::default().fg(Color::Red),
            )));
            let error_widget = Paragraph::new(text)
                .alignment(Alignment::Center)
                .wrap(Wrap { trim: true })
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Red)),
                );
            frame.render_widget(error_widget, area);
        } else if let Some(view) = &mut self.detail_view {
            view.draw(frame, area, ctx);
        } else if let Some(path) = &self.loading_secret {
            // The selected secret is on its way. Showing the previous one
            // here would put one secret's values under another one's name.
            let title_width = area.width.saturating_sub(4) as usize;
            let loading = Paragraph::new(Line::from(vec![
                Span::styled(
                    format!(" {} ", spinner_frame()),
                    Style::default().fg(Color::Yellow),
                ),
                Span::styled("Loading…", Style::default().fg(Color::Gray)),
            ]))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(border)
                    .title(Span::styled(
                        format!(" {} ", truncate(path, title_width)),
                        Style::default().fg(Color::Green),
                    )),
            );
            frame.render_widget(loading, area);
        } else {
            let empty = Paragraph::new(vec![
                Line::from(""),
                Line::from(Span::styled(
                    "Move the cursor to a secret to see it here",
                    Style::default().fg(Color::DarkGray),
                )),
            ])
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::ALL).border_style(border));
            frame.render_widget(empty, area);
        }
    }

    /// Render an error message over the screen.
    ///
    /// The width is fixed and the message is wrapped, so all of it is on
    /// screen whatever the terminal size.
    fn render_notice(frame: &mut Frame, area: Rect, message: &str, waiting: usize) {
        let width = 62;
        let inner_width = (width.min(area.width) as usize).saturating_sub(4);

        let mut text = vec![Line::from("")];
        text.extend(
            wrap_words(message, inner_width)
                .into_iter()
                .map(|line| Line::from(Span::styled(line, Style::default().fg(Color::White)))),
        );
        text.push(Line::from(""));
        let mut close = vec![
            Span::styled(
                "Esc",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" close"),
        ];
        if waiting > 0 {
            close.push(Span::styled(
                format!("   {waiting} more"),
                Style::default().fg(Color::DarkGray),
            ));
        }
        text.push(Line::from(close));

        let popup = centered(width, text.len() as u16 + 2, area);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Red))
            .title(Span::styled(
                " Error ",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ));

        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(text)
                .alignment(Alignment::Center)
                .block(block),
            popup,
        );
    }

    /// Render the question about unsaved changes. Fixed size, like the
    /// other dialogs, so all of it is on screen at 80x24.
    fn render_discard_prompt(frame: &mut Frame, area: Rect, prompt: Discard, path: &str) {
        let width = 62;
        let inner_width = (width.min(area.width) as usize).saturating_sub(4);
        let path_lines = wrap(path, inner_width);
        let path_lines = &path_lines[..path_lines.len().min(3)];

        let (consequence, yes, no) = match prompt {
            Discard::CloseEditor => (
                "Closing the editor loses them.",
                " discard   ",
                " keep editing",
            ),
            Discard::Quit => ("Quitting loses them.", " quit   ", " stay"),
        };
        let key_style = Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD);

        let mut text = vec![Line::from("")];
        text.extend(path_lines.iter().map(|line| {
            Line::from(Span::styled(
                line.clone(),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ))
        }));
        text.extend([
            Line::from(""),
            Line::from("The changes to this secret have not been saved."),
            Line::from(Span::styled(
                consequence,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(vec![
                Span::styled("y", key_style),
                Span::raw(yes),
                Span::styled("n/Esc", key_style),
                Span::raw(no),
            ]),
        ]);

        let popup = centered(width, text.len() as u16 + 2, area);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .title(Span::styled(
                " Unsaved changes ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ));

        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(text)
                .alignment(Alignment::Center)
                .block(block),
            popup,
        );
    }

    /// Render delete confirmation dialog.
    ///
    /// The size is fixed, so the path and the warning are on screen whatever
    /// the terminal size.
    fn render_delete_confirmation(frame: &mut Frame, area: Rect, target: &DeleteTarget) {
        let width = 62;
        let inner_width = (width.min(area.width) as usize).saturating_sub(4);
        let path_lines = wrap(&target.path, inner_width);
        let path_lines = &path_lines[..path_lines.len().min(3)];

        let what = match target.versions {
            Some(1) => "Its only version and its metadata are destroyed.".to_string(),
            Some(count) => format!("All {count} versions and the metadata are destroyed."),
            None => "Every version and the metadata are destroyed.".to_string(),
        };

        let key_style = Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD);

        let mut text = vec![Line::from("")];
        text.extend(path_lines.iter().map(|line| {
            Line::from(Span::styled(
                line.clone(),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ))
        }));
        text.extend([
            Line::from(""),
            Line::from(Span::styled(what, Style::default().fg(Color::Red))),
            Line::from(Span::styled(
                "This cannot be undone.",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(vec![
                Span::styled("y", key_style),
                Span::raw(" delete   "),
                Span::styled("n/Esc", key_style),
                Span::raw(" cancel"),
            ]),
        ]);

        let popup = centered(width, text.len() as u16 + 2, area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Red))
            .title(Span::styled(
                " Delete secret ",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ));

        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(text)
                .alignment(Alignment::Center)
                .block(block),
            popup,
        );
    }
}

/// Convert a key event for the single-line name prompt
fn prompt_input(key: KeyEvent) -> Input {
    let code = match key.code {
        KeyCode::Char(c) => Key::Char(c),
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Delete => Key::Delete,
        KeyCode::Left => Key::Left,
        KeyCode::Right => Key::Right,
        KeyCode::Home => Key::Home,
        KeyCode::End => Key::End,
        _ => Key::Null,
    };

    Input {
        key: code,
        ctrl: key.modifiers.contains(KeyModifiers::CONTROL),
        alt: key.modifiers.contains(KeyModifiers::ALT),
        shift: key.modifiers.contains(KeyModifiers::SHIFT),
    }
}

impl Default for MainScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for MainScreen {
    fn init(&mut self, ctx: &mut ComponentContext) -> Result<()> {
        // Initialize navigation pane
        self.navigation_pane.init(ctx)?;
        Ok(())
    }

    fn handle_key(&mut self, key: KeyEvent, ctx: &ComponentContext) -> Result<Option<Action>> {
        // Clear status message on any key
        self.status_message = None;

        // An error message is read and closed before anything else
        if !self.notices.is_empty() {
            self.handle_notice(key);
            return Ok(None);
        }

        // So is the question about unsaved changes
        if self.discard_prompt.is_some() {
            return Ok(self.handle_discard_prompt(key));
        }

        // Handle delete confirmation first
        if self.delete_confirmation.is_some() {
            return self.handle_delete_confirmation(key);
        }

        // Handle create secret input mode
        if self.create_input.is_some() {
            return self.handle_create_secret_input(key, ctx);
        }

        // While the filter is being typed, every key belongs to it. Without
        // this, letters such as q, p and r fire the global shortcuts below.
        if self.focused_pane == FocusedPane::Navigation && self.navigation_pane.captures_input() {
            return self.navigation_pane.handle_key(key, ctx);
        }

        if self.focused_pane == FocusedPane::Content {
            // The editor owns the keyboard while it has focus. Global
            // shortcuts stay off: `q` here must not quit with an edit open.
            if let Some(edit_view) = &mut self.edit_view {
                if edit_view.captures_input() {
                    return edit_view.handle_key(key, ctx);
                }
                return match (key.code, key.modifiers) {
                    (KeyCode::Tab, KeyModifiers::NONE)
                    | (KeyCode::Left, KeyModifiers::NONE)
                    | (KeyCode::Char('h'), KeyModifiers::NONE) => {
                        self.focused_pane = FocusedPane::Navigation;
                        Ok(None)
                    }
                    (KeyCode::Char('?'), KeyModifiers::NONE) => Ok(Some(Action::ShowHelp)),
                    _ => edit_view.handle_key(key, ctx),
                };
            }

            // So does the value viewer
            if let Some(detail_view) = &mut self.detail_view
                && detail_view.captures_input()
            {
                return detail_view.handle_key(key, ctx);
            }
        }

        // Clear errors with Esc
        if key.code == KeyCode::Esc && self.error_message.is_some() {
            self.error_message = None;
            return Ok(None);
        }

        // Global keys (work regardless of focused pane)
        match (key.code, key.modifiers) {
            // Quit
            (KeyCode::Char('q'), KeyModifiers::NONE) => {
                return Ok(Some(Action::Quit));
            }
            // Help
            (KeyCode::Char('?'), KeyModifiers::NONE) => {
                return Ok(Some(Action::ShowHelp));
            }
            // Logs
            (KeyCode::Char('~'), KeyModifiers::NONE) => {
                return Ok(Some(Action::ShowLogScreen));
            }
            // Policy
            (KeyCode::Char('p'), KeyModifiers::NONE) => {
                return Ok(Some(Action::ShowPolicyScreen));
            }
            // Refresh
            (KeyCode::Char('r'), KeyModifiers::NONE) => {
                return Ok(Some(Action::Refresh));
            }
            // Switch focus with Tab
            (KeyCode::Tab, KeyModifiers::NONE) => {
                match self.focused_pane {
                    FocusedPane::Navigation if self.has_content() => {
                        self.focused_pane = FocusedPane::Content;
                    }
                    FocusedPane::Navigation => {
                        self.status_message = Some("Select a secret first".to_string());
                    }
                    FocusedPane::Content => self.focused_pane = FocusedPane::Navigation,
                }
                return Ok(None);
            }
            // Switch focus with arrow keys (left/right)
            (KeyCode::Left, KeyModifiers::NONE) | (KeyCode::Char('h'), KeyModifiers::NONE)
                if self.focused_pane == FocusedPane::Content =>
            {
                self.focused_pane = FocusedPane::Navigation;
                return Ok(None);
            }
            (KeyCode::Right, KeyModifiers::NONE) | (KeyCode::Char('l'), KeyModifiers::NONE)
                if self.focused_pane == FocusedPane::Navigation && self.has_content() =>
            {
                self.focused_pane = FocusedPane::Content;
                return Ok(None);
            }
            _ => {}
        }

        // Route to focused pane
        match self.focused_pane {
            FocusedPane::Navigation => self.navigation_pane.handle_key(key, ctx),
            FocusedPane::Content => match &mut self.detail_view {
                Some(view) => view.handle_key(key, ctx),
                None => Ok(None),
            },
        }
    }

    fn handle_paste(&mut self, text: &str, ctx: &ComponentContext) -> Result<Option<Action>> {
        // Pasted text goes into the text input that has focus, if there is
        // one. Anywhere else it is dropped: it must never run as commands.
        if self.delete_confirmation.is_some()
            || self.discard_prompt.is_some()
            || !self.notices.is_empty()
        {
            return Ok(None);
        }

        if let Some(input) = &mut self.create_input {
            let line: String = text
                .lines()
                .next()
                .unwrap_or_default()
                .chars()
                .filter(|c| !c.is_control())
                .collect();
            input.insert_str(line);
            self.create_error = None;
            return Ok(None);
        }

        match self.focused_pane {
            FocusedPane::Navigation => self.navigation_pane.handle_paste(text, ctx),
            FocusedPane::Content => match &mut self.edit_view {
                Some(edit_view) => edit_view.handle_paste(text, ctx),
                None => Ok(None),
            },
        }
    }

    fn update(&mut self, action: Action, ctx: &mut ComponentContext) -> Result<Option<Action>> {
        match action {
            // Focus management
            Action::FocusPane { pane } => {
                self.focused_pane = match pane {
                    FocusedPane::Content if !self.has_content() => FocusedPane::Navigation,
                    pane => pane,
                };
            }

            // The cursor is on another item: the content pane follows it
            Action::SelectionChanged { secret, .. } => {
                // Moving on cancels a pending Enter; the list catching up
                // with the secret that is awaited does not
                if self.focus_on_load != secret {
                    self.focus_on_load = None;
                }

                let already_shown = match (&secret, &self.detail_view) {
                    (Some(path), Some(view)) => view.path() == path,
                    _ => false,
                };
                if !already_shown {
                    self.detail_view = None;
                    self.loading_secret = secret;
                    // An error belongs to what was selected when it happened
                    self.error_message = None;
                    // Nothing left to have focus on
                    if self.edit_view.is_none() {
                        self.focused_pane = FocusedPane::Navigation;
                    }
                } else {
                    self.loading_secret = None;
                }
            }

            // Enter on a secret: go into it, as soon as it is there
            Action::OpenSecret { path } => {
                let shown = self
                    .detail_view
                    .as_ref()
                    .is_some_and(|view| view.path() == path);
                if shown || self.edit_view.is_some() {
                    self.focused_pane = FocusedPane::Content;
                } else {
                    self.loading_secret = Some(path.clone());
                    self.focus_on_load = Some(path);
                }
            }

            Action::SecretLoaded {
                path,
                data,
                metadata,
            } => {
                // Create detail view with loaded data
                let mut detail_view = DetailViewComponent::new(data, path.clone());

                if let Some(meta) = metadata {
                    detail_view.set_metadata(
                        meta.created_time.clone(),
                        meta.updated_time.clone(),
                        meta.current_version,
                    );

                    // Convert action::VersionInfo to detail_view::VersionInfo
                    use crate::tui::components::VersionInfo as DetailVersionInfo;
                    let versions: Vec<DetailVersionInfo> = meta
                        .versions
                        .iter()
                        .map(|v| DetailVersionInfo {
                            version: v.version,
                            created_time: v.created_time.clone(),
                            is_deleted: v.deleted,
                            is_current: v.version == meta.current_version,
                        })
                        .collect();

                    detail_view.set_versions(versions);
                }

                // The same secret reloaded (after a save): keep the cursor
                // and what was revealed
                if let Some(previous) = &self.detail_view
                    && previous.path() == path
                {
                    detail_view.adopt_view_state(previous);
                }

                // The edit view is deliberately left alone: a preview that
                // completes late must not throw away an edit in progress.
                // It is closed by `SecretSaved`, or by the user.
                self.detail_view = Some(detail_view);
                self.error_message = None;
                if self.loading_secret.as_deref() == Some(path.as_str()) {
                    self.loading_secret = None;
                }
                if self.focus_on_load.as_deref() == Some(path.as_str()) {
                    self.focus_on_load = None;
                    self.focused_pane = FocusedPane::Content;
                }
            }

            Action::SecretLoadFailed { path, message } => {
                if self.loading_secret.as_deref() == Some(path.as_str()) {
                    self.loading_secret = None;
                }
                self.focus_on_load = None;
                self.error_message = Some(message);
            }

            // The write was confirmed by Vault: the edit is done
            Action::SecretSaved { path } => {
                if self
                    .edit_view
                    .as_ref()
                    .is_some_and(|view| view.path() == path)
                {
                    self.edit_view = None;
                    if self.detail_view.is_none() {
                        // A new secret: go into it once it has been read back
                        self.focus_on_load = Some(path.clone());
                        self.focused_pane = FocusedPane::Navigation;
                    }
                }
                // The list puts its cursor on it after the reload
                self.navigation_pane
                    .update(Action::SecretSaved { path }, ctx)?;
            }

            // The secret is gone: it must not stay on screen, where it could
            // be edited and saved back
            Action::SecretDeleted { path } => {
                if self
                    .detail_view
                    .as_ref()
                    .is_some_and(|view| view.path() == path)
                {
                    self.detail_view = None;
                }
                if self
                    .edit_view
                    .as_ref()
                    .is_some_and(|view| view.path() == path)
                {
                    self.edit_view = None;
                }
                if self.loading_secret.as_deref() == Some(path.as_str()) {
                    self.loading_secret = None;
                }
                if !self.has_content() {
                    self.focused_pane = FocusedPane::Navigation;
                }
            }

            // Edit secret
            Action::EditSecret => {
                if let Some(detail_view) = &self.detail_view {
                    let path = detail_view.path().to_string();
                    let mut edit_view = EditViewComponent::new(path, detail_view.secret_data());
                    // Editing an old version on screen is a restore
                    if let Some((viewing, current)) = detail_view.old_version() {
                        edit_view = edit_view.with_restore_note(viewing, current);
                    }
                    self.edit_view = Some(edit_view);
                    // Keep detail_view so we can restore it on cancel
                    self.focused_pane = FocusedPane::Content;
                }
            }

            // Esc in the editor's field list. Changes are not dropped by
            // one key: with unsaved changes the user is asked first.
            Action::CancelEdit => {
                if self.has_unsaved_edit() {
                    self.discard_prompt = Some(Discard::CloseEditor);
                } else if self.edit_view.is_some() {
                    self.close_editor();
                }
            }

            // `q`. The same goes for quitting with an edit open.
            Action::Quit => {
                if self.has_unsaved_edit() {
                    self.discard_prompt = Some(Discard::Quit);
                } else {
                    return Ok(Some(Action::QuitNow));
                }
            }

            // Secret version loaded successfully
            Action::SecretVersionLoaded {
                path,
                version,
                data,
            } => {
                // Ignore a version that belongs to a secret the user has
                // since left: showing it here would let it be saved over
                // the secret now on screen.
                // So is a version the user asked for before picking another
                // one: its result may arrive last.
                if let Some(detail_view) = &mut self.detail_view
                    && detail_view.path() == path
                    && detail_view.version_arrived(version)
                {
                    detail_view.load_version(version, data);
                }
            }

            // A version that is no longer awaited failed to load: of no
            // interest. One that is awaited takes the pane, like a secret
            // that could not be loaded.
            Action::VersionLoadFailed {
                path,
                version,
                message,
            } => {
                if let Some(detail_view) = &mut self.detail_view
                    && detail_view.path() == path
                    && detail_view.version_arrived(version)
                {
                    self.error_message = Some(message);
                }
            }

            // Version loaded for editing (create new from old version)
            Action::VersionLoadedForEdit {
                path,
                version,
                data,
            } => {
                // Only open the editor if the user is still on that secret,
                // still waits for this version, and has not started another
                // edit in the meantime.
                if let Some(detail_view) = &mut self.detail_view
                    && detail_view.path() == path
                    && detail_view.version_arrived(version)
                    && self.edit_view.is_none()
                {
                    let mut edit_view = EditViewComponent::new(path, data);
                    if let Some(current) = detail_view.current_version()
                        && current != version
                    {
                        edit_view = edit_view.with_restore_note(version, current);
                    }
                    self.edit_view = Some(edit_view);
                    self.focused_pane = FocusedPane::Content;
                }
            }

            // Create secret
            Action::CreateSecret { name } => {
                // The secret under the cursor stays underneath, as it does
                // for an edit: the cursor has not moved, so Esc returns to it
                self.edit_view = Some(EditViewComponent::new_secret(name));
                self.focused_pane = FocusedPane::Content;
            }

            Action::ShowCreateSecretInput => {
                // A new secret takes the editor's place, so it must not be
                // a way to lose an edit without being asked
                if self.has_unsaved_edit() {
                    self.status_message = Some("Save or close the open edit first".to_string());
                } else {
                    self.create_input = Some(name_prompt());
                    self.create_error = None;
                }
            }

            // Delete confirmation
            Action::ShowDeleteConfirmation { item_name } => {
                let versions = self
                    .detail_view
                    .as_ref()
                    .filter(|view| view.path() == item_name)
                    .and_then(|view| view.version_count());
                self.delete_confirmation = Some(DeleteTarget {
                    path: item_name,
                    versions,
                });
            }

            Action::HideConfirmation => {
                self.delete_confirmation = None;
            }

            // Status/error messages
            Action::SetStatus { message } => {
                self.status_message = Some(message);
            }

            // A save that failed is shown inside the editor it came from,
            // so the fields stay on screen. If that editor is gone, or shows
            // another secret, the failure is still said, but over the screen.
            Action::SaveFailed { path, message } => match &mut self.edit_view {
                Some(edit_view) if edit_view.path() == path => edit_view.set_error(Some(message)),
                _ => self.notices.push_back(message),
            },

            // Errors that are not about what the content pane shows
            Action::DeleteFailed { message }
            | Action::FolderListFailed { message, .. }
            | Action::CopyFailed { message } => {
                self.notices.push_back(message);
            }

            Action::SetLoading { loading } => {
                self.is_loading = loading;
            }

            // A folder was loaded: the list follows, and tells us if that
            // moved the cursor to another item
            Action::StateUpdated => {
                return self.navigation_pane.update(Action::StateUpdated, ctx);
            }

            _ => {}
        }

        Ok(None)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect, ctx: &RenderContext) {
        // One line each for the header and the footer; the panes get the rest
        let main_chunks = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area);

        self.render_header(frame, main_chunks[0], ctx);

        // Split content area into two columns
        let content_chunks = Layout::horizontal([
            Constraint::Percentage(35), // Left: Navigation
            Constraint::Percentage(65), // Right: Content
        ])
        .split(main_chunks[1]);

        // Render navigation pane (left)
        let nav_ctx = RenderContext::new(ctx.app, self.focused_pane == FocusedPane::Navigation);
        self.navigation_pane
            .draw(frame, content_chunks[0], &nav_ctx);

        // Render content pane (right)
        let content_ctx = RenderContext::new(ctx.app, self.focused_pane == FocusedPane::Content);
        self.render_content_pane(frame, content_chunks[1], &content_ctx);

        self.render_footer(frame, main_chunks[2]);

        // Render delete confirmation dialog if active
        if let Some(target) = &self.delete_confirmation {
            Self::render_delete_confirmation(frame, area, target);
        }

        if let (Some(prompt), Some(edit)) = (self.discard_prompt, &self.edit_view) {
            Self::render_discard_prompt(frame, area, prompt, edit.path());
        }

        // An error message is on top of everything
        if let Some(message) = self.notices.front() {
            Self::render_notice(frame, area, message, self.notices.len() - 1);
        }
    }

    fn captures_input(&self) -> bool {
        // Capture input in create secret mode, in the delete confirmation
        // and while an error message is shown
        self.create_input.is_some()
            || self.delete_confirmation.is_some()
            || self.discard_prompt.is_some()
            || !self.notices.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use crate::config::Config;
    use crate::tui::action::{SecretMetadata, SelectionSource, VersionInfo};
    use crate::vault::SecretData;
    use ratatui::{Terminal, backend::TestBackend};
    use serde_json::json;
    use tokio::sync::mpsc::unbounded_channel;

    fn test_app() -> App {
        let config = Config {
            vault_addr: "http://127.0.0.1:1".to_string(),
            vault_token: "test-token".to_string().into(),
            ..Config::default()
        };
        App::new(config, "/".to_string()).unwrap()
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn secret(value: serde_json::Value) -> SecretData {
        serde_json::from_value(value).unwrap()
    }

    fn loaded(path: &str, value: serde_json::Value) -> Action {
        Action::SecretLoaded {
            path: path.to_string(),
            data: secret(value),
            metadata: None,
        }
    }

    fn loaded_with_versions(path: &str, versions: u64) -> Action {
        Action::SecretLoaded {
            path: path.to_string(),
            data: secret(json!({"k": "v"})),
            metadata: Some(SecretMetadata {
                created_time: "2026-01-20T14:47:07.5Z".to_string(),
                updated_time: "2026-02-01T09:00:00.5Z".to_string(),
                current_version: versions,
                versions: (1..=versions)
                    .rev()
                    .map(|version| VersionInfo {
                        version,
                        created_time: "2026-01-20T14:47:07.5Z".to_string(),
                        deleted: false,
                    })
                    .collect(),
            }),
        }
    }

    fn selected(path: Option<&str>) -> Action {
        Action::SelectionChanged {
            secret: path.map(str::to_string),
            source: SelectionSource::Cursor,
        }
    }

    /// Ask for `version` of the secret on screen from its version list, with
    /// Enter (view) or `n` (edit a copy), as a user does
    fn ask_for_version(
        screen: &mut MainScreen,
        ctx: &ComponentContext,
        version: u64,
        with: KeyCode,
    ) -> Option<Action> {
        let detail = screen.detail_view.as_ref().expect("a secret is on screen");
        let newest = detail.current_version().expect("its history is known");
        let in_list = detail.context() == Context::Versions;

        screen.focused_pane = FocusedPane::Content;
        if !in_list {
            screen.handle_key(key(KeyCode::Char('l')), ctx).unwrap();
        }
        // Newest first
        screen.handle_key(key(KeyCode::Home), ctx).unwrap();
        for _ in version..newest {
            screen.handle_key(key(KeyCode::Down), ctx).unwrap();
        }
        screen.handle_key(key(with), ctx).unwrap()
    }

    fn version_loaded(path: &str, version: u64, value: serde_json::Value) -> Action {
        Action::SecretVersionLoaded {
            path: path.to_string(),
            version,
            data: secret(value),
        }
    }

    /// Press a key and apply the action it leads to, as `Root` does for the
    /// actions that stay on the main screen. Returns what is left for
    /// `Root`.
    fn press(screen: &mut MainScreen, ctx: &mut ComponentContext, code: KeyCode) -> Option<Action> {
        match screen.handle_key(key(code), ctx).unwrap() {
            Some(action @ (Action::CancelEdit | Action::Quit)) => {
                screen.update(action, ctx).unwrap()
            }
            other => other,
        }
    }

    /// A screen showing /a in the editor with one value changed
    fn edit_with_a_change(screen: &mut MainScreen, ctx: &mut ComponentContext) {
        screen
            .update(loaded("/a", json!({"host": "db"})), ctx)
            .unwrap();
        screen.update(Action::EditSecret, ctx).unwrap();
        press(screen, ctx, KeyCode::Enter);
        press(screen, ctx, KeyCode::Char('2'));
        press(screen, ctx, KeyCode::Enter);
    }

    fn edited(screen: &MainScreen) -> SecretData {
        let edit = screen.edit_view.as_ref().expect("the editor is open");
        edit.secret_data().unwrap()
    }

    /// Run `body` with a screen and the context components expect
    fn with_screen(body: impl FnOnce(&mut MainScreen, &mut ComponentContext)) {
        let app = test_app();
        let (tx, _rx) = unbounded_channel();
        let mut ctx = ComponentContext::new(&app, &tx);
        let mut screen = MainScreen::new();
        body(&mut screen, &mut ctx);
    }

    /// Rows on screen after drawing at the given terminal size
    fn rows(
        screen: &mut MainScreen,
        ctx: &ComponentContext,
        width: u16,
        height: u16,
    ) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                let render_ctx = RenderContext::new(ctx.app, true);
                screen.draw(frame, area, &render_ctx);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect()
    }

    fn text(screen: &mut MainScreen, ctx: &ComponentContext, width: u16, height: u16) -> String {
        rows(screen, ctx, width, height).join("\n")
    }

    #[test]
    fn filter_typing_does_not_trigger_global_shortcuts() {
        // Regression: with the filter open, "q" quit the app, "p" opened the
        // policy screen, "r" refreshed, and Tab moved focus away.
        with_screen(|screen, ctx| {
            screen.update(loaded("/a", json!({"k": "v"})), ctx).unwrap();

            screen.handle_key(key(KeyCode::Char('/')), ctx).unwrap();
            assert!(screen.navigation_pane.is_filtering());

            let mut typed = String::new();
            for c in ['q', 'p', 'r', '?', '~', 'h', 'l'] {
                typed.push(c);
                match screen.handle_key(key(KeyCode::Char(c)), ctx).unwrap() {
                    None | Some(Action::SelectionChanged { .. }) => {}
                    other => panic!("'{c}' escaped the filter as {other:?}"),
                }
                assert_eq!(screen.navigation_pane.filter_text(), typed);
            }

            assert!(screen.handle_key(key(KeyCode::Tab), ctx).unwrap().is_none());
            assert_eq!(screen.focused_pane, FocusedPane::Navigation);

            // Leaving the filter gives the shortcuts back
            screen.handle_key(key(KeyCode::Esc), ctx).unwrap();
            assert!(!screen.navigation_pane.is_filtering());
            assert_eq!(screen.navigation_pane.filter_text(), "");
            let action = screen.handle_key(key(KeyCode::Char('q')), ctx).unwrap();
            assert!(matches!(action, Some(Action::Quit)));
        });
    }

    #[test]
    fn late_secret_load_does_not_close_an_open_edit() {
        // Regression: any SecretLoaded closed the edit view, so a preview
        // finishing late discarded the edit in progress.
        with_screen(|screen, ctx| {
            screen.update(loaded("/a", json!({"k": "v"})), ctx).unwrap();
            screen.update(Action::EditSecret, ctx).unwrap();
            assert_eq!(screen.edit_view.as_ref().unwrap().path(), "/a");

            screen
                .update(loaded("/b", json!({"other": "x"})), ctx)
                .unwrap();

            let edit = screen.edit_view.as_ref().expect("edit view must survive");
            assert_eq!(edit.path(), "/a");
            assert_eq!(edit.secret_data().unwrap(), secret(json!({"k": "v"})));
        });
    }

    #[test]
    fn only_a_confirmed_save_of_the_same_secret_closes_the_edit() {
        with_screen(|screen, ctx| {
            screen.update(loaded("/a", json!({"k": "v"})), ctx).unwrap();
            screen.update(Action::EditSecret, ctx).unwrap();

            let other = Action::SecretSaved {
                path: "/b".to_string(),
            };
            screen.update(other, ctx).unwrap();
            assert!(screen.edit_view.is_some());

            // A failed save reports an error and keeps the edit buffer
            let failed = Action::SaveFailed {
                path: "/a".to_string(),
                message: "Could not save /a: Access denied".to_string(),
            };
            screen.update(failed, ctx).unwrap();
            assert!(screen.edit_view.is_some());

            let saved = Action::SecretSaved {
                path: "/a".to_string(),
            };
            screen.update(saved, ctx).unwrap();
            assert!(screen.edit_view.is_none());
        });
    }

    #[test]
    fn an_error_during_an_edit_is_shown_inside_the_editor() {
        // Regression: any error replaced the whole content pane, hiding the
        // fields being edited until it was dismissed.
        with_screen(|screen, ctx| {
            screen
                .update(loaded("/a", json!({"api_key": "v"})), ctx)
                .unwrap();
            screen.update(Action::EditSecret, ctx).unwrap();

            let failed = Action::SaveFailed {
                path: "/a".to_string(),
                message: "Could not save /a: Access denied".to_string(),
            };
            screen.update(failed, ctx).unwrap();

            let screen_text = text(screen, ctx, 80, 24);
            assert!(screen_text.contains("Access denied"), "{screen_text}");
            assert!(screen_text.contains("api_key"), "{screen_text}");
            assert!(screen.error_message.is_none());
        });
    }

    #[test]
    fn version_result_for_another_secret_is_ignored() {
        // Regression: SecretVersionLoaded carried no path, so an old version
        // of A could be loaded into B's view and then saved over B.
        with_screen(|screen, ctx| {
            screen.update(loaded_with_versions("/b", 3), ctx).unwrap();
            let asked = ask_for_version(screen, ctx, 1, KeyCode::Enter);
            assert!(
                matches!(asked, Some(Action::ViewSecretVersion { version: 1, .. })),
                "{asked:?}"
            );

            let stale = version_loaded("/a", 1, json!({"a": "old"}));
            screen.update(stale, ctx).unwrap();
            let detail = screen.detail_view.as_ref().unwrap();
            assert_eq!(detail.secret_data(), secret(json!({"k": "v"})));

            let matching = version_loaded("/b", 1, json!({"b": "old"}));
            screen.update(matching, ctx).unwrap();
            let detail = screen.detail_view.as_ref().unwrap();
            assert_eq!(detail.secret_data(), secret(json!({"b": "old"})));
        });
    }

    #[test]
    fn a_version_asked_for_before_another_one_is_not_shown() {
        // Regression: version results were checked by path only, so of two
        // requests for one secret the last to *arrive* was shown, whichever
        // version the user had picked last.
        for late_one_arrives_first in [true, false] {
            with_screen(|screen, ctx| {
                screen.update(loaded_with_versions("/b", 3), ctx).unwrap();
                ask_for_version(screen, ctx, 1, KeyCode::Enter);
                ask_for_version(screen, ctx, 2, KeyCode::Enter);

                let first = version_loaded("/b", 1, json!({"field_of_v1": "x"}));
                let second = version_loaded("/b", 2, json!({"field_of_v2": "x"}));
                let results = if late_one_arrives_first {
                    [first, second]
                } else {
                    [second, first]
                };
                for result in results {
                    screen.update(result, ctx).unwrap();
                }

                let screen_text = text(screen, ctx, 80, 24);
                assert!(screen_text.contains("field_of_v2"), "{screen_text}");
                assert!(!screen_text.contains("field_of_v1"), "{screen_text}");
            });
        }
    }

    #[test]
    fn a_version_that_failed_is_reported_only_while_it_is_awaited() {
        let failed = |path: &str, version| Action::VersionLoadFailed {
            path: path.to_string(),
            version,
            message: format!("Could not load version {version} of {path}: permission denied"),
        };

        with_screen(|screen, ctx| {
            screen.update(loaded_with_versions("/b", 3), ctx).unwrap();
            ask_for_version(screen, ctx, 1, KeyCode::Enter);
            ask_for_version(screen, ctx, 2, KeyCode::Enter);

            // Version 1 is no longer wanted, and /a is not on screen
            screen.update(failed("/b", 1), ctx).unwrap();
            screen.update(failed("/a", 2), ctx).unwrap();
            let screen_text = text(screen, ctx, 80, 24);
            assert!(!screen_text.contains("permission denied"), "{screen_text}");

            screen.update(failed("/b", 2), ctx).unwrap();
            let screen_text = text(screen, ctx, 80, 24);
            assert!(
                screen_text.contains("Could not load version 2 of /b"),
                "{screen_text}"
            );
        });
    }

    #[test]
    fn version_for_edit_opens_only_for_the_secret_on_screen() {
        with_screen(|screen, ctx| {
            screen.update(loaded_with_versions("/b", 3), ctx).unwrap();
            let asked = ask_for_version(screen, ctx, 1, KeyCode::Char('n'));
            assert!(
                matches!(asked, Some(Action::CreateFromVersion { version: 1, .. })),
                "{asked:?}"
            );

            let stale = Action::VersionLoadedForEdit {
                path: "/a".to_string(),
                version: 1,
                data: secret(json!({"a": "old"})),
            };
            screen.update(stale, ctx).unwrap();
            assert!(screen.edit_view.is_none());

            let matching = Action::VersionLoadedForEdit {
                path: "/b".to_string(),
                version: 1,
                data: secret(json!({"b": "old"})),
            };
            screen.update(matching, ctx).unwrap();
            assert_eq!(screen.edit_view.as_ref().unwrap().path(), "/b");
        });
    }

    #[test]
    fn editing_an_old_version_says_that_saving_restores_it() {
        // Regression: the only sign of an old version was a missing
        // "(current)" label, and saving the edit silently rolled back.
        with_screen(|screen, ctx| {
            screen.update(loaded_with_versions("/b", 7), ctx).unwrap();

            // Old version on screen, then `e`
            ask_for_version(screen, ctx, 3, KeyCode::Enter);
            let old = version_loaded("/b", 3, json!({"k": "old"}));
            screen.update(old, ctx).unwrap();
            let screen_text = text(screen, ctx, 100, 24);
            assert!(
                screen_text.contains("Viewing v3, not the current version (v7)"),
                "{screen_text}"
            );

            screen.update(Action::EditSecret, ctx).unwrap();
            let screen_text = text(screen, ctx, 100, 24);
            assert!(screen_text.contains("Copy of v3"), "{screen_text}");
            assert!(
                screen_text.contains("replaces the current v7"),
                "{screen_text}"
            );
            assert!(screen_text.contains("unsaved"), "{screen_text}");
        });

        with_screen(|screen, ctx| {
            screen.update(loaded_with_versions("/b", 7), ctx).unwrap();

            // "Restore as new" from the version list
            ask_for_version(screen, ctx, 2, KeyCode::Char('n'));
            let for_edit = Action::VersionLoadedForEdit {
                path: "/b".to_string(),
                version: 2,
                data: secret(json!({"k": "older"})),
            };
            screen.update(for_edit, ctx).unwrap();
            let screen_text = text(screen, ctx, 100, 24);
            assert!(screen_text.contains("Copy of v2"), "{screen_text}");
        });
    }

    #[test]
    fn selecting_another_secret_never_leaves_the_old_one_on_screen() {
        // Regression: the previous secret stayed in the pane until the next
        // one arrived, with nothing to say that it was loading.
        with_screen(|screen, ctx| {
            screen
                .update(loaded("/a", json!({"field_a": "v"})), ctx)
                .unwrap();
            screen.update(selected(Some("/b")), ctx).unwrap();

            let screen_text = text(screen, ctx, 80, 24);
            assert!(!screen_text.contains("field_a"), "{screen_text}");
            assert!(screen_text.contains("Loading"), "{screen_text}");
            assert!(screen_text.contains(" /b "), "{screen_text}");

            screen
                .update(loaded("/b", json!({"field_b": "v"})), ctx)
                .unwrap();
            let screen_text = text(screen, ctx, 80, 24);
            assert!(screen_text.contains("field_b"), "{screen_text}");
            assert!(!screen_text.contains("Loading"), "{screen_text}");
        });
    }

    #[test]
    fn selecting_a_folder_clears_the_pane() {
        with_screen(|screen, ctx| {
            screen
                .update(loaded("/a", json!({"field_a": "v"})), ctx)
                .unwrap();
            screen.focused_pane = FocusedPane::Content;

            screen.update(selected(None), ctx).unwrap();

            assert!(screen.detail_view.is_none());
            assert_eq!(screen.focused_pane, FocusedPane::Navigation);
            let screen_text = text(screen, ctx, 80, 24);
            assert!(!screen_text.contains("field_a"), "{screen_text}");
            assert!(!screen_text.contains("Loading"), "{screen_text}");
        });
    }

    #[test]
    fn a_failed_load_leaves_the_screen_when_the_cursor_moves_on() {
        // Regression: the error for one secret stayed in the pane under the
        // next selection, in place of its "Loading" placeholder and over
        // folders, until Esc.
        let failed = |path: &str| Action::SecretLoadFailed {
            path: path.to_string(),
            message: "permission denied".to_string(),
        };

        with_screen(|screen, ctx| {
            screen.update(selected(Some("/a")), ctx).unwrap();
            screen.update(failed("/a"), ctx).unwrap();
            let screen_text = text(screen, ctx, 80, 24);
            assert!(screen_text.contains("permission denied"), "{screen_text}");

            // On to another secret: its placeholder, not the old error
            screen.update(selected(Some("/b")), ctx).unwrap();
            let screen_text = text(screen, ctx, 80, 24);
            assert!(!screen_text.contains("permission denied"), "{screen_text}");
            assert!(screen_text.contains("Loading"), "{screen_text}");

            // On to a folder: nothing of the failed secret is left
            screen.update(failed("/b"), ctx).unwrap();
            screen.update(selected(None), ctx).unwrap();
            let screen_text = text(screen, ctx, 80, 24);
            assert!(!screen_text.contains("permission denied"), "{screen_text}");
        });
    }

    #[test]
    fn cancelling_a_new_secret_shows_the_selected_secret_again() {
        // Regression: starting a new secret threw away the secret under the
        // cursor, so after Esc the pane was empty with the cursor still on
        // it, and Enter waited for a load that was never requested.
        with_screen(|screen, ctx| {
            screen
                .update(loaded("/a", json!({"field_a": "v"})), ctx)
                .unwrap();
            screen
                .update(
                    Action::CreateSecret {
                        name: "/new".to_string(),
                    },
                    ctx,
                )
                .unwrap();
            let screen_text = text(screen, ctx, 80, 24);
            assert!(!screen_text.contains("field_a"), "{screen_text}");

            // Esc twice: out of the first field, then out of the editor
            press(screen, ctx, KeyCode::Esc);
            press(screen, ctx, KeyCode::Esc);

            assert!(screen.edit_view.is_none());
            let screen_text = text(screen, ctx, 80, 24);
            assert!(screen_text.contains("field_a"), "{screen_text}");
            assert!(!screen_text.contains("Loading"), "{screen_text}");
        });
    }

    #[test]
    fn reselecting_the_secret_on_screen_keeps_it() {
        with_screen(|screen, ctx| {
            screen
                .update(loaded("/a", json!({"field_a": "v"})), ctx)
                .unwrap();
            screen.update(selected(Some("/a")), ctx).unwrap();
            assert!(screen.detail_view.is_some());
            assert!(screen.loading_secret.is_none());
        });
    }

    #[test]
    fn enter_on_a_secret_focuses_it_once_it_is_there() {
        with_screen(|screen, ctx| {
            // Not loaded yet: focus waits for it
            let open = Action::OpenSecret {
                path: "/a".to_string(),
            };
            screen.update(open.clone(), ctx).unwrap();
            assert_eq!(screen.focused_pane, FocusedPane::Navigation);
            assert!(text(screen, ctx, 80, 24).contains("Loading"));

            screen.update(loaded("/a", json!({"k": "v"})), ctx).unwrap();
            assert_eq!(screen.focused_pane, FocusedPane::Content);

            // Already on screen: focus moves at once
            screen.focused_pane = FocusedPane::Navigation;
            screen.update(open, ctx).unwrap();
            assert_eq!(screen.focused_pane, FocusedPane::Content);
        });
    }

    #[test]
    fn a_new_secret_is_focused_once_it_has_been_saved_and_read_back() {
        with_screen(|screen, ctx| {
            screen
                .update(
                    Action::CreateSecret {
                        name: "/new".to_string(),
                    },
                    ctx,
                )
                .unwrap();
            assert_eq!(screen.focused_pane, FocusedPane::Content);

            // The order the save produces: saved, the list reloads and puts
            // its cursor on the new secret, then the secret arrives
            let saved = Action::SecretSaved {
                path: "/new".to_string(),
            };
            screen.update(saved, ctx).unwrap();
            assert!(screen.edit_view.is_none());
            screen.update(selected(Some("/new")), ctx).unwrap();
            screen
                .update(loaded("/new", json!({"k": "v"})), ctx)
                .unwrap();

            assert_eq!(screen.focused_pane, FocusedPane::Content);
            assert_eq!(screen.detail_view.as_ref().unwrap().path(), "/new");
        });
    }

    #[test]
    fn moving_on_cancels_a_pending_enter() {
        with_screen(|screen, ctx| {
            let open = Action::OpenSecret {
                path: "/a".to_string(),
            };
            screen.update(open, ctx).unwrap();
            screen.update(selected(Some("/b")), ctx).unwrap();
            screen.update(loaded("/b", json!({"k": "v"})), ctx).unwrap();
            assert_eq!(screen.focused_pane, FocusedPane::Navigation);
        });
    }

    #[test]
    fn a_deleted_secret_leaves_the_screen() {
        // Regression: the pane kept showing the deleted secret, which could
        // then be edited and saved back.
        with_screen(|screen, ctx| {
            screen.update(loaded("/a", json!({"k": "v"})), ctx).unwrap();
            screen.focused_pane = FocusedPane::Content;

            let deleted = Action::SecretDeleted {
                path: "/a".to_string(),
            };
            screen.update(deleted, ctx).unwrap();

            assert!(screen.detail_view.is_none());
            assert_eq!(screen.focused_pane, FocusedPane::Navigation);
        });
    }

    #[test]
    fn delete_dialog_shows_everything_at_80x24_and_needs_y() {
        // Regression: the dialog was 30% of the screen height, so at 80x24
        // neither the path nor the warning fitted, and Enter confirmed.
        with_screen(|screen, ctx| {
            screen
                .update(loaded_with_versions("/app/db", 3), ctx)
                .unwrap();
            let ask = Action::ShowDeleteConfirmation {
                item_name: "/app/db".to_string(),
            };
            screen.update(ask, ctx).unwrap();

            let screen_text = text(screen, ctx, 80, 24);
            assert!(screen_text.contains("Delete secret"), "{screen_text}");
            assert!(screen_text.contains("/app/db"), "{screen_text}");
            assert!(
                screen_text.contains("All 3 versions and the metadata are destroyed."),
                "{screen_text}"
            );
            assert!(
                screen_text.contains("This cannot be undone."),
                "{screen_text}"
            );
            assert!(screen_text.contains("y delete"), "{screen_text}");

            // Enter is not a confirmation
            let action = screen.handle_key(key(KeyCode::Enter), ctx).unwrap();
            assert!(action.is_none(), "{action:?}");
            assert!(screen.delete_confirmation.is_some());

            let action = screen.handle_key(key(KeyCode::Char('y')), ctx).unwrap();
            assert!(
                matches!(&action, Some(Action::DeleteSecret { path }) if path == "/app/db"),
                "{action:?}"
            );
            assert!(screen.delete_confirmation.is_none());
        });
    }

    #[test]
    fn delete_dialog_without_history_does_not_guess_a_count() {
        with_screen(|screen, ctx| {
            let ask = Action::ShowDeleteConfirmation {
                item_name: "/app/other".to_string(),
            };
            screen.update(ask, ctx).unwrap();
            let screen_text = text(screen, ctx, 80, 24);
            assert!(
                screen_text.contains("Every version and the metadata are destroyed."),
                "{screen_text}"
            );

            let action = screen.handle_key(key(KeyCode::Esc), ctx).unwrap();
            assert!(action.is_none());
            assert!(screen.delete_confirmation.is_none());
        });
    }

    #[test]
    fn global_shortcuts_are_off_while_the_editor_has_focus() {
        // Regression: `q` in the edit view quit the app with the edit open.
        with_screen(|screen, ctx| {
            screen.update(loaded("/a", json!({"k": "v"})), ctx).unwrap();
            screen.update(Action::EditSecret, ctx).unwrap();
            assert_eq!(screen.focused_pane, FocusedPane::Content);

            for c in ['q', 'p', '~'] {
                let action = screen.handle_key(key(KeyCode::Char(c)), ctx).unwrap();
                assert!(action.is_none(), "'{c}' gave {action:?}");
            }
            assert!(screen.edit_view.is_some());

            // Help and leaving the pane still work
            let action = screen.handle_key(key(KeyCode::Char('?')), ctx).unwrap();
            assert!(matches!(action, Some(Action::ShowHelp)));
            screen.handle_key(key(KeyCode::Tab), ctx).unwrap();
            assert_eq!(screen.focused_pane, FocusedPane::Navigation);
            assert!(screen.edit_view.is_some());
        });
    }

    #[test]
    fn footer_follows_the_field_delete_question() {
        // Regression: with the question open the footer went on offering
        // the editor's keys, none of which did anything.
        with_screen(|screen, ctx| {
            screen
                .update(loaded("/a", json!({"host": "db", "port": "1"})), ctx)
                .unwrap();
            screen.update(Action::EditSecret, ctx).unwrap();
            screen.handle_key(key(KeyCode::Char('d')), ctx).unwrap();

            let all_rows = rows(screen, ctx, 80, 24);
            let footer = all_rows.last().unwrap();
            assert!(footer.contains("y delete"), "{footer}");
            assert!(footer.contains("cancel"), "{footer}");
            assert!(!footer.contains("rename"), "{footer}");
            assert!(!footer.contains("save"), "{footer}");
        });
    }

    #[test]
    fn creating_over_an_existing_name_is_refused() {
        // Regression: the name was not checked, and saving the empty editor
        // wrote a new version over the existing secret.
        with_screen(|screen, ctx| {
            ctx.app
                .state()
                .reload_folder("/", vec!["db".to_string(), "dir/".to_string()]);
            screen.update(Action::ShowCreateSecretInput, ctx).unwrap();

            for c in "db".chars() {
                screen.handle_key(key(KeyCode::Char(c)), ctx).unwrap();
            }
            let action = screen.handle_key(key(KeyCode::Enter), ctx).unwrap();
            assert!(action.is_none(), "{action:?}");
            assert!(screen.create_input.is_some());
            let screen_text = text(screen, ctx, 80, 24);
            assert!(screen_text.contains("'db' already exists"), "{screen_text}");

            // The prompt is a real text input: fix the name in place
            screen.handle_key(key(KeyCode::Home), ctx).unwrap();
            for c in "new-".chars() {
                screen.handle_key(key(KeyCode::Char(c)), ctx).unwrap();
            }
            let action = screen.handle_key(key(KeyCode::Enter), ctx).unwrap();
            assert!(
                matches!(&action, Some(Action::CreateSecret { name }) if name == "/new-db"),
                "{action:?}"
            );
            assert!(screen.create_input.is_none());
        });
    }

    #[test]
    fn pasted_text_is_never_run_as_commands() {
        // Regression: without bracketed paste a pasted "dy" deleted the
        // selected secret.
        with_screen(|screen, ctx| {
            ctx.app.state().reload_folder("/", vec!["db".to_string()]);
            screen.init(ctx).unwrap();
            screen
                .update(loaded("/db", json!({"k": "v"})), ctx)
                .unwrap();

            let action = screen.handle_paste("dy\nq", ctx).unwrap();
            assert!(action.is_none(), "{action:?}");
            assert!(screen.delete_confirmation.is_none());

            // In the name prompt it is text, first line only
            screen.update(Action::ShowCreateSecretInput, ctx).unwrap();
            screen.handle_paste("pasted-name\nq", ctx).unwrap();
            let action = screen.handle_key(key(KeyCode::Enter), ctx).unwrap();
            assert!(
                matches!(&action, Some(Action::CreateSecret { name }) if name == "/pasted-name"),
                "{action:?}"
            );
        });
    }

    #[test]
    fn a_standard_terminal_shows_more_than_a_dozen_fields() {
        // Regression: at 80x24, 15 rows were headers, footers and borders,
        // leaving 7 rows for fields.
        with_screen(|screen, ctx| {
            let fields: serde_json::Map<String, serde_json::Value> = (0..16)
                .map(|i| (format!("field_{i:02}"), json!("v")))
                .collect();
            screen
                .update(loaded("/app/db", serde_json::Value::Object(fields)), ctx)
                .unwrap();

            let screen_text = text(screen, ctx, 80, 24);
            for i in 0..16 {
                assert!(
                    screen_text.contains(&format!("field_{i:02}")),
                    "field_{i:02} is not on screen:\n{screen_text}"
                );
            }
        });
    }

    #[test]
    fn header_and_footer_say_where_you_are_and_what_works() {
        with_screen(|screen, ctx| {
            screen.update(loaded("/a", json!({"k": "v"})), ctx).unwrap();
            let lines = rows(screen, ctx, 80, 24);

            // Which Vault this is
            assert!(lines[0].contains("VltView"), "{}", lines[0]);
            assert!(lines[0].contains("http://127.0.0.1:1"), "{}", lines[0]);

            // Navigation has focus: its keys, none of the secret's
            let footer = &lines[23];
            assert!(footer.contains("Enter open"), "{footer}");
            assert!(footer.contains("n new"), "{footer}");
            assert!(!footer.contains("e edit"), "{footer}");

            // Regression: the footer always offered n/d, which do nothing
            // while the content pane has focus
            screen.handle_key(key(KeyCode::Tab), ctx).unwrap();
            let lines = rows(screen, ctx, 80, 24);
            let footer = &lines[23];
            assert!(footer.contains("e edit"), "{footer}");
            assert!(footer.contains("c copy"), "{footer}");
            assert!(!footer.contains("n new"), "{footer}");
            assert!(!footer.contains("d delete"), "{footer}");

            // Regression: "Ctrl+s: Save secret" was cut off at 80 columns
            screen.update(Action::EditSecret, ctx).unwrap();
            let lines = rows(screen, ctx, 80, 24);
            let footer = &lines[23];
            assert!(footer.contains("Ctrl+s save"), "{footer}");
            assert!(footer.contains("Esc cancel"), "{footer}");
        });
    }

    #[test]
    fn busy_indicator_follows_the_loading_state() {
        with_screen(|screen, ctx| {
            let spinner = |line: &str| line.chars().any(|c| ('⠀'..='⣿').contains(&c));

            let idle = rows(screen, ctx, 80, 24);
            assert!(!spinner(&idle[0]), "{}", idle[0]);

            screen
                .update(Action::SetLoading { loading: true }, ctx)
                .unwrap();
            let busy = rows(screen, ctx, 80, 24);
            assert!(spinner(&busy[0]), "{}", busy[0]);
        });
    }

    #[test]
    fn tab_with_nothing_to_focus_says_why() {
        with_screen(|screen, ctx| {
            screen.handle_key(key(KeyCode::Tab), ctx).unwrap();
            assert_eq!(screen.focused_pane, FocusedPane::Navigation);
            let lines = rows(screen, ctx, 80, 24);
            assert!(lines[23].contains("Select a secret first"), "{}", lines[23]);
        });
    }

    #[test]
    fn a_failed_save_is_shown_in_the_editor_it_came_from() {
        with_screen(|screen, ctx| {
            screen
                .update(loaded("/a", json!({"api_key": "v"})), ctx)
                .unwrap();
            screen.update(Action::EditSecret, ctx).unwrap();

            let failed = Action::SaveFailed {
                path: "/a".to_string(),
                message: "Could not save /a: Access denied".to_string(),
            };
            screen.update(failed, ctx).unwrap();

            let screen_text = text(screen, ctx, 80, 24);
            assert!(screen_text.contains("Access denied"), "{screen_text}");
            assert!(screen_text.contains("api_key"), "{screen_text}");
            assert!(screen.notices.is_empty());
        });
    }

    #[test]
    fn an_error_about_something_else_never_lands_in_an_open_editor() {
        // Regression: errors carried no hint of what they were about, so
        // whatever failed was shown inside the editor that happened to be
        // open, as if the edit had caused it.
        let errors = [
            Action::SaveFailed {
                path: "/other".to_string(),
                message: "Could not save /other: Access denied".to_string(),
            },
            Action::DeleteFailed {
                message: "Could not delete /other: Access denied".to_string(),
            },
            Action::FolderListFailed {
                request: 1,
                message: "Could not open /other: Access denied".to_string(),
            },
        ];

        for error in errors {
            with_screen(|screen, ctx| {
                screen
                    .update(loaded("/a", json!({"api_key": "v"})), ctx)
                    .unwrap();
                screen.update(Action::EditSecret, ctx).unwrap();
                screen.update(error.clone(), ctx).unwrap();

                // Said over the screen, naming what it is about
                let screen_text = text(screen, ctx, 80, 24);
                assert!(screen_text.contains(" Error "), "{screen_text}");
                assert!(
                    screen_text.contains("/other: Access denied"),
                    "{screen_text}"
                );

                // Closed, it leaves the editor as it was, with no error in it
                screen.handle_key(key(KeyCode::Esc), ctx).unwrap();
                let screen_text = text(screen, ctx, 80, 24);
                assert!(!screen_text.contains("Access denied"), "{screen_text}");
                assert!(screen_text.contains("api_key"), "{screen_text}");
                assert_eq!(screen.edit_view.as_ref().unwrap().path(), "/a");
            });
        }
    }

    #[test]
    fn an_error_message_is_all_on_screen_and_takes_every_key() {
        with_screen(|screen, ctx| {
            screen.update(loaded("/a", json!({"k": "v"})), ctx).unwrap();
            let message = "Could not open /team/payments/production/eu-west-1: permission \
                           denied. The token's policies do not allow listing this folder.";
            let failed = Action::FolderListFailed {
                request: 1,
                message: message.to_string(),
            };
            screen.update(failed, ctx).unwrap();

            // Every word of it is there at 80x24, in order
            let screen_text = text(screen, ctx, 80, 24);
            crate::tui::components::popup::assert_shown_whole(&screen_text, message);
            assert!(screen_text.contains("Esc close"), "{screen_text}");

            // Nothing else happens until it is closed: `q` does not quit
            for c in ['q', 'd', 'n', 'p'] {
                let action = screen.handle_key(key(KeyCode::Char(c)), ctx).unwrap();
                assert!(action.is_none(), "'{c}' got through as {action:?}");
            }
            assert!(!screen.notices.is_empty());
            assert!(screen.create_input.is_none());

            screen.handle_key(key(KeyCode::Enter), ctx).unwrap();
            assert!(screen.notices.is_empty());
            let action = screen.handle_key(key(KeyCode::Char('q')), ctx).unwrap();
            assert!(matches!(action, Some(Action::Quit)));
        });
    }

    #[test]
    fn a_new_secret_is_saved_as_a_create_and_a_taken_name_keeps_the_editor() {
        // Regression: the name was only compared with the folder on screen,
        // so a nested name such as `a/b`, or a secret someone else created
        // meanwhile, was overwritten with a new version.
        with_screen(|screen, ctx| {
            ctx.app.state().reload_folder("/", vec!["a/".to_string()]);
            screen.update(Action::ShowCreateSecretInput, ctx).unwrap();
            for c in "a/b".chars() {
                screen.handle_key(key(KeyCode::Char(c)), ctx).unwrap();
            }
            let create = screen.handle_key(key(KeyCode::Enter), ctx).unwrap();
            screen
                .update(create.expect("the editor opens"), ctx)
                .unwrap();

            // One field, then Ctrl+S
            for c in "user".chars() {
                screen.handle_key(key(KeyCode::Char(c)), ctx).unwrap();
            }
            screen.handle_key(key(KeyCode::Enter), ctx).unwrap();
            for c in "admin".chars() {
                screen.handle_key(key(KeyCode::Char(c)), ctx).unwrap();
            }
            let save = screen
                .handle_key(
                    KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
                    ctx,
                )
                .unwrap();
            match save {
                Some(Action::SaveSecret { path, mode, .. }) => {
                    assert_eq!(path, "/a/b");
                    assert_eq!(mode, crate::vault::WriteMode::Create);
                }
                other => panic!("expected a save, got {other:?}"),
            }

            // Vault refuses: /a/b exists
            let refused = Action::SaveFailed {
                path: "/a/b".to_string(),
                message: "Could not save /a/b: a secret already exists at this path; \
                          nothing was written"
                    .to_string(),
            };
            screen.update(refused, ctx).unwrap();

            let screen_text = text(screen, ctx, 80, 24);
            assert!(screen_text.contains("already exists"), "{screen_text}");
            assert!(screen_text.contains("nothing was written"), "{screen_text}");
            assert!(screen_text.contains("user"), "{screen_text}");
            let edit = screen.edit_view.as_ref().expect("the editor stays open");
            assert_eq!(
                edit.secret_data().unwrap(),
                secret(json!({"user": "admin"}))
            );
        });
    }

    #[test]
    fn an_edit_of_an_existing_secret_is_saved_as_an_update() {
        with_screen(|screen, ctx| {
            screen.update(loaded("/a", json!({"k": "v"})), ctx).unwrap();
            screen.update(Action::EditSecret, ctx).unwrap();
            let save = screen
                .handle_key(
                    KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
                    ctx,
                )
                .unwrap();
            assert!(
                matches!(
                    save,
                    Some(Action::SaveSecret {
                        mode: crate::vault::WriteMode::Update,
                        ..
                    })
                ),
                "{save:?}"
            );
        });
    }

    #[test]
    fn escape_with_unsaved_changes_asks_and_y_discards_them() {
        // Regression: one Esc in the editor threw the changes away.
        with_screen(|screen, ctx| {
            edit_with_a_change(screen, ctx);

            assert!(press(screen, ctx, KeyCode::Esc).is_none());
            assert!(screen.edit_view.is_some(), "not closed without asking");
            let screen_text = text(screen, ctx, 80, 24);
            assert!(screen_text.contains(" Unsaved changes "), "{screen_text}");
            assert!(
                screen_text.contains("The changes to this secret have not been saved."),
                "{screen_text}"
            );
            let footer = rows(screen, ctx, 80, 24).pop().unwrap();
            assert!(footer.contains("y discard"), "{footer}");
            assert!(footer.contains("n/Esc keep editing"), "{footer}");

            press(screen, ctx, KeyCode::Char('y'));
            assert!(screen.edit_view.is_none());
            let screen_text = text(screen, ctx, 80, 24);
            assert!(!screen_text.contains("Unsaved changes"), "{screen_text}");
            assert!(screen_text.contains("host"), "{screen_text}");
        });
    }

    #[test]
    fn declining_to_discard_keeps_every_change() {
        for decline in [KeyCode::Char('n'), KeyCode::Esc] {
            with_screen(|screen, ctx| {
                edit_with_a_change(screen, ctx);
                press(screen, ctx, KeyCode::Esc);

                // Other keys do nothing while the question is open
                press(screen, ctx, KeyCode::Enter);
                press(screen, ctx, KeyCode::Char('q'));
                assert!(text(screen, ctx, 80, 24).contains(" Unsaved changes "));

                press(screen, ctx, decline);
                assert!(!text(screen, ctx, 80, 24).contains(" Unsaved changes "));
                assert_eq!(edited(screen), secret(json!({"host": "db2"})));
                assert_eq!(screen.focused_pane, FocusedPane::Content);
            });
        }
    }

    #[test]
    fn escape_without_changes_closes_the_editor_at_once() {
        with_screen(|screen, ctx| {
            screen
                .update(loaded("/a", json!({"host": "db"})), ctx)
                .unwrap();
            screen.update(Action::EditSecret, ctx).unwrap();

            press(screen, ctx, KeyCode::Esc);
            assert!(screen.edit_view.is_none());
            assert!(!text(screen, ctx, 80, 24).contains("Unsaved changes"));
        });
    }

    #[test]
    fn quitting_from_the_list_with_unsaved_changes_asks_first() {
        // Regression: Tab to the list and `q` quit with the edit open.
        with_screen(|screen, ctx| {
            edit_with_a_change(screen, ctx);
            press(screen, ctx, KeyCode::Tab);
            assert_eq!(screen.focused_pane, FocusedPane::Navigation);

            let left_for_root = press(screen, ctx, KeyCode::Char('q'));
            assert!(left_for_root.is_none(), "{left_for_root:?}");
            let screen_text = text(screen, ctx, 80, 24);
            assert!(screen_text.contains(" Unsaved changes "), "{screen_text}");
            let footer = rows(screen, ctx, 80, 24).pop().unwrap();
            assert!(footer.contains("y quit"), "{footer}");
            assert!(footer.contains("n/Esc stay"), "{footer}");

            // Declined: back in the list, with the edit as it was
            press(screen, ctx, KeyCode::Char('n'));
            assert_eq!(screen.focused_pane, FocusedPane::Navigation);
            assert_eq!(edited(screen), secret(json!({"host": "db2"})));

            // Confirmed: quit
            press(screen, ctx, KeyCode::Char('q'));
            let left_for_root = press(screen, ctx, KeyCode::Char('y'));
            assert!(
                matches!(left_for_root, Some(Action::QuitNow)),
                "{left_for_root:?}"
            );
        });
    }

    #[test]
    fn quitting_with_nothing_unsaved_does_not_ask() {
        with_screen(|screen, ctx| {
            // No editor
            let left_for_root = press(screen, ctx, KeyCode::Char('q'));
            assert!(matches!(left_for_root, Some(Action::QuitNow)));

            // An editor with no changes
            screen
                .update(loaded("/a", json!({"host": "db"})), ctx)
                .unwrap();
            screen.update(Action::EditSecret, ctx).unwrap();
            press(screen, ctx, KeyCode::Tab);
            let left_for_root = press(screen, ctx, KeyCode::Char('q'));
            assert!(matches!(left_for_root, Some(Action::QuitNow)));
        });
    }

    #[test]
    fn an_edit_of_an_old_version_counts_as_changed_from_the_start() {
        with_screen(|screen, ctx| {
            screen.update(loaded_with_versions("/b", 7), ctx).unwrap();
            ask_for_version(screen, ctx, 2, KeyCode::Char('n'));
            let for_edit = Action::VersionLoadedForEdit {
                path: "/b".to_string(),
                version: 2,
                data: secret(json!({"k": "older"})),
            };
            screen.update(for_edit, ctx).unwrap();

            press(screen, ctx, KeyCode::Esc);
            assert!(screen.edit_view.is_some());
            assert!(text(screen, ctx, 80, 24).contains(" Unsaved changes "));
        });
    }

    #[test]
    fn a_new_secret_cannot_replace_an_edit_with_unsaved_changes() {
        // The other way to lose an edit: Tab to the list, `n`, a name, Enter
        with_screen(|screen, ctx| {
            edit_with_a_change(screen, ctx);
            press(screen, ctx, KeyCode::Tab);

            screen.update(Action::ShowCreateSecretInput, ctx).unwrap();
            assert!(screen.create_input.is_none());
            let footer = rows(screen, ctx, 80, 24).pop().unwrap();
            assert!(
                footer.contains("Save or close the open edit first"),
                "{footer}"
            );
            assert_eq!(edited(screen), secret(json!({"host": "db2"})));
        });
    }

    #[test]
    fn the_name_prompts_error_is_never_cut() {
        // Regression: the error quoted the name and was cut at two thirds
        // of the footer, so a long name pushed "already exists" off screen.
        let name = "payments-gateway-production-credentials-eu";
        let type_name = |screen: &mut MainScreen, ctx: &mut ComponentContext| {
            ctx.app.state().reload_folder("/", vec![name.to_string()]);
            screen.update(Action::ShowCreateSecretInput, ctx).unwrap();
            for c in name.chars() {
                screen.handle_key(key(KeyCode::Char(c)), ctx).unwrap();
            }
            screen.handle_key(key(KeyCode::Enter), ctx).unwrap();
            assert!(screen.create_input.is_some(), "the name is refused");
        };

        with_screen(|screen, ctx| {
            type_name(screen, ctx);
            let footer = rows(screen, ctx, 80, 24).pop().unwrap();
            assert!(
                footer.trim_end().ends_with("that name already exists"),
                "{footer}"
            );
        });

        // Less room: a shorter text, whole
        with_screen(|screen, ctx| {
            type_name(screen, ctx);
            let footer = rows(screen, ctx, 44, 24).pop().unwrap();
            assert!(footer.trim_end().ends_with("name taken"), "{footer}");
        });

        // No room at all: nothing rather than a piece of it
        with_screen(|screen, ctx| {
            type_name(screen, ctx);
            let footer = rows(screen, ctx, 30, 24).pop().unwrap();
            assert!(!footer.contains("taken"), "{footer}");
            assert!(!footer.contains("exist"), "{footer}");
        });

        // An empty name
        with_screen(|screen, ctx| {
            screen.update(Action::ShowCreateSecretInput, ctx).unwrap();
            screen.handle_key(key(KeyCode::Enter), ctx).unwrap();
            let footer = rows(screen, ctx, 80, 24).pop().unwrap();
            assert!(footer.trim_end().ends_with("a name is needed"), "{footer}");
        });
    }
}
