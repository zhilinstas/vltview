use anyhow::Result;
use crossterm::event::KeyEvent;
use ratatui::Frame;

use crate::app::App;
use crate::tui::action::Action;
use crate::tui::async_manager::AsyncManager;
use crate::tui::component::Component;
use crate::tui::components::{HelpOverlay, LogScreen, MainScreen, PolicyScreen};
use crate::tui::context::{ComponentContext, FocusState, RenderContext, ActionSender};

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

    /// Process an action
    ///
    /// This updates component state based on actions. Some actions trigger
    /// screen transitions or other side effects, including async operations.
    pub fn process_action(&mut self, action: Action, app: &mut App) -> Result<()> {
        let action_tx = self.action_sender();
        let mut ctx = ComponentContext::new(app, &self.focus, &action_tx);

        match &action {
            // Async operations - spawn and return
            Action::ViewSecret { path, should_focus } => {
                let path = path.clone();
                let should_focus = *should_focus;
                // Drop ctx before mutable borrow
                drop(ctx);
                self.spawn_load_secret(app, path, should_focus);
                // Set loading state
                let action_tx = self.action_sender();
                let mut ctx = ComponentContext::new(app, &self.focus, &action_tx);
                let _ = self.main_screen.update(Action::SetLoading { loading: true }, &mut ctx);
                return Ok(());
            }

            Action::LoadPolicies => {
                self.spawn_load_policies(app);
                return Ok(());
            }

            Action::ViewPolicy { name } => {
                self.spawn_load_policy(app, name.clone());
                return Ok(());
            }

            Action::SaveSecret { path, data } => {
                // Spawn async save operation
                let _ = self.main_screen.update(
                    Action::SetLoading { loading: true },
                    &mut ctx,
                );
                drop(ctx);
                self.spawn_save_secret(app, path.clone(), data.clone());
                return Ok(());
            }

            Action::ViewSecretVersion { path, version } => {
                let _ = self.main_screen.update(
                    Action::SetLoading { loading: true },
                    &mut ctx,
                );
                drop(ctx);
                self.spawn_load_secret_version(app, path.clone(), *version);
                return Ok(());
            }

            Action::CreateFromVersion { path, version } => {
                let _ = self.main_screen.update(
                    Action::SetLoading { loading: true },
                    &mut ctx,
                );
                drop(ctx);
                self.spawn_load_version_for_edit(app, path.clone(), *version);
                return Ok(());
            }

            Action::DeleteSecret { path } => {
                self.spawn_delete_secret(app, path.clone());
                return Ok(());
            }

            Action::NavigateTo { path } => {
                // Set loading state and spawn async navigation
                let _ = self.main_screen.update(
                    Action::SetLoading { loading: true },
                    &mut ctx,
                );
                drop(ctx);
                self.spawn_navigate_to(app, path.clone());
                return Ok(());
            }

            Action::NavigateUp => {
                let current = app.state().current_path();
                if current != "/" {
                    let parent = if let Some(pos) = current.rfind('/') {
                        if pos == 0 {
                            "/".to_string()
                        } else {
                            current[..pos].to_string()
                        }
                    } else {
                        "/".to_string()
                    };
                    let _ = self.main_screen.update(
                        Action::SetLoading { loading: true },
                        &mut ctx,
                    );
                    drop(ctx);
                    self.spawn_navigate_to(app, parent);
                }
                return Ok(());
            }

            Action::Refresh => {
                // Spawn async operation to reload current directory from Vault
                let _ = self.main_screen.update(
                    Action::SetLoading { loading: true },
                    &mut ctx,
                );
                drop(ctx);
                self.spawn_refresh(app);
                return Ok(());
            }

            Action::HistoryBack => {
                if let Some(path) = app.state().go_back() {
                    let _ = self.main_screen.update(
                        Action::SetLoading { loading: true },
                        &mut ctx,
                    );
                    drop(ctx);
                    self.spawn_navigate_to(app, path);
                }
                return Ok(());
            }

            Action::HistoryForward => {
                if let Some(path) = app.state().go_forward() {
                    let _ = self.main_screen.update(
                        Action::SetLoading { loading: true },
                        &mut ctx,
                    );
                    drop(ctx);
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

            // Main screen actions
            Action::SecretLoaded { .. }
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
            | Action::StartFilter
            | Action::UpdateFilter { .. }
            | Action::ConfirmFilter
            | Action::CancelFilter
            | Action::FocusPane { .. }
            | Action::ToggleValueVisibility
            | Action::ToggleDetailMode
            | Action::CopySelectedValue
            | Action::SecretVersionLoaded { .. }
            | Action::VersionLoadedForEdit { .. }
            | Action::StartEditValue
            | Action::StartEditKey
            | Action::ConfirmEdit
            | Action::CancelEdit
            | Action::ToggleEditTarget
            | Action::AddField
            | Action::DeleteField => {
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

            Action::ScrollPolicy { .. } => {
                if self.active_screen == ActiveScreen::Policy {
                    self.policy_screen.update(action, &mut ctx)?;
                }
            }

            Action::CopyPolicy => {
                if self.active_screen == ActiveScreen::Policy {
                    // This action is handled by the old Tui for now (clipboard integration)
                    // In future phases, this would be handled by Root with a clipboard service
                }
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
    fn spawn_load_secret(&mut self, app: &App, path: String, _should_focus: bool) {
        let vault_client = app.vault_client().clone();

        self.async_manager.spawn(
            async move {
                // Load secret data
                let secret_data = vault_client.kv().get(&path).await?;

                // Convert VaultSecret to HashMap<String, String>
                let data: std::collections::HashMap<String, String> = secret_data
                    .data
                    .iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect();

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
            move |e| Action::SetError {
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
                Ok(Action::Batch {
                    actions: vec![
                        Action::StateUpdated,
                        Action::SetLoading { loading: false },
                    ],
                })
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
                Ok(Action::Batch {
                    actions: vec![
                        Action::StateUpdated,
                        Action::SetLoading { loading: false },
                    ],
                })
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
    fn spawn_save_secret(&mut self, app: &App, path: String, data: std::collections::HashMap<String, String>) {
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
                        Action::Refresh,  // Refresh navigation list to show new secret
                        Action::ViewSecret {
                            path: path.clone(),
                            should_focus: false,
                        },  // Reload secret to show new version in history
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

                Ok(Action::Batch {
                    actions: vec![
                        Action::SecretVersionLoaded {
                            version,
                            data,
                        },
                        Action::SetLoading { loading: false },
                    ],
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

                Ok(Action::Batch {
                    actions: vec![
                        Action::VersionLoadedForEdit {
                            path: path.clone(),
                            data,
                        },
                        Action::SetLoading { loading: false },
                    ],
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
    use tokio::runtime::Runtime;

    #[test]
    fn test_root_creation() {
        let rt = Runtime::new().unwrap();
        let root = Root::new(rt.handle().clone());

        assert_eq!(root.active_screen(), ActiveScreen::Main);
        assert!(!root.should_quit());
    }

    #[test]
    fn test_screen_transition() {
        let rt = Runtime::new().unwrap();
        let mut root = Root::new(rt.handle().clone());

        // Mock app (in real tests, we'd create a proper mock)
        // For now, this demonstrates the structure
        assert_eq!(root.active_screen(), ActiveScreen::Main);

        // Transition to logs would happen via process_action
        root.active_screen = ActiveScreen::Logs;
        assert_eq!(root.active_screen(), ActiveScreen::Logs);
    }
}
