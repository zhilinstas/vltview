use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Frame;
use std::time::{Duration, Instant};

use crate::app::App;
use crate::app::navigation::parent_path;
use crate::tui::action::{Action, SelectionSource, VersionUse};
use crate::tui::async_manager::AsyncManager;
use crate::tui::component::Component;
use crate::tui::components::{HelpOverlay, LogScreen, MainScreen, PolicyScreen};
use crate::tui::context::{ActionSender, ComponentContext, RenderContext};

/// How long the cursor has to rest on a secret before it is loaded.
///
/// Every load is a read of the secret in Vault's audit log, so scrolling
/// through a folder must not read everything on the way. The first step after
/// a pause loads at once; while the cursor keeps moving, only the secret it
/// stops on is loaded.
const PREVIEW_DEBOUNCE: Duration = Duration::from_millis(200);

/// What a folder listing was requested for
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ListingKind {
    /// Go to another folder
    Open,
    /// Read the folder on screen again
    Reload,
    /// Go back or forward to the folder at this position in the history
    History { position: usize },
}

/// The folder listing that is wanted. Listings are not cancelled, so their
/// results can arrive in any order; only the one for the latest request is
/// applied, and the path and items on screen always change together.
#[derive(Debug)]
struct ListingRequest {
    id: u64,
    kind: ListingKind,
}

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

    /// Number of the last load of a secret that was started. Two loads of
    /// one path can be on their way at once (the preview, and the read
    /// after a save); only the result of the later one is shown.
    secret_load_seq: u64,

    /// Screen `q` was pressed on while the main screen asks about unsaved
    /// changes; the user is taken back there if they decline
    asked_to_quit_from: Option<ActiveScreen>,

    /// Selected secret whose load is waiting for the cursor to rest, and
    /// when it is due
    pending_preview: Option<(String, Instant)>,

    /// When the selection last changed
    last_selection_change: Option<Instant>,

    /// The listing whose result is awaited, if any
    listing: Option<ListingRequest>,

    /// Number of the last listing request
    listing_seq: u64,

    /// A reload was asked for while the user was on the way to another
    /// folder. It is made up for if they do not get there.
    reload_skipped: bool,
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
            async_manager: AsyncManager::new(runtime),
            should_quit: false,
            pending_clipboard: None,
            latest_secret_request: None,
            secret_load_seq: 0,
            asked_to_quit_from: None,
            pending_preview: None,
            last_selection_change: None,
            listing: None,
            listing_seq: 0,
            reload_skipped: false,
        }
    }

    /// Initialize all components
    pub fn init(&mut self, app: &App) -> Result<()> {
        let action_tx = self.action_sender();
        let mut ctx = ComponentContext::new(app, &action_tx);

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
        // Ctrl+C quits from anywhere, at once, whatever has the keyboard
        if key.code == KeyCode::Char('c') && key.modifiers == KeyModifiers::CONTROL {
            return Ok(Some(Action::QuitNow));
        }

        let action_tx = self.action_sender();
        let ctx = ComponentContext::new(app, &action_tx);

        // Help overlay captures input when visible
        if self.help_overlay.captures_input() {
            return self.help_overlay.handle_key(key, &ctx);
        }

        // Route to active screen
        match self.active_screen {
            ActiveScreen::Main => {
                let action = self.main_screen.handle_key(key, &ctx)?;
                // The question about quitting was asked from another screen
                // and has been answered "no": back to that screen
                if !self.main_screen.captures_input()
                    && let Some(screen) = self.asked_to_quit_from.take()
                {
                    self.active_screen = screen;
                }
                Ok(action)
            }
            ActiveScreen::Logs => self.log_screen.handle_key(key, &ctx),
            ActiveScreen::Policy => self.policy_screen.handle_key(key, &ctx),
        }
    }

    /// Route pasted text to whatever text input has focus
    pub fn handle_paste(&mut self, text: &str, app: &App) -> Result<Option<Action>> {
        let action_tx = self.action_sender();
        let ctx = ComponentContext::new(app, &action_tx);

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
        let mut ctx = ComponentContext::new(app, &action_tx);
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
        let mut ctx = ComponentContext::new(app, &action_tx);

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

            // A secret was written. It is read again only if the cursor is
            // still on it: the content pane shows what is selected, not what
            // was saved last. (A new secret gets the cursor when the list
            // reloads, and is loaded as the selection then.)
            Action::ReloadSecret { path } => {
                if self.latest_secret_request.as_deref() == Some(path.as_str()) {
                    self.pending_preview = None;
                    self.spawn_load_secret(app, path.clone());
                }
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

            Action::SaveSecret { path, data, mode } => {
                self.spawn_save_secret(app, path.clone(), data.clone(), *mode);
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
                self.request_listing(app, path.clone(), ListingKind::Open);
                return Ok(());
            }

            Action::NavigateUp => {
                let current = app.state().current_path();
                if current != "/" {
                    self.request_listing(app, parent_path(&current), ListingKind::Open);
                }
                return Ok(());
            }

            Action::Refresh => {
                // A folder the user is on the way to is listed fresh anyway,
                // and a reload must not take its place as the listing wanted
                if self
                    .listing
                    .as_ref()
                    .is_some_and(|listing| listing.kind != ListingKind::Reload)
                {
                    self.reload_skipped = true;
                } else {
                    self.request_listing(app, app.state().current_path(), ListingKind::Reload);
                }
                return Ok(());
            }

            Action::HistoryBack => {
                self.history_step(app, -1);
                return Ok(());
            }

            Action::HistoryForward => {
                self.history_step(app, 1);
                return Ok(());
            }

            Action::FolderListed {
                request,
                path,
                items,
            } => {
                // Overtaken by a newer request: applying it would put this
                // folder's items on screen after the user has moved on
                let Some(kind) = self.take_listing(*request) else {
                    tracing::debug!("Ignoring stale folder listing");
                    return Ok(());
                };
                self.reload_skipped = false;

                let state = app.state();
                match kind {
                    ListingKind::Open => state.show_folder(path.clone(), items.clone()),
                    ListingKind::Reload => {
                        state.reload_folder(path, items.clone());
                    }
                    ListingKind::History { position } => {
                        state.show_history_entry(position, path, items.clone());
                    }
                }
                return self.process_action(Action::StateUpdated, app);
            }

            Action::FolderListFailed { request, .. } => {
                if self
                    .listing
                    .take_if(|listing| listing.id == *request)
                    .is_none()
                {
                    tracing::debug!("Ignoring stale folder listing failure");
                    return Ok(());
                }
                self.main_screen.update(action, &mut ctx)?;

                // The user stays where they were, so the reload that was put
                // off for this navigation is due after all
                if std::mem::take(&mut self.reload_skipped) {
                    self.request_listing(app, app.state().current_path(), ListingKind::Reload);
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
            Action::ScreenBack => {
                self.active_screen = ActiveScreen::Main;
            }
            Action::ShowPolicyScreen => {
                self.active_screen = ActiveScreen::Policy;
            }

            // `q`, from any screen. The main screen knows whether an edit
            // would be lost; if so it asks, so that is the screen to show.
            Action::Quit => {
                if let Some(next) = self.main_screen.update(action, &mut ctx)? {
                    return self.process_action(next, app);
                }
                if self.active_screen != ActiveScreen::Main {
                    self.asked_to_quit_from = Some(self.active_screen);
                    self.active_screen = ActiveScreen::Main;
                }
            }

            Action::QuitNow => {
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
            // content pane never shows a secret other than the last one
            // asked for, nor an older read of it after a newer one
            Action::SecretLoaded { request, path, .. } if !self.is_wanted(*request, path) => {
                tracing::debug!("Ignoring stale secret load");
            }

            // A failed load that was overtaken is of no interest either
            Action::SecretLoadFailed { request, path, .. } if !self.is_wanted(*request, path) => {
                tracing::debug!("Ignoring stale secret load failure");
            }

            // The secret on screen could not be loaded. Forget the request,
            // so that Enter on it tries again.
            Action::SecretLoadFailed { .. } => {
                self.latest_secret_request = None;
                self.main_screen.update(action, &mut ctx)?;
            }

            // A failed copy is reported where the key was pressed
            Action::CopyFailed { .. } if self.active_screen == ActiveScreen::Policy => {
                self.policy_screen.update(action, &mut ctx)?;
            }

            // Main screen actions
            Action::SecretLoaded { .. }
            | Action::SecretSaved { .. }
            | Action::SecretDeleted { .. }
            | Action::EditSecret
            | Action::CreateSecret { .. }
            | Action::ShowCreateSecretInput
            | Action::ShowDeleteConfirmation { .. }
            | Action::SetStatus { .. }
            | Action::SaveFailed { .. }
            | Action::DeleteFailed { .. }
            | Action::VersionLoadFailed { .. }
            | Action::CopyFailed { .. }
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

            // Policy screen actions. Its errors go there too, whatever is
            // on screen when they arrive.
            Action::PoliciesLoaded { .. }
            | Action::PolicyLoaded { .. }
            | Action::PoliciesLoadFailed { .. }
            | Action::PolicyLoadFailed { .. } => {
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

    /// Whether the result of secret load `request` for `path` is the one
    /// to show: the latest load started, for the secret that is selected
    fn is_wanted(&self, request: u64, path: &str) -> bool {
        request == self.secret_load_seq && self.latest_secret_request.as_deref() == Some(path)
    }

    /// Spawn async operation to load a secret
    fn spawn_load_secret(&mut self, app: &App, path: String) {
        let secret_service = app.secret_service().clone();
        let failed_path = path.clone();
        self.secret_load_seq += 1;
        let request = self.secret_load_seq;

        self.async_manager.spawn(
            async move {
                use crate::tui::action::{SecretMetadata, VersionInfo};

                let secret = secret_service.get(&path).await?;
                let data = secret.data;
                let metadata = secret.metadata.map(|meta| {
                    let versions: Vec<VersionInfo> = meta
                        .version_list()
                        .into_iter()
                        .map(|v| VersionInfo {
                            version: v,
                            created_time: meta
                                .versions
                                .get(&v.to_string())
                                .map(|m| m.created_time.clone())
                                .unwrap_or_default(),
                            deleted: meta.is_version_deleted(v),
                        })
                        .collect();

                    SecretMetadata {
                        created_time: meta.created_time,
                        updated_time: meta.updated_time,
                        current_version: meta.current_version,
                        versions,
                    }
                });

                Ok(Action::SecretLoaded {
                    request,
                    path: path.clone(),
                    data,
                    metadata,
                })
            },
            move |e| Action::SecretLoadFailed {
                request,
                message: format!("Could not load {failed_path}: {e}"),
                path: failed_path.clone(),
            },
        );
    }

    /// List a folder. This becomes the listing that is wanted: the result
    /// of any earlier request is ignored when it arrives.
    fn request_listing(&mut self, app: &App, path: String, kind: ListingKind) {
        self.listing_seq += 1;
        let request = self.listing_seq;
        self.listing = Some(ListingRequest { id: request, kind });

        let nav = app.navigation().clone();
        let failed_path = path.clone();

        self.async_manager.spawn(
            async move {
                let items = nav.list(&path).await?;
                Ok(Action::FolderListed {
                    request,
                    path,
                    items,
                })
            },
            move |e| Action::FolderListFailed {
                request,
                message: match kind {
                    ListingKind::Reload => format!("Could not reload {failed_path}: {e}"),
                    ListingKind::Open | ListingKind::History { .. } => {
                        format!("Could not open {failed_path}: {e}")
                    }
                },
            },
        );
    }

    /// Go back (`-1`) or forward (`1`) in the history.
    ///
    /// The place in the history moves only when the folder has been listed,
    /// so a step that fails leaves the user where they were. A step asked
    /// for while another one is on its way continues from where that one is
    /// heading: `b`, `b` goes two folders back however slow Vault is.
    fn history_step(&mut self, app: &App, steps: isize) {
        let state = app.state();
        let from = match self.listing {
            Some(ListingRequest {
                kind: ListingKind::History { position },
                ..
            }) => position,
            _ => state.history_position(),
        };

        // At either end of the history there is nowhere to go
        if let Some(position) = from.checked_add_signed(steps)
            && let Some(path) = state.history_entry(position)
        {
            self.request_listing(app, path, ListingKind::History { position });
        }
    }

    /// What the listing `request` was for, if it is still the one wanted.
    /// It is no longer awaited after this.
    fn take_listing(&mut self, request: u64) -> Option<ListingKind> {
        let listing = self.listing.take_if(|listing| listing.id == request)?;
        Some(listing.kind)
    }

    /// Spawn async operation to delete a secret
    fn spawn_delete_secret(&mut self, app: &App, path: String) {
        let secret_service = app.secret_service().clone();
        let failed_path = path.clone();

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
                    ],
                })
            },
            move |e| Action::DeleteFailed {
                path: failed_path.clone(),
                message: e.to_string(),
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
            |e| Action::PoliciesLoadFailed {
                message: format!("Could not load the policy list: {e}"),
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
            move |e| Action::PolicyLoadFailed {
                message: format!("Could not load policy '{name_for_error}': {e}"),
                name: name_for_error.clone(),
            },
        );
    }

    /// Spawn async operation to save a secret
    fn spawn_save_secret(
        &mut self,
        app: &App,
        path: String,
        data: crate::vault::SecretData,
        mode: crate::vault::WriteMode,
    ) {
        let secret_service = app.secret_service().clone();
        let failed_path = path.clone();

        self.async_manager.spawn(
            async move {
                // Save the secret
                secret_service.save(&path, data, mode).await?;

                Ok(Action::Batch {
                    actions: vec![
                        Action::SecretSaved { path: path.clone() },
                        Action::SetStatus {
                            message: format!("Saved: {}", path),
                        },
                        Action::Refresh, // Refresh navigation list to show new secret
                        // Show the new version, if the secret is still selected
                        Action::ReloadSecret { path: path.clone() },
                    ],
                })
            },
            move |e| Action::SaveFailed {
                message: format!("Could not save {failed_path}: {e}"),
                path: failed_path.clone(),
            },
        );
    }

    /// Spawn async operation to load a specific version of a secret
    fn spawn_load_secret_version(&mut self, app: &App, path: String, version: u64) {
        let secret_service = app.secret_service().clone();
        let failed_path = path.clone();

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
            move |e| Action::VersionLoadFailed {
                message: format!("Could not load version {version} of {failed_path}: {e}"),
                path: failed_path.clone(),
                version,
                wanted_for: VersionUse::View,
            },
        );
    }

    /// Spawn async operation to load a version for editing (create from old)
    fn spawn_load_version_for_edit(&mut self, app: &App, path: String, version: u64) {
        let secret_service = app.secret_service().clone();
        let failed_path = path.clone();

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
            move |e| Action::VersionLoadFailed {
                message: format!("Could not load version {version} of {failed_path}: {e}"),
                path: failed_path.clone(),
                version,
                wanted_for: VersionUse::Edit,
            },
        );
    }

    /// Render the current screen and overlays
    pub fn render(&mut self, frame: &mut Frame, app: &App) {
        let area = frame.area();
        let ctx = RenderContext::new(app, true);

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
            vault_token: "test-token".to_string().into(),
            ..Config::default()
        };
        App::new(config, "/".to_string()).unwrap()
    }

    /// An app showing the folder `path` with `items` in it
    fn test_app_at(path: &str, items: &[&str]) -> App {
        let config = Config {
            vault_addr: "http://127.0.0.1:1".to_string(),
            vault_token: "test-token".to_string().into(),
            ..Config::default()
        };
        let app = App::new(config, path.to_string()).unwrap();
        app.state()
            .reload_folder(path, items.iter().map(|s| s.to_string()).collect());
        app
    }

    /// Number of the listing request that is awaited
    fn listing(root: &Root) -> u64 {
        root.listing.as_ref().expect("a listing is awaited").id
    }

    fn listed(request: u64, path: &str, items: &[&str]) -> Action {
        Action::FolderListed {
            request,
            path: path.to_string(),
            items: items.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn open(path: &str) -> Action {
        Action::NavigateTo {
            path: path.to_string(),
        }
    }

    fn test_root(app: &App) -> Root {
        let mut root = Root::new(tokio::runtime::Handle::current());
        root.init(app).unwrap();
        root
    }

    fn loaded(path: &str, field: &str) -> Action {
        Action::SecretLoaded {
            request: 0,
            path: path.to_string(),
            data: serde_json::from_value(serde_json::json!({ field: "value" })).unwrap(),
            metadata: None,
        }
    }

    /// The cursor comes to rest on the secret at `path`
    fn view(path: &str) -> Action {
        Action::SelectionChanged {
            secret: Some(path.to_string()),
            source: SelectionSource::Cursor,
        }
    }

    fn failed(path: &str) -> Action {
        Action::SecretLoadFailed {
            request: 0,
            path: path.to_string(),
            message: format!("Failed to load secret: no access to {path}"),
        }
    }

    /// The action as the load that was started last would send it
    fn stamped(root: &Root, action: Action) -> Action {
        let request = root.secret_load_seq;
        match action {
            Action::SecretLoaded {
                path,
                data,
                metadata,
                ..
            } => Action::SecretLoaded {
                request,
                path,
                data,
                metadata,
            },
            Action::SecretLoadFailed { path, message, .. } => Action::SecretLoadFailed {
                request,
                path,
                message,
            },
            other => other,
        }
    }

    /// Let the result of the load that was started last arrive
    fn deliver(root: &mut Root, app: &mut App, result: Action) {
        let result = stamped(root, result);
        root.process_action(result, app).unwrap();
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

    const SCREEN_WIDTH: usize = 120;

    fn screen_text(root: &mut Root, app: &App) -> String {
        let mut terminal = Terminal::new(TestBackend::new(SCREEN_WIDTH as u16, 30)).unwrap();
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
        deliver(&mut root, &mut app, loaded("/app/b", "field_b"));
        deliver(&mut root, &mut app, loaded("/app/a", "field_a"));

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
        deliver(&mut root, &mut app, loaded("/app/a", "field_a"));

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
        deliver(&mut root, &mut app, loaded("/a", "field_a"));
        assert!(!screen_text(&mut root, &app).contains("field_a"));
    }

    #[tokio::test]
    async fn the_selected_secret_is_not_requested_twice() {
        let mut app = test_app();
        let mut root = test_root(&app);
        let start = Instant::now();

        // The cursor is on /a; it is saved and read again. The list reload
        // that follows finds the cursor on the same secret.
        root.process_action(view("/a"), &mut app).unwrap();
        let reload = Action::ReloadSecret {
            path: "/a".to_string(),
        };
        root.process_action(reload, &mut app).unwrap();
        assert_eq!(spawned(&root), 2);
        root.selection_changed(
            &app,
            Some("/a".to_string()),
            SelectionSource::ListChange,
            start,
        );
        root.tick_at(&mut app, start + PREVIEW_DEBOUNCE).unwrap();
        assert_eq!(spawned(&root), 2);
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
        deliver(&mut root, &mut app, failed("/a"));

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
        deliver(&mut root, &mut app, failed("/a"));

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
        deliver(&mut root, &mut app, loaded("/a", "field_a"));
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

        let error = Action::PoliciesLoadFailed {
            message: "Could not load the policy list: permission denied".to_string(),
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

    #[tokio::test]
    async fn a_late_reload_never_puts_its_items_under_another_folder() {
        // Regression: a refresh wrote its items into whatever folder was
        // current when it finished. Refreshing /a and opening /b before the
        // refresh returned could leave /a's items under /b's path, where
        // Enter or `d` would act on the wrong secret.
        for reload_arrives_first in [true, false] {
            let mut app = test_app_at("/a", &["a-old"]);
            let mut root = test_root(&app);

            root.process_action(Action::Refresh, &mut app).unwrap();
            let reload = listed(listing(&root), "/a", &["a-new"]);
            root.process_action(open("/b"), &mut app).unwrap();
            let opened = listed(listing(&root), "/b", &["b-secret"]);

            let results = if reload_arrives_first {
                [reload, opened]
            } else {
                [opened, reload]
            };
            for result in results {
                root.process_action(result, &mut app).unwrap();
            }

            assert_eq!(app.state().current_path(), "/b");
            assert_eq!(app.state().list_items(), ["b-secret"]);
            let text = screen_text(&mut root, &app);
            assert!(text.contains("│ /b "), "{text}");
            assert!(text.contains("b-secret"), "{text}");
            assert!(!text.contains("a-new"), "{text}");
        }
    }

    #[tokio::test]
    async fn only_the_last_folder_asked_for_is_shown() {
        let mut app = test_app_at("/", &["a/", "b/"]);
        let mut root = test_root(&app);

        root.process_action(open("/a"), &mut app).unwrap();
        let first = listed(listing(&root), "/a", &["in-a"]);
        root.process_action(open("/b"), &mut app).unwrap();
        let second = listed(listing(&root), "/b", &["in-b"]);

        // /a answers after /b: it is no longer wanted
        root.process_action(second, &mut app).unwrap();
        root.process_action(first, &mut app).unwrap();

        assert_eq!(app.state().current_path(), "/b");
        assert_eq!(app.state().list_items(), ["in-b"]);
        assert!(!screen_text(&mut root, &app).contains("in-a"));
    }

    #[tokio::test]
    async fn a_failed_navigation_leaves_the_folder_as_it_was() {
        let mut app = test_app_at("/a", &["a-secret"]);
        let mut root = test_root(&app);

        root.process_action(open("/a/locked"), &mut app).unwrap();
        let failed = Action::FolderListFailed {
            request: listing(&root),
            message: "Could not open /a/locked: permission denied".to_string(),
        };
        root.process_action(failed, &mut app).unwrap();

        assert_eq!(app.state().current_path(), "/a");
        assert_eq!(app.state().list_items(), ["a-secret"]);
        let text = screen_text(&mut root, &app);
        assert!(text.contains("│ /a "), "{text}");
        assert!(text.contains("a-secret"), "{text}");
        assert!(text.contains("permission denied"), "{text}");
    }

    #[tokio::test]
    async fn a_reload_does_not_replace_a_navigation_that_is_on_its_way() {
        // A save or a delete reloads the list when it completes. If the user
        // has opened a folder meanwhile, that folder must still be shown.
        let mut app = test_app_at("/a", &["dir/", "old"]);
        let mut root = test_root(&app);

        root.process_action(open("/a/dir"), &mut app).unwrap();
        let opened = listed(listing(&root), "/a/dir", &["inside"]);
        root.process_action(Action::Refresh, &mut app).unwrap();
        root.process_action(opened, &mut app).unwrap();

        assert_eq!(app.state().current_path(), "/a/dir");
        assert_eq!(app.state().list_items(), ["inside"]);
    }

    #[tokio::test]
    async fn a_reload_put_off_for_a_navigation_is_made_when_it_fails() {
        let mut app = test_app_at("/a", &["dir/", "deleted"]);
        let mut root = test_root(&app);

        root.process_action(open("/a/dir"), &mut app).unwrap();
        let request = listing(&root);
        root.process_action(Action::Refresh, &mut app).unwrap();
        let failed = Action::FolderListFailed {
            request,
            message: "Could not open /a/dir: permission denied".to_string(),
        };
        root.process_action(failed, &mut app).unwrap();

        // The folder on screen is read again after all
        let reload = listed(listing(&root), "/a", &["dir/"]);
        root.process_action(reload, &mut app).unwrap();
        assert_eq!(app.state().list_items(), ["dir/"]);
    }

    /// Press a key and process what it leads to
    fn press(root: &mut Root, app: &mut App, c: char) {
        let key = KeyEvent::new(
            crossterm::event::KeyCode::Char(c),
            crossterm::event::KeyModifiers::NONE,
        );
        if let Some(action) = root.handle_key_event(key, app).unwrap() {
            root.process_action(action, app).unwrap();
        }
    }

    /// Open `path` and let its listing arrive
    fn visit(root: &mut Root, app: &mut App, path: &str) {
        root.process_action(open(path), app).unwrap();
        answer(root, app, path);
    }

    /// Let the awaited listing arrive; it must be for `path`
    fn answer(root: &mut Root, app: &mut App, path: &str) {
        let result = listed(listing(root), path, &["x"]);
        root.process_action(result, app).unwrap();
        assert_eq!(app.state().current_path(), path);
    }

    #[tokio::test]
    async fn history_keys_walk_back_and_forward() {
        // Regression: `b` went back and forth between the last two folders,
        // and `f` never did anything.
        let mut app = test_app_at("/a", &["x"]);
        let mut root = test_root(&app);
        visit(&mut root, &mut app, "/b");
        visit(&mut root, &mut app, "/c");

        press(&mut root, &mut app, 'b');
        answer(&mut root, &mut app, "/b");
        press(&mut root, &mut app, 'b');
        answer(&mut root, &mut app, "/a");

        press(&mut root, &mut app, 'f');
        answer(&mut root, &mut app, "/b");
        press(&mut root, &mut app, 'f');
        answer(&mut root, &mut app, "/c");
    }

    #[tokio::test]
    async fn history_keys_do_nothing_at_either_end() {
        let mut app = test_app_at("/a", &["x"]);
        let mut root = test_root(&app);

        // Nowhere to go back or forward to yet
        press(&mut root, &mut app, 'b');
        press(&mut root, &mut app, 'f');
        assert!(root.listing.is_none());
        assert_eq!(spawned(&root), 0);

        visit(&mut root, &mut app, "/b");
        press(&mut root, &mut app, 'f');
        assert!(root.listing.is_none());
        assert_eq!(app.state().current_path(), "/b");

        press(&mut root, &mut app, 'b');
        answer(&mut root, &mut app, "/a");
        press(&mut root, &mut app, 'b');
        assert!(root.listing.is_none());
        assert_eq!(app.state().current_path(), "/a");
    }

    #[tokio::test]
    async fn a_new_folder_after_going_back_drops_the_way_forward() {
        let mut app = test_app_at("/a", &["x"]);
        let mut root = test_root(&app);
        visit(&mut root, &mut app, "/b");
        press(&mut root, &mut app, 'b');
        answer(&mut root, &mut app, "/a");

        visit(&mut root, &mut app, "/c");
        press(&mut root, &mut app, 'f');
        assert!(root.listing.is_none(), "/b is no longer ahead");

        press(&mut root, &mut app, 'b');
        answer(&mut root, &mut app, "/a");
    }

    #[tokio::test]
    async fn two_quick_steps_back_go_two_folders_back() {
        let mut app = test_app_at("/a", &["x"]);
        let mut root = test_root(&app);
        visit(&mut root, &mut app, "/b");
        visit(&mut root, &mut app, "/c");

        // The second `b` comes before the first listing has arrived
        press(&mut root, &mut app, 'b');
        press(&mut root, &mut app, 'b');
        answer(&mut root, &mut app, "/a");

        press(&mut root, &mut app, 'f');
        answer(&mut root, &mut app, "/b");
    }

    #[tokio::test]
    async fn a_failed_history_step_leaves_the_user_where_they_were() {
        let mut app = test_app_at("/a", &["x"]);
        let mut root = test_root(&app);
        visit(&mut root, &mut app, "/b");

        press(&mut root, &mut app, 'b');
        let failed = Action::FolderListFailed {
            request: listing(&root),
            message: "Could not open /a: permission denied".to_string(),
        };
        root.process_action(failed, &mut app).unwrap();

        assert_eq!(app.state().current_path(), "/b");
        let text = screen_text(&mut root, &app);
        assert!(text.contains("│ /b "), "{text}");
        assert!(text.contains("permission denied"), "{text}");

        // The step was not taken, so it can be tried again
        let esc = KeyEvent::new(
            crossterm::event::KeyCode::Esc,
            crossterm::event::KeyModifiers::NONE,
        );
        root.handle_key_event(esc, &app).unwrap();
        press(&mut root, &mut app, 'b');
        answer(&mut root, &mut app, "/a");
    }

    /// What a completed save sends
    fn saved(path: &str) -> Action {
        Action::Batch {
            actions: vec![
                Action::SecretSaved {
                    path: path.to_string(),
                },
                Action::Refresh,
                Action::ReloadSecret {
                    path: path.to_string(),
                },
            ],
        }
    }

    #[tokio::test]
    async fn a_saved_secret_is_read_again_while_the_cursor_is_on_it() {
        let mut app = test_app_at("/", &["a", "b"]);
        let mut root = test_root(&app);
        root.process_action(view("/a"), &mut app).unwrap();
        deliver(&mut root, &mut app, loaded("/a", "field_a"));
        let before = spawned(&root);

        root.process_action(saved("/a"), &mut app).unwrap();

        // The list and the secret
        assert_eq!(spawned(&root), before + 2);
        assert_eq!(root.latest_secret_request.as_deref(), Some("/a"));
    }

    #[tokio::test]
    async fn a_saved_secret_does_not_take_the_pane_after_the_cursor_left_it() {
        // Regression: a save always loaded and showed the saved secret, so
        // the pane could show /a while the cursor was on /b.
        let mut app = test_app_at("/", &["a", "b"]);
        let mut root = test_root(&app);
        root.process_action(view("/a"), &mut app).unwrap();
        deliver(&mut root, &mut app, loaded("/a", "field_a"));

        // The cursor moves on while the save is on its way
        root.process_action(view("/b"), &mut app).unwrap();
        deliver(&mut root, &mut app, loaded("/b", "field_b"));
        let before = spawned(&root);

        root.process_action(saved("/a"), &mut app).unwrap();

        // Only the list is read again; /a is not, and the pane stays on /b
        assert_eq!(spawned(&root), before + 1);
        assert_eq!(root.latest_secret_request.as_deref(), Some("/b"));
        let listing = listed(listing(&root), "/", &["a", "b"]);
        root.process_action(listing, &mut app).unwrap();
        deliver(&mut root, &mut app, loaded("/a", "field_a"));

        let text = screen_text(&mut root, &app);
        assert!(text.contains("field_b"), "{text}");
        assert!(!text.contains("field_a"), "{text}");
    }

    #[tokio::test]
    async fn main_screen_errors_do_not_appear_on_other_screens() {
        // Regression: an error went to whichever screen was open when it
        // arrived, so a failed delete could replace the policy list.
        let mut app = test_app();
        let mut root = test_root(&app);
        root.process_action(Action::ShowPolicyScreen, &mut app)
            .unwrap();
        let policies = Action::PoliciesLoaded {
            policies: vec!["admin".to_string()],
        };
        root.process_action(policies, &mut app).unwrap();

        let failed = Action::DeleteFailed {
            path: "/a".to_string(),
            message: "permission denied".to_string(),
        };
        root.process_action(failed, &mut app).unwrap();

        let text = screen_text(&mut root, &app);
        assert!(text.contains("admin"), "{text}");
        assert!(!text.contains("permission denied"), "{text}");

        // It is waiting where it belongs
        root.process_action(Action::ScreenBack, &mut app).unwrap();
        let text = screen_text(&mut root, &app);
        assert!(text.contains("Could not delete /a"), "{text}");
    }

    #[tokio::test]
    async fn policy_errors_do_not_appear_on_the_main_screen() {
        let mut app = test_app();
        let mut root = test_root(&app);

        let failed = Action::PoliciesLoadFailed {
            message: "Could not load the policy list: permission denied".to_string(),
        };
        root.process_action(failed, &mut app).unwrap();
        assert!(!screen_text(&mut root, &app).contains("permission denied"));

        root.process_action(Action::ShowPolicyScreen, &mut app)
            .unwrap();
        assert!(screen_text(&mut root, &app).contains("permission denied"));
    }

    #[tokio::test]
    async fn q_asks_before_losing_an_edit_and_ctrl_c_does_not() {
        let edit_with_a_change = |root: &mut Root, app: &mut App| {
            root.process_action(view("/a"), app).unwrap();
            deliver(root, app, loaded("/a", "field_a"));
            root.process_action(Action::EditSecret, app).unwrap();
            for c in ['a', 'k'] {
                press(root, app, c);
            }
            let enter = KeyEvent::new(
                crossterm::event::KeyCode::Enter,
                crossterm::event::KeyModifiers::NONE,
            );
            root.handle_key_event(enter, app).unwrap();
            root.handle_key_event(enter, app).unwrap();
        };

        // `q`, wherever it is pressed, asks on the main screen
        let mut app = test_app();
        let mut root = test_root(&app);
        edit_with_a_change(&mut root, &mut app);
        root.process_action(Action::ShowLogScreen, &mut app)
            .unwrap();
        root.process_action(Action::Quit, &mut app).unwrap();
        assert!(!root.should_quit());
        assert!(screen_text(&mut root, &app).contains(" Unsaved changes "));
        press(&mut root, &mut app, 'y');
        assert!(root.should_quit());

        // Ctrl+C quits at once
        let mut app = test_app();
        let mut root = test_root(&app);
        edit_with_a_change(&mut root, &mut app);
        root.process_action(Action::QuitNow, &mut app).unwrap();
        assert!(root.should_quit());

        // And `q` with nothing to lose does too
        let mut app = test_app();
        let mut root = test_root(&app);
        root.process_action(Action::Quit, &mut app).unwrap();
        assert!(root.should_quit());
    }

    // ═══════════════════════════════════════════════════════════════
    // Every key in the key table does something where it is hinted
    // ═══════════════════════════════════════════════════════════════

    use crate::tui::keymap::{self, Context};
    use crossterm::event::{KeyCode as Code, KeyModifiers as Mods};

    /// The keys a hint in the key table stands for: `j/k ↑/↓` is four keys,
    /// `Ctrl+s` is one. A spelling this does not know fails the test, so the
    /// table cannot grow hints that are never pressed.
    fn keys_of(hint: &str) -> Vec<KeyEvent> {
        let one = |name: &str| -> KeyEvent {
            let plain = |code| KeyEvent::new(code, Mods::NONE);
            match name {
                "↑" => plain(Code::Up),
                "↓" => plain(Code::Down),
                "←" => plain(Code::Left),
                "→" => plain(Code::Right),
                "Enter" => plain(Code::Enter),
                "Esc" => plain(Code::Esc),
                "Tab" => plain(Code::Tab),
                "Space" => plain(Code::Char(' ')),
                "Backspace" => plain(Code::Backspace),
                "PgUp" => plain(Code::PageUp),
                "PgDn" => plain(Code::PageDown),
                "Home" => plain(Code::Home),
                "End" => plain(Code::End),
                // Typing, in the filter
                "type" => plain(Code::Char('s')),
                "Alt+Enter" => KeyEvent::new(Code::Enter, Mods::ALT),
                name => {
                    let mut chars = name.chars();
                    match (name.strip_prefix("Ctrl+"), chars.next(), chars.next()) {
                        (Some(letter), _, _) if letter.chars().count() == 1 => {
                            KeyEvent::new(Code::Char(letter.chars().next().unwrap()), Mods::CONTROL)
                        }
                        (None, Some(c), None) if c.is_uppercase() => {
                            KeyEvent::new(Code::Char(c), Mods::SHIFT)
                        }
                        (None, Some(c), None) => plain(Code::Char(c)),
                        _ => panic!("the test does not know the key {name:?} in {hint:?}"),
                    }
                }
            }
        };

        if hint == "/" {
            return vec![one("/")];
        }
        hint.split_whitespace()
            .flat_map(|word| {
                if word.contains('+') {
                    vec![word]
                } else {
                    word.split('/').collect()
                }
            })
            .map(one)
            .collect()
    }

    /// Everything on screen, styles included (a cursor that moves changes
    /// colours, not text). The spinner is left out: it moves by itself.
    fn screen_cells(root: &mut Root, app: &App) -> Vec<(String, ratatui::style::Style)> {
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|frame| root.render(frame, app)).unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| {
                let symbol = cell.symbol();
                let spinner = "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏".contains(symbol) && !symbol.trim().is_empty();
                let symbol = if spinner { "*" } else { symbol };
                (symbol.to_string(), cell.style())
            })
            .collect()
    }

    /// Press a key as the event loop does. True if it led to an action,
    /// changed what is on screen, or quit.
    fn key_has_an_effect(root: &mut Root, app: &mut App, key: KeyEvent) -> bool {
        let before = screen_cells(root, app);
        let action = root.handle_key_event(key, app).unwrap();
        let acted = action.is_some();
        match action {
            // The log buffer is shared by every test in the process
            Some(Action::ClearLogs) | None => {}
            Some(action) => root.process_action(action, app).unwrap(),
        }
        acted || root.should_quit() || screen_cells(root, app) != before
    }

    /// Press keys as the event loop does, without looking at the screen
    fn feed(root: &mut Root, app: &mut App, keys: &[KeyEvent]) {
        for key in keys {
            if let Some(action) = root.handle_key_event(*key, app).unwrap() {
                root.process_action(action, app).unwrap();
            }
        }
    }

    fn plain(code: Code) -> KeyEvent {
        KeyEvent::new(code, Mods::NONE)
    }

    fn chars(text: &str) -> Vec<KeyEvent> {
        text.chars().map(|c| plain(Code::Char(c))).collect()
    }

    fn times(key: KeyEvent, count: usize) -> Vec<KeyEvent> {
        vec![key; count]
    }

    /// A secret with thirty fields, so that a cursor can be in the middle
    fn big_secret(path: &str, versions: u64) -> Action {
        use crate::tui::action::{SecretMetadata, VersionInfo};
        let fields: serde_json::Map<String, serde_json::Value> = (0..30)
            .map(|i| {
                (
                    format!("field_{i:02}"),
                    serde_json::json!(format!("value {i}")),
                )
            })
            .chain([(
                "long".to_string(),
                serde_json::json!((0..200).map(|i| format!("line {i}\n")).collect::<String>()),
            )])
            .collect();
        Action::SecretLoaded {
            request: 0,
            path: path.to_string(),
            data: fields.into_iter().collect(),
            metadata: (versions > 0).then(|| SecretMetadata {
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

    /// An app showing /app with thirty secrets. The keys pressed below
    /// never change it (no result of a request is delivered), so one app
    /// serves every screen built from it; building one is slow, because of
    /// the HTTP client.
    fn app_with_a_long_list() -> App {
        let names: Vec<String> = (0..30).map(|i| format!("s{i:02}")).collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        test_app_at("/app", &names)
    }

    fn then(app: &mut App, mut root: Root, keys: &[KeyEvent]) -> Root {
        feed(&mut root, app, keys);
        root
    }

    fn after(app: &mut App, mut root: Root, action: Action) -> Root {
        deliver(&mut root, app, action);
        root
    }

    /// The cursor on /app/s09, that secret loaded
    fn in_the_list(app: &mut App) -> Root {
        let mut root = test_root(app);
        // Draw once, so that paging knows the height of the list
        screen_cells(&mut root, app);
        let root = then(app, root, &times(plain(Code::Down), 10));
        after(app, root, big_secret("/app/s09", 30))
    }

    /// The same, inside the secret with the cursor on its eleventh field
    fn in_the_secret(app: &mut App) -> Root {
        let root = in_the_list(app);
        let root = then(app, root, &[plain(Code::Enter)]);
        then(app, root, &times(plain(Code::Down), 10))
    }

    /// The same, in the editor
    fn in_the_editor(app: &mut App) -> Root {
        let root = in_the_secret(app);
        let root = then(app, root, &chars("e"));
        then(app, root, &times(plain(Code::Down), 10))
    }

    /// The editor with one value changed
    fn with_an_unsaved_change(app: &mut App) -> Root {
        let root = in_the_editor(app);
        let keys = [
            vec![plain(Code::Enter)],
            chars("x"),
            vec![plain(Code::Enter)],
        ]
        .concat();
        then(app, root, &keys)
    }

    fn on_the_policy_screen(app: &mut App) -> Root {
        let root = in_the_list(app);
        let root = after(app, root, Action::ShowPolicyScreen);
        let policies = Action::PoliciesLoaded {
            policies: (0..40).map(|i| format!("policy-{i:02}")).collect(),
        };
        let mut root = after(app, root, policies);
        screen_cells(&mut root, app);
        then(app, root, &times(plain(Code::Down), 10))
    }

    /// The ways to get into `context`. A key passes if it does something
    /// in at least one of them: `h` focuses the list only from the secret,
    /// `l` focuses the secret only from the list.
    fn ways_into(context: Context, app: &mut App) -> Vec<Root> {
        match context {
            Context::Global => vec![in_the_list(app), in_the_secret(app)],
            Context::Navigation => vec![in_the_list(app)],
            Context::Filter => {
                let root = in_the_list(app);
                vec![then(app, root, &chars("/"))]
            }
            Context::Filtered => {
                let root = in_the_list(app);
                vec![then(
                    app,
                    root,
                    &[chars("/s"), vec![plain(Code::Enter)]].concat(),
                )]
            }
            Context::Secret => vec![in_the_secret(app)],
            Context::Versions => {
                let root = in_the_secret(app);
                let keys = [chars("l"), times(plain(Code::Down), 10)].concat();
                vec![then(app, root, &keys)]
            }
            Context::ValueViewer => {
                let root = in_the_secret(app);
                let keys = [
                    vec![plain(Code::End), plain(Code::Enter)],
                    times(plain(Code::Down), 40),
                ]
                .concat();
                vec![then(app, root, &keys)]
            }
            Context::Edit => vec![in_the_editor(app)],
            Context::EditField => {
                let value = in_the_editor(app);
                let value = then(app, value, &[plain(Code::Enter)]);
                let name = in_the_editor(app);
                let name = then(app, name, &chars("r"));
                vec![value, name]
            }
            Context::FieldDeleteConfirm => {
                let root = in_the_editor(app);
                vec![then(app, root, &chars("d"))]
            }
            Context::CreatePrompt => {
                let empty = in_the_list(app);
                let empty = then(app, empty, &chars("n"));
                let named = in_the_list(app);
                let named = then(app, named, &chars("nx"));
                vec![empty, named]
            }
            Context::DeleteConfirm => {
                let root = in_the_list(app);
                vec![then(app, root, &chars("d"))]
            }
            Context::DiscardConfirm => {
                let root = with_an_unsaved_change(app);
                vec![then(app, root, &[plain(Code::Esc)])]
            }
            Context::QuitConfirm => {
                let root = with_an_unsaved_change(app);
                vec![then(
                    app,
                    root,
                    &[vec![plain(Code::Tab)], chars("q")].concat(),
                )]
            }
            Context::Notice => {
                let root = in_the_list(app);
                let failed = Action::DeleteFailed {
                    path: "/app/s09".to_string(),
                    message: "permission denied".to_string(),
                };
                vec![after(app, root, failed)]
            }
            Context::LoadError => {
                let root = in_the_list(app);
                let root = then(app, root, &[plain(Code::Down)]);
                vec![after(app, root, failed("/app/s10"))]
            }
            Context::Policies => vec![on_the_policy_screen(app)],
            Context::PolicyContent => {
                let root = on_the_policy_screen(app);
                let root = then(app, root, &[plain(Code::Enter)]);
                let loaded = Action::PolicyLoaded {
                    name: "policy-10".to_string(),
                    content: (0..200).map(|i| format!("# line {i}\n")).collect(),
                };
                let root = after(app, root, loaded);
                vec![then(app, root, &times(plain(Code::PageDown), 2))]
            }
            Context::Logs => {
                let root = in_the_list(app);
                vec![after(app, root, Action::ShowLogScreen)]
            }
            Context::Help => {
                let root = in_the_list(app);
                let root = after(app, root, Action::ShowHelp);
                vec![then(app, root, &times(plain(Code::Down), 5))]
            }
            // Drawn and handled by the event loop before `Root` exists
            Context::Connecting => vec![],
        }
    }

    #[test]
    fn the_test_knows_every_key_spelling_in_the_table() {
        for context in Context::ALL {
            for binding in keymap::bindings(context) {
                assert!(!keys_of(binding.keys).is_empty(), "{:?}", binding.keys);
            }
        }
    }

    /// What a hinted key is expected to do, as its help text says
    #[derive(Debug, Clone, Copy)]
    enum Effect {
        /// It asks `Root` for this action (the name of the variant)
        Asks(&'static str),
        /// It leads to where the keys of this context are hinted
        To(Context),
        /// It stays where it is and changes what is shown: moves, scrolls,
        /// reveals, types
        Changes,
        /// Afterwards the screen shows this text, which it did not before
        Shows(&'static str),
        /// The app quits
        Quits,
    }

    /// The expected effect of each key of each hint. This is the second,
    /// independent description of the keys: a hint added to the table
    /// without a line here fails the test.
    fn expected(context: Context, hint: &str, key: KeyEvent) -> Effect {
        use Context as C;
        use Effect::*;
        let moves = ["j/k ↑/↓", "PgUp/PgDn", "g/G Home/End", "PgUp/PgDn g/G"];
        match (context, hint) {
            (C::Global, "Tab") => To(C::Secret),
            (C::Global, "h/l ←/→") => match key.code {
                Code::Char('h') | Code::Left => To(C::Navigation),
                _ => To(C::Secret),
            },
            (C::Global, "r") => Asks("Refresh"),
            (C::Global, "p") => Asks("ShowPolicyScreen"),
            (C::Global, "~") => Asks("ShowLogScreen"),
            (C::Global, "?") => Asks("ShowHelp"),
            (C::Global, "q" | "Ctrl+c") => Quits,

            (C::Navigation, hint) if moves.contains(&hint) => Asks("SelectionChanged"),
            (C::Navigation, "Enter") => Asks("OpenSecret"),
            (C::Navigation, "Backspace") => Asks("NavigateUp"),
            (C::Navigation, "b/f") => match key.code {
                Code::Char('b') => Asks("HistoryBack"),
                _ => Asks("HistoryForward"),
            },
            (C::Navigation, "/") => To(C::Filter),
            (C::Navigation, "n") => To(C::CreatePrompt),
            (C::Navigation, "d") => To(C::DeleteConfirm),
            (C::Navigation, "y") => Asks("CopyToClipboard"),

            (C::Filter, "type") => Changes,
            (C::Filter, "Enter" | "Esc") => To(C::Navigation),
            (C::Filtered, "/") => To(C::Filter),
            (C::Filtered, "Esc") => To(C::Navigation),

            (C::Secret, hint) if moves.contains(&hint) => Changes,
            (C::Secret, "Enter") => To(C::ValueViewer),
            (C::Secret, "Space" | "v") => Changes,
            (C::Secret, "c" | "y") => Asks("CopyToClipboard"),
            (C::Secret, "e") => Asks("EditSecret"),
            (C::Secret, "l") => To(C::Versions),
            (C::Secret, "Esc") => To(C::Navigation),

            (C::Versions, hint) if moves.contains(&hint) => Changes,
            (C::Versions, "Enter") => Asks("ViewSecretVersion"),
            (C::Versions, "n") => Asks("CreateFromVersion"),
            (C::Versions, "l/Esc") => To(C::Secret),

            (C::ValueViewer, hint) if moves.contains(&hint) => Changes,
            (C::ValueViewer, "c" | "y") => Asks("CopyToClipboard"),
            (C::ValueViewer, "Esc" | "Enter/q") => To(C::Secret),

            (C::Edit, hint) if moves.contains(&hint) => Changes,
            (C::Edit, "Enter" | "e" | "r" | "a") => To(C::EditField),
            (C::Edit, "d") => To(C::FieldDeleteConfirm),
            (C::Edit, "Space" | "v") => Changes,
            (C::Edit, "Ctrl+s") => Asks("SaveSecret"),
            (C::Edit, "Esc") => Asks("CancelEdit"),
            (C::Edit, "Tab" | "h ←") => To(C::Navigation),
            (C::Edit, "?") => Asks("ShowHelp"),

            (C::EditField, "Enter" | "Esc") => To(C::Edit),
            (C::EditField, "Alt+Enter" | "Ctrl+j" | "Tab") => Changes,
            (C::EditField, "Ctrl+s") => Asks("SaveSecret"),

            (C::FieldDeleteConfirm, "y") => Shows("30 fields"),
            (C::FieldDeleteConfirm, "n/Esc") => To(C::Edit),
            (C::CreatePrompt, "Enter") => Asks("CreateSecret"),
            (C::CreatePrompt, "Esc") => To(C::Navigation),
            (C::DeleteConfirm, "y") => Asks("DeleteSecret"),
            (C::DeleteConfirm, "n/Esc") => To(C::Navigation),
            (C::DiscardConfirm, "y") => To(C::Secret),
            (C::DiscardConfirm, "n/Esc") => To(C::Edit),
            (C::QuitConfirm, "y") => Quits,
            (C::QuitConfirm, "n/Esc") => To(C::Navigation),
            (C::Notice, "Esc/Enter") => To(C::Navigation),
            (C::LoadError, "Esc") => To(C::Navigation),

            (C::Policies, hint) if moves.contains(&hint) => Changes,
            (C::Policies, "Enter") => Asks("ViewPolicy"),
            (C::Policies, "Esc") => Asks("ScreenBack"),
            (C::Policies | C::PolicyContent, "?") => Asks("ToggleHelp"),
            (C::Policies | C::PolicyContent, "q") => Quits,
            (C::PolicyContent, hint) if moves.contains(&hint) => Asks("ScrollPolicy"),
            (C::PolicyContent, "y") => Asks("CopyToClipboard"),
            (C::PolicyContent, "Esc") => To(C::Policies),

            (C::Logs, hint) if moves.contains(&hint) => Asks("ScrollLogs"),
            (C::Logs, "a") => Asks("ToggleAutoScroll"),
            (C::Logs, "c") => Asks("ClearLogs"),
            (C::Logs, "Esc" | "~") => Asks("ScreenBack"),
            (C::Logs, "q") => Quits,

            (C::Help, hint) if moves.contains(&hint) => Changes,
            (C::Help, "Space") => Changes,
            (C::Help, "Esc/?/q") => Asks("HideHelp"),

            (C::Connecting, "q" | "Ctrl+c") => Quits,

            (context, hint) => panic!("no expected effect for {hint:?} in {context:?}"),
        }
    }

    /// The footer line, where the keys that work are hinted
    fn footer(root: &mut Root, app: &App) -> String {
        let text: Vec<char> = screen_text(root, app).chars().collect();
        text[text.len() - SCREEN_WIDTH..].iter().collect()
    }

    /// Whether the footer hints the keys of `context` (its first-priority
    /// hints, which tell the contexts apart)
    fn hints_of(footer: &str, context: Context) -> bool {
        keymap::bindings(context)
            .iter()
            .filter(|binding| binding.footer == 1)
            .all(|binding| footer.contains(&format!("{} {}", binding.keys, binding.label)))
    }

    /// The name of an action's variant
    fn action_name(action: &Action) -> String {
        let debug = format!("{action:?}");
        debug
            .split(['{', '(', ' '])
            .next()
            .unwrap_or_default()
            .to_string()
    }

    /// Press `key` and say whether it had `effect`
    fn key_does(root: &mut Root, app: &mut App, key: KeyEvent, effect: Effect) -> bool {
        let footer_before = footer(root, app);
        let text_before = screen_text(root, app);
        let cells_before = screen_cells(root, app);

        let action = root.handle_key_event(key, app).unwrap();
        let asked = action.as_ref().map(action_name);
        match action {
            // The log buffer is shared by every test in the process
            Some(Action::ClearLogs) | None => {}
            Some(action) => root.process_action(action, app).unwrap(),
        }

        let footer_after = footer(root, app);
        match effect {
            Effect::Asks(name) => asked.as_deref() == Some(name),
            Effect::To(context) => {
                !root.should_quit()
                    && footer_after != footer_before
                    && hints_of(&footer_after, context)
            }
            Effect::Changes => {
                !root.should_quit()
                    && footer_after == footer_before
                    && screen_cells(root, app) != cells_before
            }
            Effect::Shows(text) => {
                !text_before.contains(text) && screen_text(root, app).contains(text)
            }
            Effect::Quits => root.should_quit(),
        }
    }

    #[tokio::test]
    async fn every_hinted_key_does_what_its_help_says() {
        // The key table and the key handling are two pieces of code. This
        // presses every key of every hint, in the place the hint is shown
        // for, and fails for a key that does not do what `expected` says.
        let mut app = app_with_a_long_list();
        let mut pressed = 0;
        for context in Context::ALL {
            // Handled by the event loop before `Root` exists
            if context == Context::Connecting {
                continue;
            }
            assert!(
                !ways_into(context, &mut app).is_empty(),
                "{context:?} cannot be reached"
            );

            for binding in keymap::bindings(context) {
                for key in keys_of(binding.keys) {
                    let effect = expected(context, binding.keys, key);
                    let works = ways_into(context, &mut app)
                        .into_iter()
                        .any(|mut root| key_does(&mut root, &mut app, key, effect));
                    assert!(
                        works,
                        "{context:?}: {:?} ({}): {key:?} did not have the effect {effect:?}",
                        binding.keys, binding.help
                    );
                    pressed += 1;
                }
            }
        }
        assert!(pressed > 150, "only {pressed} keys were pressed");
        assert_eq!(
            app.state().current_path(),
            "/app",
            "the app was not to change"
        );
    }

    #[test]
    fn the_keys_hinted_on_the_connecting_screen_give_up() {
        for binding in keymap::bindings(Context::Connecting) {
            for key in keys_of(binding.keys) {
                assert!(matches!(
                    expected(Context::Connecting, binding.keys, key),
                    Effect::Quits
                ));
                assert!(crate::tui::gives_up_connecting(key), "{key:?}");
            }
        }
        // Other keys do not
        for key in [plain(Code::Char('x')), plain(Code::Esc), plain(Code::Enter)] {
            assert!(!crate::tui::gives_up_connecting(key), "{key:?}");
        }
    }

    #[tokio::test]
    async fn each_way_in_shows_the_hints_of_its_context() {
        // Guards the test above: it only means something if every setup
        // really is in the context it claims, which the footer shows
        let mut app = app_with_a_long_list();
        for context in Context::ALL {
            // The first hint that is in the footer at all
            let Some(binding) = keymap::bindings(context)
                .iter()
                .find(|binding| binding.footer == 1)
            else {
                continue;
            };
            let hint = format!("{} {}", binding.keys, binding.label);

            for mut root in ways_into(context, &mut app) {
                let text = screen_text(&mut root, &app);
                assert!(text.contains(&hint), "{context:?}: no {hint:?} on\n{text}");
            }
        }
    }

    #[tokio::test]
    async fn a_key_that_does_something_else_is_caught_by_the_same_check() {
        // The check must be able to fail: `x` does nothing in the list, and
        // `d` asks before deleting, it does not open the name prompt
        let mut app = app_with_a_long_list();
        let mut root = in_the_list(&mut app);
        assert!(!key_does(
            &mut root,
            &mut app,
            plain(Code::Char('x')),
            Effect::Changes
        ));
        let mut root = in_the_list(&mut app);
        assert!(!key_does(
            &mut root,
            &mut app,
            plain(Code::Char('d')),
            Effect::To(Context::CreatePrompt)
        ));
        let mut root = in_the_list(&mut app);
        assert!(key_does(
            &mut root,
            &mut app,
            plain(Code::Char('d')),
            Effect::To(Context::DeleteConfirm)
        ));
    }

    #[tokio::test]
    async fn ctrl_c_quits_from_anywhere() {
        let mut app = app_with_a_long_list();
        for context in Context::ALL {
            for mut root in ways_into(context, &mut app) {
                let ctrl_c = KeyEvent::new(Code::Char('c'), Mods::CONTROL);
                assert!(key_has_an_effect(&mut root, &mut app, ctrl_c));
                assert!(root.should_quit(), "{context:?}");
            }
        }
    }

    #[tokio::test]
    async fn declining_to_quit_returns_to_the_screen_q_was_pressed_on() {
        let mut app = app_with_a_long_list();
        let root = with_an_unsaved_change(&mut app);
        let mut root = after(&mut app, root, Action::ShowLogScreen);

        feed(&mut root, &mut app, &chars("q"));
        assert!(screen_text(&mut root, &app).contains(" Unsaved changes "));

        feed(&mut root, &mut app, &chars("n"));
        assert!(!root.should_quit());
        assert_eq!(root.active_screen, ActiveScreen::Logs);
    }

    #[tokio::test]
    async fn an_older_read_of_the_selected_secret_does_not_replace_a_newer_one() {
        // Regression: results were matched by path only. The preview of a
        // secret and the read after saving it could arrive in either order,
        // and the old content could end up on screen as if it were current.
        let mut app = test_app();
        let mut root = test_root(&app);

        root.process_action(view("/a"), &mut app).unwrap();
        let preview = stamped(&root, loaded("/a", "before_the_save"));
        let reload = Action::ReloadSecret {
            path: "/a".to_string(),
        };
        root.process_action(reload, &mut app).unwrap();
        let after_save = stamped(&root, loaded("/a", "after_the_save"));

        root.process_action(after_save, &mut app).unwrap();
        root.process_action(preview, &mut app).unwrap();

        let text = screen_text(&mut root, &app);
        assert!(text.contains("after_the_save"), "{text}");
        assert!(!text.contains("before_the_save"), "{text}");
    }
}
