use anyhow::Result;
use crossterm::event::KeyEvent;
use ratatui::Frame;
use std::time::{Duration, Instant};

use crate::app::App;
use crate::app::navigation::parent_path;
use crate::tui::action::{Action, SelectionSource};
use crate::tui::async_manager::AsyncManager;
use crate::tui::component::Component;
use crate::tui::components::{HelpOverlay, LogScreen, MainScreen, PolicyScreen};
use crate::tui::context::{ActionSender, ComponentContext, FocusState, RenderContext};

/// How long the cursor has to rest on a secret before it is loaded.
///
/// Every load is a read of the secret in Vault's audit log, so scrolling
/// through a folder must not read everything on the way. The first step after
/// a pause loads at once; while the cursor keeps moving, only the secret it
/// stops on is loaded.
const PREVIEW_DEBOUNCE: Duration = Duration::from_millis(200);

/// Active screen in the application
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveScreen {
    /// Main screen (navigation + detail/edit views)
    /// Still handled by old Tui struct during migration
    Main,
    /// Log viewer screen
    Logs,
    /// Policy viewer screen (not yet implemented)
    Policy,
}

/// Root component managing the entire TUI
///
/// This is the top-level component that:
/// - Manages screen transitions
/// - Handles global overlays (Help)
/// - Routes events to active screen
/// - Processes async action completions
///
/// Phase 7: Manages all screens including MainScreen with component architecture.
pub struct Root {
    /// Current active screen
    active_screen: ActiveScreen,

    /// Main screen component (navigation + detail/edit split view)
    main_screen: Box<dyn Component>,

    /// Help overlay (always present, visibility controlled internally)
    help_overlay: Box<dyn Component>,

    /// Log screen component
    log_screen: Box<dyn Component>,

    /// Policy screen component
    policy_screen: Box<dyn Component>,

    /// Focus state
    focus: FocusState,

    /// Async operation manager
    async_manager: AsyncManager,

    /// Whether application should quit
    should_quit: bool,

    /// Pending clipboard text (to be handled by Tui)
    pending_clipboard: Option<String>,

    /// Path of the most recently requested secret. Loads are not cancelled,
    /// so results can arrive out of order; only the latest request is shown.
    /// `None` when no secret is selected.
    latest_secret_request: Option<String>,

    /// Selected secret whose load is waiting for the cursor to rest, and
    /// when it is due
    pending_preview: Option<(String, Instant)>,

    /// When the selection last changed
    last_selection_change: Option<Instant>,
}

impl Root {
    /// Create a new root component
    pub fn new(runtime: tokio::runtime::Handle) -> Self {
        Self {
            active_screen: ActiveScreen::Main,
            main_screen: Box::new(MainScreen::new()),
            help_overlay: Box::new(HelpOverlay::new()),
            log_screen: Box::new(LogScreen::new()),
            policy_screen: Box::new(PolicyScreen::new()),
            focus: FocusState::new(),
            async_manager: AsyncManager::new(runtime),
            should_quit: false,
            pending_clipboard: None,
            latest_secret_request: None,
            pending_preview: None,
            last_selection_change: None,
        }
    }

    /// Initialize all components
    pub fn init(&mut self, app: &App) -> Result<()> {
        let action_tx = self.action_sender();
        let mut ctx = ComponentContext::new(app, &self.focus, &action_tx);

        // Initialize all screen components
        self.main_screen.init(&mut ctx)?;
        self.help_overlay.init(&mut ctx)?;
        self.log_screen.init(&mut ctx)?;
        self.policy_screen.init(&mut ctx)?;

        Ok(())
    }

    /// Get action sender for components
    pub fn action_sender(&self) -> ActionSender {
        self.async_manager.action_sender()
    }

    /// Check if application should quit
    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    /// Take pending clipboard text (returns and clears it)
    pub fn take_pending_clipboard(&mut self) -> Option<String> {
        self.pending_clipboard.take()
    }

    /// Process a keyboard event
    ///
    /// Returns an optional action to be processed. This allows the old Tui
    /// to handle the action if Root doesn't consume it.
    pub fn handle_key_event(&mut self, key: KeyEvent, app: &App) -> Result<Option<Action>> {
        let action_tx = self.action_sender();
        let ctx = ComponentContext::new(app, &self.focus, &action_tx);

        // Help overlay captures input when visible
        if self.help_overlay.captures_input() {
            return self.help_overlay.handle_key(key, &ctx);
        }

        // Route to active screen
        match self.active_screen {
            ActiveScreen::Main => self.main_screen.handle_key(key, &ctx),
            ActiveScreen::Logs => self.log_screen.handle_key(key, &ctx),
            ActiveScreen::Policy => self.policy_screen.handle_key(key, &ctx),
        }
    }

    /// Route pasted text to whatever text input has focus
    pub fn handle_paste(&mut self, text: &str, app: &App) -> Result<Option<Action>> {
        let action_tx = self.action_sender();
        let ctx = ComponentContext::new(app, &self.focus, &action_tx);

        // A paste is never a command: only the main screen has text inputs
        if self.help_overlay.captures_input() || self.active_screen != ActiveScreen::Main {
            return Ok(None);
        }
        self.main_screen.handle_paste(text, &ctx)
    }

    /// Periodic work, called once per event-loop iteration
    pub fn tick(&mut self, app: &mut App) -> Result<()> {
        self.tick_at(app, Instant::now())
    }

    fn tick_at(&mut self, app: &mut App, now: Instant) -> Result<()> {
        // The cursor has rested long enough: load the secret under it
        if self
            .pending_preview
            .as_ref()
            .is_some_and(|(_, due)| now >= *due)
            && let Some((path, _)) = self.pending_preview.take()
        {
            self.spawn_load_secret(app, path);
        }

        // Busy means a request is in flight, whichever one it is
        let busy = self.async_manager.has_pending();
        let action_tx = self.action_sender();
        let mut ctx = ComponentContext::new(app, &self.focus, &action_tx);
        self.main_screen
            .update(Action::SetLoading { loading: busy }, &mut ctx)?;

        Ok(())
    }

    /// The navigation cursor rests on a different item
    fn selection_changed(
        &mut self,
        app: &App,
        secret: Option<String>,
        source: SelectionSource,
        now: Instant,
    ) {
        let was_resting = self
            .last_selection_change
            .is_none_or(|last| now.duration_since(last) >= PREVIEW_DEBOUNCE);
        self.last_selection_change = Some(now);
        self.pending_preview = None;

        match secret {
            // A folder or `..`: nothing to load, and anything still in
            // flight is no longer wanted
            None => self.latest_secret_request = None,
            // Already requested (for example, just saved and reloaded)
            Some(path) if self.latest_secret_request.as_deref() == Some(path.as_str()) => {}
            Some(path) => {
                self.latest_secret_request = Some(path.clone());
                if source == SelectionSource::Cursor && was_resting {
                    self.spawn_load_secret(app, path);
                } else {
                    self.pending_preview = Some((path, now + PREVIEW_DEBOUNCE));
                }
            }
        }
    }

    /// Process an action
    ///
    /// This updates component state based on actions. Some actions trigger
    /// screen transitions or other side effects, including async operations.
    pub fn process_action(&mut self, action: Action, app: &mut App) -> Result<()> {
        let action_tx = self.action_sender();
        let mut ctx = ComponentContext::new(app, &self.focus, &action_tx);

        match &action {
            // The cursor moved to another item: the content pane follows,
            // and the secret is loaded once the cursor rests
            Action::SelectionChanged { secret, source } => {
                let (secret, source) = (secret.clone(), *source);
                self.main_screen.update(action, &mut ctx)?;
                self.selection_changed(app, secret, source, Instant::now());
                return Ok(());
            }

            // Enter on a secret: focus it, loading it now if needed
            Action::OpenSecret { path } => {
                let path = path.clone();
                self.main_screen.update(action, &mut ctx)?;

                let waiting = self
                    .pending_preview
                    .as_ref()
                    .is_some_and(|(pending, _)| *pending == path);
                if waiting || self.latest_secret_request.as_deref() != Some(path.as_str()) {
                    self.pending_preview = None;
                    self.latest_secret_request = Some(path.clone());
                    self.spawn_load_secret(app, path);
                }
                return Ok(());
            }

            // Load a secret now, whatever the cursor is doing
            Action::ViewSecret { path } => {
                self.pending_preview = None;
                self.latest_secret_request = Some(path.clone());
                self.spawn_load_secret(app, path.clone());
                return Ok(());
            }

            Action::LoadPolicies => {
                self.policy_screen
                    .update(Action::SetLoading { loading: true }, &mut ctx)?;
                self.spawn_load_policies(app);
                return Ok(());
            }

            Action::ViewPolicy { name } => {
                self.spawn_load_policy(app, name.clone());
                return Ok(());
            }

            Action::SaveSecret { path, data } => {
                self.spawn_save_secret(app, path.clone(), data.clone());
                return Ok(());
            }

            Action::ViewSecretVersion { path, version } => {
                self.spawn_load_secret_version(app, path.clone(), *version);
                return Ok(());
            }

            Action::CreateFromVersion { path, version } => {
                self.spawn_load_version_for_edit(app, path.clone(), *version);
                return Ok(());
            }

            Action::DeleteSecret { path } => {
                self.spawn_delete_secret(app, path.clone());
                return Ok(());
            }

            Action::NavigateTo { path } => {
                self.spawn_navigate_to(app, path.clone());
                return Ok(());
            }

            Action::NavigateUp => {
                let current = app.state().current_path();
                if current != "/" {
                    self.spawn_navigate_to(app, parent_path(&current));
                }
                return Ok(());
            }

            Action::Refresh => {
                self.spawn_refresh(app);
                return Ok(());
            }

            Action::HistoryBack => {
                if let Some(path) = app.state().go_back() {
                    self.spawn_navigate_to(app, path);
                }
                return Ok(());
            }

            Action::HistoryForward => {
                if let Some(path) = app.state().go_forward() {
                    self.spawn_navigate_to(app, path);
                }
                return Ok(());
            }

            Action::CopyToClipboard { text } => {
                // Store clipboard text for Tui to handle (Tui has access to clipboard)
                self.pending_clipboard = Some(text.clone());
                return Ok(());
            }

            Action::Batch { actions } => {
                // Process multiple actions in sequence
                for action in actions {
                    self.process_action(action.clone(), app)?;
                }
                return Ok(());
            }

            // Screen transitions
            Action::ShowLogScreen => {
                self.active_screen = ActiveScreen::Logs;
            }
            Action::ShowMainScreen | Action::ScreenBack => {
                self.active_screen = ActiveScreen::Main;
            }
            Action::ShowPolicyScreen => {
                self.active_screen = ActiveScreen::Policy;
            }

            // Quit
            Action::Quit => {
                self.should_quit = true;
            }

            // Help overlay actions
            Action::ShowHelp | Action::HideHelp | Action::ToggleHelp => {
                self.help_overlay.update(action.clone(), &mut ctx)?;
            }

            // A deleted secret is no longer requested or shown
            Action::SecretDeleted { path }
                if self.latest_secret_request.as_deref() == Some(path.as_str()) =>
            {
                self.latest_secret_request = None;
                self.pending_preview = None;
                self.main_screen.update(action, &mut ctx)?;
            }

            // A load that was overtaken by a newer request: drop it, so the
            // content pane never shows a secret other than the last one asked for
            Action::SecretLoaded { path, .. }
                if self.latest_secret_request.as_deref() != Some(path.as_str()) =>
            {
                tracing::debug!("Ignoring stale secret load");
            }

            // A failed load that was overtaken is of no interest either
            Action::SecretLoadFailed { path, .. }
                if self.latest_secret_request.as_deref() != Some(path.as_str()) =>
            {
                tracing::debug!("Ignoring stale secret load failure");
            }

            // The secret on screen could not be loaded. Forget the request,
            // so that Enter on it tries again.
            Action::SecretLoadFailed { .. } => {
                self.latest_secret_request = None;
                self.main_screen.update(action, &mut ctx)?;
            }

            // Errors belong to the screen the user is looking at
            Action::SetError { .. } if self.active_screen == ActiveScreen::Policy => {
                self.policy_screen.update(action, &mut ctx)?;
            }

            // Main screen actions
            Action::SecretLoaded { .. }
            | Action::SecretSaved { .. }
            | Action::SecretDeleted { .. }
            | Action::EditSecret
            | Action::CreateSecret { .. }
            | Action::ShowCreateSecretInput
            | Action::HideCreateSecretInput
            | Action::ShowDeleteConfirmation { .. }
            | Action::HideConfirmation
            | Action::SetStatus { .. }
            | Action::ClearStatus
            | Action::SetError { .. }
            | Action::ClearError
            | Action::SetLoading { .. }
            | Action::StateUpdated
            | Action::FocusPane { .. }
            | Action::SecretVersionLoaded { .. }
            | Action::VersionLoadedForEdit { .. }
            | Action::CancelEdit => {
                // Process in MainScreen and handle any returned actions
                if let Some(returned_action) = self.main_screen.update(action, &mut ctx)? {
                    // Recursively process returned actions (like CopyToClipboard)
                    self.process_action(returned_action, app)?;
                }
            }

            // Log screen actions (when active)
            Action::ScrollLogs { .. } | Action::ToggleAutoScroll | Action::ClearLogs => {
                if self.active_screen == ActiveScreen::Logs {
                    self.log_screen.update(action, &mut ctx)?;
                }
            }

            // Policy screen actions
            Action::PoliciesLoaded { .. } | Action::PolicyLoaded { .. } => {
                self.policy_screen.update(action, &mut ctx)?;
            }

            Action::ScrollPolicy { .. } if self.active_screen == ActiveScreen::Policy => {
                self.policy_screen.update(action, &mut ctx)?;
            }

            _ => {
                // Other actions not handled by Root yet
            }
        }

        Ok(())
    }

    /// Poll for completed async operations
    ///
    /// Returns actions from completed async operations that should be processed.
    pub fn poll_async_completions(&mut self) -> Vec<Action> {
        self.async_manager.poll_completions()
    }

    // ═══════════════════════════════════════════════════════════════
    // ASYNC OPERATION SPAWNING
    // ═══════════════════════════════════════════════════════════════

    /// Spawn async operation to load a secret
    fn spawn_load_secret(&mut self, app: &App, path: String) {
        let vault_client = app.vault_client().clone();
        let failed_path = path.clone();

        self.async_manager.spawn(
            async move {
                // Load secret data
                let secret_data = vault_client.kv().get(&path).await?;

                // Keep every field with its JSON type. Dropping non-string
                // values here would delete them from Vault on the next save.
                let data = secret_data.data;

                // Load metadata
                let metadata = match vault_client.kv().get_metadata(&path).await {
                    Ok(meta) => {
                        use crate::tui::action::{SecretMetadata, VersionInfo};

                        let version_list = meta.version_list();
                        let versions: Vec<VersionInfo> = version_list
                            .into_iter()
                            .map(|v| {
                                let version_meta = meta.versions.get(&v.to_string());
                                VersionInfo {
                                    version: v,
                                    created_time: version_meta
                                        .map(|m| m.created_time.clone())
                                        .unwrap_or_default(),
                                    deleted: meta.is_version_deleted(v),
                                }
                            })
                            .collect();

                        Some(SecretMetadata {
                            created_time: meta.created_time,
                            updated_time: meta.updated_time,
                            current_version: meta.current_version,
                            versions,
                        })
                    }
                    Err(e) => {
                        tracing::warn!("Failed to load metadata for {}: {}", path, e);
                        None
                    }
                };

                Ok(Action::SecretLoaded {
                    path: path.clone(),
                    data,
                    metadata,
                })
            },
            move |e| Action::SecretLoadFailed {
                path: failed_path.clone(),
                message: format!("Failed to load secret: {}", e),
            },
        );
    }

    /// Spawn async operation to navigate to a path
    fn spawn_navigate_to(&mut self, app: &App, path: String) {
        let nav = app.navigation().clone();

        self.async_manager.spawn(
            async move {
                nav.navigate_to(path.clone()).await?;
                // Emit action to update UI after state has been updated
                Ok(Action::StateUpdated)
            },
            |e| Action::SetError {
                message: format!("Navigation failed: {}", e),
            },
        );
    }

    /// Spawn async operation to refresh current directory from Vault
    fn spawn_refresh(&mut self, app: &App) {
        let nav = app.navigation().clone();

        self.async_manager.spawn(
            async move {
                nav.refresh().await?;
                // Emit action to update UI after state has been updated
                Ok(Action::StateUpdated)
            },
            |e| Action::SetError {
                message: format!("Failed to refresh: {}", e),
            },
        );
    }

    /// Spawn async operation to delete a secret
    fn spawn_delete_secret(&mut self, app: &App, path: String) {
        let secret_service = app.secret_service().clone();

        self.async_manager.spawn(
            async move {
                secret_service.delete(&path).await?;
                Ok(Action::Batch {
                    actions: vec![
                        Action::SecretDeleted { path: path.clone() },
                        Action::SetStatus {
                            message: format!("Deleted: {}", path),
                        },
                        Action::Refresh,
                        Action::HideConfirmation,
                    ],
                })
            },
            |e| Action::SetError {
                message: format!("Delete failed: {}", e),
            },
        );
    }

    /// Spawn async operation to load policies
    fn spawn_load_policies(&mut self, app: &App) {
        let policy_service = app.policy_service().clone();

        self.async_manager.spawn(
            async move {
                let policies = policy_service.list().await?;
                Ok(Action::PoliciesLoaded { policies })
            },
            |e| Action::SetError {
                message: format!("Failed to load policies: {}", e),
            },
        );
    }

    /// Spawn async operation to load a policy
    fn spawn_load_policy(&mut self, app: &App, name: String) {
        let policy_service = app.policy_service().clone();
        let name_for_error = name.clone();

        self.async_manager.spawn(
            async move {
                let content = policy_service.read(&name).await?;
                Ok(Action::PolicyLoaded {
                    name: name.clone(),
                    content,
                })
            },
            move |e| Action::SetError {
                message: format!("Failed to load policy '{}': {}", name_for_error, e),
            },
        );
    }

    /// Spawn async operation to save a secret
    fn spawn_save_secret(&mut self, app: &App, path: String, data: crate::vault::SecretData) {
        let secret_service = app.secret_service().clone();

        self.async_manager.spawn(
            async move {
                // Save the secret
                secret_service.save(&path, data).await?;

                Ok(Action::Batch {
                    actions: vec![
                        Action::SecretSaved { path: path.clone() },
                        Action::SetStatus {
                            message: format!("Saved: {}", path),
                        },
                        Action::Refresh, // Refresh navigation list to show new secret
                        // Reload secret to show new version in history
                        Action::ViewSecret { path: path.clone() },
                    ],
                })
            },
            |e| Action::SetError {
                message: format!("Failed to save secret: {}", e),
            },
        );
    }

    /// Spawn async operation to load a specific version of a secret
    fn spawn_load_secret_version(&mut self, app: &App, path: String, version: u64) {
        let secret_service = app.secret_service().clone();

        self.async_manager.spawn(
            async move {
                // Load the specific version
                let data = secret_service.get_version(&path, version).await?;

                Ok(Action::SecretVersionLoaded {
                    path: path.clone(),
                    version,
                    data,
                })
            },
            |e| Action::SetError {
                message: format!("Failed to load secret version: {}", e),
            },
        );
    }

    /// Spawn async operation to load a version for editing (create from old)
    fn spawn_load_version_for_edit(&mut self, app: &App, path: String, version: u64) {
        let secret_service = app.secret_service().clone();

        self.async_manager.spawn(
            async move {
                // Load the specific version
                let data = secret_service.get_version(&path, version).await?;

                Ok(Action::VersionLoadedForEdit {
                    path: path.clone(),
                    version,
                    data,
                })
            },
            |e| Action::SetError {
                message: format!("Failed to load version for editing: {}", e),
            },
        );
    }

    /// Render the current screen and overlays
    pub fn render(&mut self, frame: &mut Frame, app: &App) {
        let area = frame.area();
        let ctx = RenderContext::new(app, &self.focus, true, area);

        // Render active screen
        match self.active_screen {
            ActiveScreen::Main => {
                self.main_screen.draw(frame, area, &ctx);
            }
            ActiveScreen::Logs => {
                self.log_screen.draw(frame, area, &ctx);
            }
            ActiveScreen::Policy => {
                self.policy_screen.draw(frame, area, &ctx);
            }
        }

        // Always render help overlay on top (if visible)
        self.help_overlay.draw(frame, area, &ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use ratatui::{Terminal, backend::TestBackend};

    fn test_app() -> App {
        let config = Config {
            vault_addr: "http://127.0.0.1:1".to_string(),
            vault_token: "test-token".to_string(),
            ..Config::default()
        };
        App::new(config, "/".to_string()).unwrap()
    }

    fn test_root(app: &App) -> Root {
        let mut root = Root::new(tokio::runtime::Handle::current());
        root.init(app).unwrap();
        root
    }

    fn loaded(path: &str, field: &str) -> Action {
        Action::SecretLoaded {
            path: path.to_string(),
            data: serde_json::from_value(serde_json::json!({ field: "value" })).unwrap(),
            metadata: None,
        }
    }

    fn view(path: &str) -> Action {
        Action::ViewSecret {
            path: path.to_string(),
        }
    }

    fn failed(path: &str) -> Action {
        Action::SecretLoadFailed {
            path: path.to_string(),
            message: format!("Failed to load secret: no access to {path}"),
        }
    }

    fn pending(root: &Root) -> Option<&str> {
        root.pending_preview.as_ref().map(|(path, _)| path.as_str())
    }

    /// Requests started so far
    fn spawned(root: &Root) -> u64 {
        root.async_manager.spawned_count()
    }

    fn cursor_on(root: &mut Root, app: &App, path: &str, at: Instant) {
        root.selection_changed(app, Some(path.to_string()), SelectionSource::Cursor, at);
    }

    fn screen_text(root: &mut Root, app: &App) -> String {
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        terminal.draw(|frame| root.render(frame, app)).unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    #[tokio::test]
    async fn stale_secret_load_is_dropped() {
        // Regression: loads are not cancelled, and the last one to *arrive*
        // used to win, so the pane could show A while the cursor was on B.
        let mut app = test_app();
        let mut root = test_root(&app);

        // The user moves from A to B before A's load completes
        root.process_action(view("/app/a"), &mut app).unwrap();
        root.process_action(view("/app/b"), &mut app).unwrap();

        // B's result arrives first, A's arrives late
        root.process_action(loaded("/app/b", "field_b"), &mut app)
            .unwrap();
        root.process_action(loaded("/app/a", "field_a"), &mut app)
            .unwrap();

        let text = screen_text(&mut root, &app);
        assert!(text.contains(" /app/b "), "{text}");
        assert!(text.contains("field_b"));
        assert!(!text.contains("field_a"));
    }

    #[tokio::test]
    async fn requested_secret_load_is_shown() {
        let mut app = test_app();
        let mut root = test_root(&app);

        root.process_action(view("/app/a"), &mut app).unwrap();
        root.process_action(loaded("/app/a", "field_a"), &mut app)
            .unwrap();

        let text = screen_text(&mut root, &app);
        assert!(text.contains(" /app/a "), "{text}");
        assert!(text.contains("field_a"));
    }

    #[tokio::test]
    async fn scrolling_reads_only_where_the_cursor_starts_and_stops() {
        // Regression: every cursor step read the secret under it, so holding
        // `j` read (and audit-logged) every secret in the folder.
        let mut app = test_app();
        let mut root = test_root(&app);
        let start = Instant::now();
        let step = Duration::from_millis(30);

        // The first step after a pause loads at once
        cursor_on(&mut root, &app, "/s00", start);
        assert_eq!(spawned(&root), 1);
        assert_eq!(pending(&root), None);

        // Twenty more in quick succession: none of them is read
        let mut now = start;
        for i in 1..=20 {
            now += step;
            cursor_on(&mut root, &app, &format!("/s{i:02}"), now);
            root.tick_at(&mut app, now).unwrap();
        }
        assert_eq!(spawned(&root), 1);
        assert_eq!(pending(&root), Some("/s20"));
        assert_eq!(root.latest_secret_request.as_deref(), Some("/s20"));

        // Not yet: the cursor has rested for less than the debounce time
        root.tick_at(&mut app, now + PREVIEW_DEBOUNCE / 2).unwrap();
        assert_eq!(spawned(&root), 1);

        // Now it has: the secret it stopped on is read, once
        root.tick_at(&mut app, now + PREVIEW_DEBOUNCE).unwrap();
        assert_eq!(spawned(&root), 2);
        assert_eq!(pending(&root), None);
        root.tick_at(&mut app, now + PREVIEW_DEBOUNCE * 2).unwrap();
        assert_eq!(spawned(&root), 2);
    }

    #[tokio::test]
    async fn selection_moved_by_a_reload_always_waits() {
        // Typing a filter moves the cursor with every key; none of the
        // matches on the way should be read
        let mut app = test_app();
        let mut root = test_root(&app);
        let start = Instant::now();

        root.selection_changed(
            &app,
            Some("/db".to_string()),
            SelectionSource::ListChange,
            start,
        );
        assert_eq!(spawned(&root), 0);
        assert_eq!(pending(&root), Some("/db"));

        root.tick_at(&mut app, start + PREVIEW_DEBOUNCE).unwrap();
        assert_eq!(spawned(&root), 1);
    }

    #[tokio::test]
    async fn moving_onto_a_folder_cancels_the_load() {
        let mut app = test_app();
        let mut root = test_root(&app);
        let start = Instant::now();

        cursor_on(&mut root, &app, "/a", start);
        cursor_on(&mut root, &app, "/b", start + Duration::from_millis(20));
        assert_eq!(pending(&root), Some("/b"));

        root.selection_changed(
            &app,
            None,
            SelectionSource::Cursor,
            start + Duration::from_millis(40),
        );
        assert_eq!(pending(&root), None);
        assert_eq!(root.latest_secret_request, None);

        root.tick_at(&mut app, start + PREVIEW_DEBOUNCE * 3)
            .unwrap();
        assert_eq!(spawned(&root), 1, "only /a was ever read");

        // /a's result arrives after the cursor left: it is not shown
        root.process_action(loaded("/a", "field_a"), &mut app)
            .unwrap();
        assert!(!screen_text(&mut root, &app).contains("field_a"));
    }

    #[tokio::test]
    async fn the_selected_secret_is_not_requested_twice() {
        let mut app = test_app();
        let mut root = test_root(&app);
        let start = Instant::now();

        // Saved and reloaded explicitly; then the list reload puts the
        // cursor on it
        root.process_action(view("/a"), &mut app).unwrap();
        assert_eq!(spawned(&root), 1);
        root.selection_changed(
            &app,
            Some("/a".to_string()),
            SelectionSource::ListChange,
            start,
        );
        root.tick_at(&mut app, start + PREVIEW_DEBOUNCE).unwrap();
        assert_eq!(spawned(&root), 1);
    }

    #[tokio::test]
    async fn enter_loads_a_waiting_secret_at_once() {
        let mut app = test_app();
        let mut root = test_root(&app);
        let start = Instant::now();

        cursor_on(&mut root, &app, "/a", start);
        cursor_on(&mut root, &app, "/b", start + Duration::from_millis(20));
        assert_eq!(spawned(&root), 1);
        assert_eq!(pending(&root), Some("/b"));

        let open = Action::OpenSecret {
            path: "/b".to_string(),
        };
        root.process_action(open.clone(), &mut app).unwrap();
        assert_eq!(spawned(&root), 2);
        assert_eq!(pending(&root), None);

        // Enter again while it is on its way: no second read
        root.process_action(open, &mut app).unwrap();
        assert_eq!(spawned(&root), 2);
    }

    #[tokio::test]
    async fn a_failed_load_is_shown_and_enter_retries_it() {
        let mut app = test_app();
        let mut root = test_root(&app);

        root.process_action(view("/a"), &mut app).unwrap();
        root.process_action(failed("/a"), &mut app).unwrap();

        let text = screen_text(&mut root, &app);
        assert!(text.contains("no access to /a"), "{text}");
        assert_eq!(root.latest_secret_request, None);

        let open = Action::OpenSecret {
            path: "/a".to_string(),
        };
        root.process_action(open, &mut app).unwrap();
        assert_eq!(spawned(&root), 2);
    }

    #[tokio::test]
    async fn a_failure_for_a_secret_the_user_left_is_ignored() {
        let mut app = test_app();
        let mut root = test_root(&app);

        root.process_action(view("/a"), &mut app).unwrap();
        root.process_action(view("/b"), &mut app).unwrap();
        root.process_action(failed("/a"), &mut app).unwrap();

        let text = screen_text(&mut root, &app);
        assert!(!text.contains("no access"), "{text}");
        assert_eq!(root.latest_secret_request.as_deref(), Some("/b"));
    }

    #[tokio::test]
    async fn a_deleted_secret_is_forgotten() {
        // Regression: `SecretDeleted` was never handled, and the deleted
        // secret stayed on screen.
        let mut app = test_app();
        let mut root = test_root(&app);

        root.process_action(view("/a"), &mut app).unwrap();
        root.process_action(loaded("/a", "field_a"), &mut app)
            .unwrap();
        assert!(screen_text(&mut root, &app).contains("field_a"));

        let deleted = Action::SecretDeleted {
            path: "/a".to_string(),
        };
        root.process_action(deleted, &mut app).unwrap();

        assert!(!screen_text(&mut root, &app).contains("field_a"));
        assert_eq!(root.latest_secret_request, None);
    }

    #[tokio::test]
    async fn policy_errors_stay_on_the_policy_screen() {
        // Regression: a failed policy load set the error on the main screen,
        // and the policy screen said "No policies found".
        let mut app = test_app();
        let mut root = test_root(&app);

        root.process_action(Action::ShowPolicyScreen, &mut app)
            .unwrap();
        root.process_action(Action::LoadPolicies, &mut app).unwrap();
        assert!(screen_text(&mut root, &app).contains("Loading policies"));

        let error = Action::SetError {
            message: "Failed to load policies: permission denied".to_string(),
        };
        root.process_action(error, &mut app).unwrap();

        let text = screen_text(&mut root, &app);
        assert!(text.contains("permission denied"), "{text}");
        assert!(!text.contains("No policies found"), "{text}");
    }

    #[tokio::test]
    async fn paste_outside_a_text_input_does_nothing() {
        let mut app = test_app();
        let mut root = test_root(&app);

        // On the main screen with no input open, and behind the help overlay
        assert!(root.handle_paste("dy", &app).unwrap().is_none());
        root.process_action(Action::ShowHelp, &mut app).unwrap();
        assert!(root.handle_paste("q", &app).unwrap().is_none());
        assert!(!root.should_quit());
    }
}
