use crate::tui::action::{Action, FocusedPane};
use crate::tui::component::Component;
use crate::tui::components::{DetailViewComponent, EditViewComponent, NavigationPane};
use crate::tui::context::{ComponentContext, RenderContext};
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

/// Input mode for the main screen
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InputMode {
    /// Normal navigation mode
    Normal,
    /// Filter mode (typing filter text)
    Filter,
    /// Create secret name input mode
    CreateSecret,
}

/// Main screen component with split view (navigation + content)
///
/// This is the primary screen of VltView, showing:
/// - Left pane: NavigationPane for browsing paths/secrets
/// - Right pane: DetailView, EditView, or empty placeholder
/// - Header: Current path display
/// - Footer: Status/shortcuts/input prompts
pub struct MainScreen {
    /// Navigation pane (left side)
    navigation_pane: NavigationPane,

    /// Detail view component (right side, when viewing)
    detail_view: Option<DetailViewComponent>,

    /// Edit view component (right side, when editing)
    edit_view: Option<EditViewComponent>,

    /// Which pane currently has focus
    focused_pane: FocusedPane,

    /// Input mode
    input_mode: InputMode,

    /// New secret name being typed
    new_secret_name: String,

    /// Status message (temporary)
    status_message: Option<String>,

    /// Error message
    error_message: Option<String>,

    /// Loading state
    is_loading: bool,

    /// Delete confirmation - stores the path to delete
    delete_confirmation: Option<String>,
}

impl MainScreen {
    /// Create a new main screen
    pub fn new() -> Self {
        Self {
            navigation_pane: NavigationPane::new(),
            detail_view: None,
            edit_view: None,
            focused_pane: FocusedPane::Navigation,
            input_mode: InputMode::Normal,
            new_secret_name: String::new(),
            status_message: None,
            error_message: None,
            is_loading: false,
            delete_confirmation: None,
        }
    }

    /// Handle create secret name input
    fn handle_create_secret_input(
        &mut self,
        key: KeyEvent,
        ctx: &ComponentContext,
    ) -> Result<Option<Action>> {
        match key.code {
            KeyCode::Char(c) => {
                self.new_secret_name.push(c);
                Ok(None)
            }
            KeyCode::Backspace => {
                self.new_secret_name.pop();
                Ok(None)
            }
            KeyCode::Enter => {
                let name = self.new_secret_name.trim().to_string();
                if name.is_empty() {
                    self.error_message = Some("Secret name cannot be empty".to_string());
                    return Ok(None);
                }

                // Build full path
                let current_path = ctx.app.state().current_path();
                let path = format!("{}/{}", current_path.trim_end_matches('/'), name);

                // Exit create mode and emit action
                self.new_secret_name.clear();
                self.input_mode = InputMode::Normal;

                Ok(Some(Action::CreateSecret { name: path }))
            }
            KeyCode::Esc => {
                self.new_secret_name.clear();
                self.input_mode = InputMode::Normal;
                self.error_message = None;
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    /// Handle delete confirmation dialog
    fn handle_delete_confirmation(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        match key.code {
            KeyCode::Char('y') | KeyCode::Enter => {
                if let Some(path) = self.delete_confirmation.take() {
                    Ok(Some(Action::DeleteSecret { path }))
                } else {
                    Ok(None)
                }
            }
            KeyCode::Char('n') | KeyCode::Esc => {
                self.delete_confirmation = None;
                Ok(None)
            }
            _ => Ok(None), // Ignore other keys
        }
    }

    /// Render the header
    fn render_header(frame: &mut Frame, area: Rect, current_path: &str) {
        let title = Paragraph::new(Line::from(vec![
            Span::styled(
                "⚙️ VltView",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" - HashiCorp Vault TUI"),
            Span::styled(" │ ", Style::default().fg(Color::DarkGray)),
            Span::styled("Path: ", Style::default().fg(Color::Gray)),
            Span::styled(current_path, Style::default().fg(Color::Green)),
        ]))
        .block(Block::default().borders(Borders::ALL));

        frame.render_widget(title, area);
    }

    /// Render the footer
    fn render_footer(
        frame: &mut Frame,
        area: Rect,
        focused_pane: FocusedPane,
        input_mode: InputMode,
        filter_active: bool,
        new_secret_name: &str,
        status_message: &Option<String>,
    ) {
        let content = match input_mode {
            InputMode::CreateSecret => {
                // Show create secret name input box
                let mut spans = vec![Span::styled(
                    "New secret name: ",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )];

                if new_secret_name.is_empty() {
                    spans.push(Span::styled(
                        " ",
                        Style::default().fg(Color::Black).bg(Color::Green),
                    ));
                } else {
                    spans.push(Span::styled(
                        new_secret_name,
                        Style::default().fg(Color::White),
                    ));
                    spans.push(Span::styled(
                        " ",
                        Style::default().fg(Color::Black).bg(Color::Green),
                    ));
                }

                spans.extend(vec![
                    Span::raw("  "),
                    Span::styled(
                        "Enter",
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(": Create | "),
                    Span::styled(
                        "Esc",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(": Cancel"),
                ]);

                Line::from(spans)
            }
            InputMode::Filter => {
                // Filter input is handled by NavigationPane itself
                // This shouldn't happen, but show a placeholder
                Line::from(Span::styled(
                    "Filter mode...",
                    Style::default().fg(Color::Cyan),
                ))
            }
            InputMode::Normal => {
                if let Some(status) = status_message {
                    Line::from(vec![Span::styled(
                        status,
                        Style::default().fg(Color::Green),
                    )])
                } else {
                    let focus_indicator = match focused_pane {
                        FocusedPane::Navigation => Span::styled(
                            "[NAV]",
                            Style::default()
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::BOLD),
                        ),
                        FocusedPane::Content => Span::styled(
                            "[CONTENT]",
                            Style::default()
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::BOLD),
                        ),
                    };

                    let mut spans = vec![focus_indicator, Span::raw(" ")];

                    if filter_active {
                        spans.push(Span::styled(
                            "🔍 Filter active",
                            Style::default()
                                .fg(Color::Green)
                                .add_modifier(Modifier::BOLD),
                        ));
                        spans.push(Span::raw(" | "));
                    }

                    spans.extend(vec![
                        Span::styled(
                            "n",
                            Style::default()
                                .fg(Color::Green)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::raw(": New | "),
                        Span::styled(
                            "d",
                            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                        ),
                        Span::raw(": Delete | "),
                        Span::styled(
                            "Tab",
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::raw(": Switch | "),
                        Span::styled(
                            "?",
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::raw(": Help | "),
                        Span::styled(
                            "q",
                            Style::default()
                                .fg(Color::Yellow)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::raw(": Quit"),
                    ]);

                    Line::from(spans)
                }
            }
        };

        let footer = Paragraph::new(content).block(Block::default().borders(Borders::ALL));
        frame.render_widget(footer, area);
    }

    /// Render the content pane (right side)
    fn render_content_pane(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        ctx: &RenderContext,
        error_message: &Option<String>,
    ) {
        let border_color = if ctx.is_focused {
            Color::Cyan
        } else {
            Color::DarkGray
        };

        // Priority: error > edit view > detail view > empty
        if let Some(error) = error_message {
            let error_text = vec![
                Line::from(""),
                Line::from(""),
                Line::from(Span::styled(
                    "Error",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    error.as_str(),
                    Style::default().fg(Color::Red),
                )),
                Line::from(""),
                Line::from(""),
                Line::from(Span::styled(
                    "Press Esc to dismiss",
                    Style::default().fg(Color::Yellow),
                )),
            ];
            let error_widget = Paragraph::new(error_text)
                .alignment(Alignment::Center)
                .wrap(Wrap { trim: true })
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Red))
                        .title("Content"),
                );
            frame.render_widget(error_widget, area);
        } else if let Some(view) = &mut self.edit_view {
            view.draw(frame, area, ctx);
        } else if let Some(view) = &mut self.detail_view {
            view.draw(frame, area, ctx);
        } else {
            // Empty content pane
            let empty = Paragraph::new(vec![
                Line::from(""),
                Line::from(Span::styled(
                    "Select a secret to view its contents",
                    Style::default().fg(Color::DarkGray),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "Press Enter on a secret in the navigation pane",
                    Style::default().fg(Color::DarkGray),
                )),
            ])
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(border_color))
                    .title("Content"),
            );
            frame.render_widget(empty, area);
        }
    }

    /// Render delete confirmation dialog
    fn render_delete_confirmation(frame: &mut Frame, area: Rect, item_path: &str) {
        use ratatui::widgets::Clear;

        // Calculate centered popup area
        let popup_area = centered_rect(50, 30, area);

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
            " Confirm Delete ",
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
        let message = vec![
            Line::from(""),
            Line::from(Span::styled(
                "Are you sure you want to delete:",
                Style::default().fg(Color::White),
            )),
            Line::from(""),
            Line::from(Span::styled(
                item_path,
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

        // Footer
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

        // Handle delete confirmation first
        if self.delete_confirmation.is_some() {
            return self.handle_delete_confirmation(key);
        }

        // Handle create secret input mode
        if self.input_mode == InputMode::CreateSecret {
            return self.handle_create_secret_input(key, ctx);
        }

        // While the filter is being typed, every key belongs to it. Without
        // this, letters such as q, p and r fire the global shortcuts below.
        if self.focused_pane == FocusedPane::Navigation && self.navigation_pane.captures_input() {
            return self.navigation_pane.handle_key(key, ctx);
        }

        // If edit view is active and focused, let it handle Tab for mode switching
        if let Some(edit_view) = &self.edit_view
            && self.focused_pane == FocusedPane::Content
        {
            // When editing a field, EditView captures ALL input
            if edit_view.is_editing() {
                return self.edit_view.as_mut().unwrap().handle_key(key, ctx);
            }
            // When not editing but EditView is focused, let it handle Tab to start editing
            if key.code == KeyCode::Tab {
                return self.edit_view.as_mut().unwrap().handle_key(key, ctx);
            }
        }

        // Clear errors with Esc
        if key.code == KeyCode::Esc && self.error_message.is_some() {
            self.error_message = None;
            self.is_loading = false;
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
                self.focused_pane = match self.focused_pane {
                    FocusedPane::Navigation => {
                        if self.detail_view.is_some() || self.edit_view.is_some() {
                            FocusedPane::Content
                        } else {
                            FocusedPane::Navigation
                        }
                    }
                    FocusedPane::Content => FocusedPane::Navigation,
                };
                return Ok(None);
            }
            // Switch focus with arrow keys (left/right)
            (KeyCode::Left, KeyModifiers::NONE) | (KeyCode::Char('h'), KeyModifiers::NONE) => {
                if self.focused_pane == FocusedPane::Content {
                    self.focused_pane = FocusedPane::Navigation;
                    return Ok(None);
                }
            }
            (KeyCode::Right, KeyModifiers::NONE) | (KeyCode::Char('l'), KeyModifiers::NONE)
                if self.focused_pane == FocusedPane::Navigation
                    && (self.detail_view.is_some() || self.edit_view.is_some()) =>
            {
                self.focused_pane = FocusedPane::Content;
                return Ok(None);
            }
            _ => {}
        }

        // Route to focused pane
        match self.focused_pane {
            FocusedPane::Navigation => self.navigation_pane.handle_key(key, ctx),
            FocusedPane::Content => {
                // Route to edit or detail view
                if let Some(view) = &mut self.edit_view {
                    view.handle_key(key, ctx)
                } else if let Some(view) = &mut self.detail_view {
                    view.handle_key(key, ctx)
                } else {
                    Ok(None)
                }
            }
        }
    }

    fn update(&mut self, action: Action, ctx: &mut ComponentContext) -> Result<Option<Action>> {
        match &action {
            // Focus management
            Action::FocusPane { pane } => {
                self.focused_pane = *pane;
            }

            // Secret viewing
            Action::ViewSecret { .. } => {
                self.is_loading = true;
                // For now, return the action for Root to handle async loading
                // In future, MainScreen could use AsyncManager directly
                return Ok(Some(action));
            }

            Action::SecretLoaded {
                path,
                data,
                metadata,
            } => {
                // Save selected version index if reloading the same secret
                let saved_version_index = if let Some(existing_detail) = &self.detail_view {
                    if existing_detail.path() == path {
                        Some(existing_detail.selected_version_index())
                    } else {
                        None
                    }
                } else {
                    None
                };

                // Create detail view with loaded data
                let mut detail_view = DetailViewComponent::new(data.clone(), path.clone());

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

                    // Restore selected version index if we saved one
                    if let Some(index) = saved_version_index {
                        detail_view.set_selected_version_index(index);
                    }
                }

                // The edit view is deliberately left alone: a preview that
                // completes late must not throw away an edit in progress.
                // It is closed by `SecretSaved`, or by the user.
                self.detail_view = Some(detail_view);
                self.error_message = None;
                self.is_loading = false;
            }

            // The write was confirmed by Vault: the edit is done
            Action::SecretSaved { path } => {
                if self
                    .edit_view
                    .as_ref()
                    .is_some_and(|view| view.path() == path)
                {
                    self.edit_view = None;
                }
            }

            // Edit secret
            Action::EditSecret => {
                if let Some(detail_view) = &self.detail_view {
                    let path = detail_view.path().to_string();
                    let data = detail_view.secret_data();
                    self.edit_view = Some(EditViewComponent::new(path, data));
                    // Keep detail_view so we can restore it on cancel
                    self.focused_pane = FocusedPane::Content;
                }
            }

            // Cancel edit is handled by EditView itself
            // If EditView is not editing, it emits ScreenBack to close
            Action::CancelEdit => {
                // Pass to EditView, it will handle cancel properly
                if let Some(view) = &mut self.edit_view
                    && let Some(action) = view.update(action.clone(), ctx)?
                {
                    // EditView wants to close (returned ScreenBack)
                    // Process it here to close edit view and show detail view
                    if matches!(action, Action::ScreenBack) {
                        self.edit_view = None;
                        // Keep detail_view visible, focus on it
                        if self.detail_view.is_some() {
                            self.focused_pane = FocusedPane::Content;
                        } else {
                            self.focused_pane = FocusedPane::Navigation;
                        }
                    }
                }
            }

            // Copy selected value from detail view
            Action::CopySelectedValue => {
                if let Some(detail_view) = &self.detail_view
                    && let Some(value) = detail_view.selected_value()
                {
                    return Ok(Some(Action::CopyToClipboard {
                        text: value.to_string(),
                    }));
                }
            }

            // View specific secret version - pass to Root to spawn async load
            Action::ViewSecretVersion { .. } => {
                self.is_loading = true;
                return Ok(Some(action));
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
                if let Some(detail_view) = &mut self.detail_view
                    && detail_view.path() == path
                {
                    detail_view.load_version(*version, data.clone());
                }
                self.is_loading = false;
            }

            // Create new version from historical version - pass to Root to load
            Action::CreateFromVersion { .. } => {
                self.is_loading = true;
                return Ok(Some(action));
            }

            // Version loaded for editing (create from old version)
            Action::VersionLoadedForEdit { path, data } => {
                // Only open the editor if the user is still on that secret
                // and has not started another edit in the meantime.
                let still_viewing = self
                    .detail_view
                    .as_ref()
                    .is_some_and(|view| view.path() == path);
                if still_viewing && self.edit_view.is_none() {
                    self.edit_view = Some(EditViewComponent::new(path.clone(), data.clone()));
                    self.focused_pane = FocusedPane::Content;
                }
                self.is_loading = false;
            }

            // Create secret
            Action::CreateSecret { name } => {
                self.edit_view = Some(EditViewComponent::new_secret(name.clone()));
                self.detail_view = None;
                self.focused_pane = FocusedPane::Content;
            }

            // Save secret - pass through to Root which will handle async save
            Action::SaveSecret { .. } => {
                // Root will handle the async save operation
            }

            Action::ShowCreateSecretInput => {
                self.input_mode = InputMode::CreateSecret;
                self.new_secret_name.clear();
            }

            Action::HideCreateSecretInput => {
                self.input_mode = InputMode::Normal;
                self.new_secret_name.clear();
            }

            // Delete confirmation
            Action::ShowDeleteConfirmation { item_name } => {
                self.delete_confirmation = Some(item_name.clone());
            }

            Action::HideConfirmation => {
                self.delete_confirmation = None;
            }

            // Status/error messages
            Action::SetStatus { message } => {
                self.status_message = Some(message.clone());
            }

            Action::ClearStatus => {
                self.status_message = None;
            }

            Action::SetError { message } => {
                self.error_message = Some(message.clone());
                self.is_loading = false;
            }

            Action::ClearError => {
                self.error_message = None;
            }

            Action::SetLoading { loading } => {
                self.is_loading = *loading;
            }

            // Filter mode tracking
            Action::StartFilter => {
                self.input_mode = InputMode::Filter;
            }

            Action::ConfirmFilter | Action::CancelFilter => {
                self.input_mode = InputMode::Normal;
            }

            // Navigation actions - propagate to navigation pane
            Action::Refresh
            | Action::StateUpdated
            | Action::NavigateTo { .. }
            | Action::NavigateUp
            | Action::UpdateFilter { .. } => {
                self.navigation_pane.update(action.clone(), ctx)?;
            }

            // Close detail/edit views
            Action::ScreenBack => {
                if self.edit_view.is_some() {
                    self.edit_view = None;
                    self.focused_pane = FocusedPane::Navigation;
                } else if self.detail_view.is_some() {
                    self.detail_view = None;
                    self.focused_pane = FocusedPane::Navigation;
                }
            }

            _ => {}
        }

        // Update child components (but don't broadcast everything)
        // Navigation pane only needs navigation-related actions
        match &action {
            Action::Refresh
            | Action::StateUpdated
            | Action::NavigateTo { .. }
            | Action::NavigateUp
            | Action::HistoryBack
            | Action::HistoryForward
            | Action::UpdateFilter { .. }
            | Action::CancelFilter
            | Action::ConfirmFilter
            | Action::StartFilter
            | Action::SelectNext
            | Action::SelectPrevious => {
                self.navigation_pane.update(action.clone(), ctx)?;
            }
            _ => {}
        }

        // Content views get their relevant actions
        if let Some(view) = &mut self.detail_view {
            view.update(action.clone(), ctx)?;
        }

        if let Some(view) = &mut self.edit_view {
            view.update(action, ctx)?;
        }

        Ok(None)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect, ctx: &RenderContext) {
        // Main layout: header, content, footer
        let main_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Header
                Constraint::Min(0),    // Content (split view)
                Constraint::Length(3), // Footer
            ])
            .split(area);

        // Render header
        let current_path = ctx.app.state().current_path();
        Self::render_header(frame, main_chunks[0], &current_path);

        // Split content area into two columns
        let content_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(35), // Left: Navigation
                Constraint::Percentage(65), // Right: Content
            ])
            .split(main_chunks[1]);

        // Render navigation pane (left)
        let nav_ctx = RenderContext::new(
            ctx.app,
            ctx.focus,
            self.focused_pane == FocusedPane::Navigation,
            content_chunks[0],
        );
        self.navigation_pane
            .draw(frame, content_chunks[0], &nav_ctx);

        // Render content pane (right)
        let content_ctx = RenderContext::new(
            ctx.app,
            ctx.focus,
            self.focused_pane == FocusedPane::Content,
            content_chunks[1],
        );
        let error_message = self.error_message.clone();
        self.render_content_pane(frame, content_chunks[1], &content_ctx, &error_message);

        // Render footer
        // Check if navigation pane has active filter
        let filter_active = false; // TODO: expose from NavigationPane
        Self::render_footer(
            frame,
            main_chunks[2],
            self.focused_pane,
            self.input_mode,
            filter_active,
            &self.new_secret_name,
            &self.status_message,
        );

        // Render delete confirmation dialog if active
        if let Some(ref item_path) = self.delete_confirmation {
            Self::render_delete_confirmation(frame, area, item_path);
        }
    }

    fn captures_input(&self) -> bool {
        // Capture input when in create secret mode or delete confirmation
        self.input_mode == InputMode::CreateSecret || self.delete_confirmation.is_some()
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use crate::config::Config;
    use crate::tui::context::FocusState;
    use crate::vault::SecretData;
    use serde_json::json;
    use tokio::sync::mpsc::unbounded_channel;

    fn test_app() -> App {
        let config = Config {
            vault_addr: "http://127.0.0.1:1".to_string(),
            vault_token: "test-token".to_string(),
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

    /// Run `body` with a screen and the context components expect
    fn with_screen(body: impl FnOnce(&mut MainScreen, &mut ComponentContext)) {
        let app = test_app();
        let focus = FocusState::new();
        let (tx, _rx) = unbounded_channel();
        let mut ctx = ComponentContext::new(&app, &focus, &tx);
        let mut screen = MainScreen::new();
        body(&mut screen, &mut ctx);
    }

    #[test]
    fn filter_typing_does_not_trigger_global_shortcuts() {
        // Regression: with the filter open, "q" quit the app, "p" opened the
        // policy screen, "r" refreshed, and Tab moved focus away.
        with_screen(|screen, ctx| {
            screen.update(loaded("/a", json!({"k": "v"})), ctx).unwrap();

            let action = screen.handle_key(key(KeyCode::Char('/')), ctx).unwrap();
            assert!(matches!(action, Some(Action::StartFilter)));
            screen.update(Action::StartFilter, ctx).unwrap();

            let mut typed = String::new();
            for c in ['q', 'p', 'r', '?', '~', 'h', 'l'] {
                typed.push(c);
                match screen.handle_key(key(KeyCode::Char(c)), ctx).unwrap() {
                    Some(Action::UpdateFilter { text }) => assert_eq!(text, typed),
                    other => panic!("'{c}' escaped the filter as {other:?}"),
                }
            }

            assert!(screen.handle_key(key(KeyCode::Tab), ctx).unwrap().is_none());
            assert_eq!(screen.focused_pane, FocusedPane::Navigation);

            // Leaving the filter gives the shortcuts back
            let action = screen.handle_key(key(KeyCode::Esc), ctx).unwrap();
            assert!(matches!(action, Some(Action::CancelFilter)));
            screen.update(Action::CancelFilter, ctx).unwrap();
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
            let failed = Action::SetError {
                message: "Failed to save secret: Access denied".to_string(),
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
    fn version_result_for_another_secret_is_ignored() {
        // Regression: SecretVersionLoaded carried no path, so an old version
        // of A could be loaded into B's view and then saved over B.
        with_screen(|screen, ctx| {
            screen
                .update(loaded("/b", json!({"b": "current"})), ctx)
                .unwrap();

            let stale = Action::SecretVersionLoaded {
                path: "/a".to_string(),
                version: 1,
                data: secret(json!({"a": "old"})),
            };
            screen.update(stale, ctx).unwrap();
            let detail = screen.detail_view.as_ref().unwrap();
            assert_eq!(detail.secret_data(), secret(json!({"b": "current"})));

            let matching = Action::SecretVersionLoaded {
                path: "/b".to_string(),
                version: 1,
                data: secret(json!({"b": "old"})),
            };
            screen.update(matching, ctx).unwrap();
            let detail = screen.detail_view.as_ref().unwrap();
            assert_eq!(detail.secret_data(), secret(json!({"b": "old"})));
        });
    }

    #[test]
    fn version_for_edit_opens_only_for_the_secret_on_screen() {
        with_screen(|screen, ctx| {
            screen
                .update(loaded("/b", json!({"b": "current"})), ctx)
                .unwrap();

            let stale = Action::VersionLoadedForEdit {
                path: "/a".to_string(),
                data: secret(json!({"a": "old"})),
            };
            screen.update(stale, ctx).unwrap();
            assert!(screen.edit_view.is_none());

            let matching = Action::VersionLoadedForEdit {
                path: "/b".to_string(),
                data: secret(json!({"b": "old"})),
            };
            screen.update(matching, ctx).unwrap();
            assert_eq!(screen.edit_view.as_ref().unwrap().path(), "/b");
        });
    }
}
