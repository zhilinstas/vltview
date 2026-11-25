use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io;
use std::time::Duration;

use crate::app::App;
use crate::clipboard::ClipboardManager;
use crate::config::Config;

// Component architecture modules
pub mod action;
pub mod async_manager;
pub mod component;
pub mod components;
pub mod context;
pub mod log_buffer;
pub mod root;

use root::Root;

/// Main TUI application struct (new component-based architecture)
pub struct Tui {
    /// Terminal instance
    terminal: Terminal<CrosstermBackend<io::Stdout>>,

    /// Application core (business logic layer)
    app: App,

    /// Clipboard manager
    clipboard: Option<ClipboardManager>,

    /// Root component managing all screens
    root: Root,
}

impl Tui {
    /// Create a new TUI instance
    pub fn new(config: Config, initial_path: String) -> Result<Self> {
        // Setup terminal
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend)?;

        // Create App layer
        let app = App::new(config, initial_path)?;

        // Try to create clipboard manager (may fail on headless systems)
        let clipboard = ClipboardManager::new().ok();

        // Create root component with runtime handle
        let runtime = tokio::runtime::Handle::current();
        let root = Root::new(runtime);

        Ok(Self {
            terminal,
            app,
            clipboard,
            root,
        })
    }

    /// Run the TUI application
    pub async fn run(&mut self) -> Result<()> {
        tracing::info!("Starting TUI event loop (component architecture)");

        // Initialize app (load initial data)
        self.app.initialize().await?;

        // Initialize root component
        self.root.init(&self.app)?;

        // Main event loop
        while !self.root.should_quit() {
            // Render the UI
            self.render()?;

            // Poll for events with timeout
            if event::poll(Duration::from_millis(100))?
                && let Event::Key(key) = event::read()?
            {
                self.handle_key_event(key).await?;
            }

            // Poll for async operation completions
            let completed_actions = self.root.poll_async_completions();
            for action in completed_actions {
                self.root.process_action(action, &mut self.app)?;
            }

            // Handle pending clipboard operations
            if let Some(text) = self.root.take_pending_clipboard() {
                if let Some(clipboard) = &mut self.clipboard {
                    match clipboard.copy_text(&text).await {
                        Ok(()) => {
                            self.root.process_action(
                                action::Action::SetStatus {
                                    message: "Copied to clipboard (auto-clears in 30s)".to_string(),
                                },
                                &mut self.app,
                            )?;
                        }
                        Err(e) => {
                            self.root.process_action(
                                action::Action::SetError {
                                    message: format!("Failed to copy: {}", e),
                                },
                                &mut self.app,
                            )?;
                        }
                    }
                } else {
                    self.root.process_action(
                        action::Action::SetError {
                            message: "Clipboard not available".to_string(),
                        },
                        &mut self.app,
                    )?;
                }
            }
        }

        // Shutdown
        self.app.shutdown().await?;

        Ok(())
    }

    /// Handle keyboard events
    async fn handle_key_event(&mut self, key: KeyEvent) -> Result<()> {
        // Global quit with Ctrl+C (works from anywhere)
        if key.code == KeyCode::Char('c') && key.modifiers == KeyModifiers::CONTROL {
            self.root
                .process_action(action::Action::Quit, &mut self.app)?;
            return Ok(());
        }

        // Route event to root component
        if let Some(action) = self.root.handle_key_event(key, &self.app)? {
            // Process the action
            self.process_action(action).await?;
        }

        Ok(())
    }

    /// Process an action (may trigger async operations or side effects)
    async fn process_action(&mut self, action: action::Action) -> Result<()> {
        // Handle ShowPolicyScreen - load policies async
        if matches!(action, action::Action::ShowPolicyScreen) {
            // Transition to screen first
            self.root.process_action(action, &mut self.app)?;
            // Then trigger async load
            self.root
                .process_action(action::Action::LoadPolicies, &mut self.app)?;
            return Ok(());
        }

        // Process action in root
        self.root.process_action(action, &mut self.app)?;

        Ok(())
    }

    /// Render the current UI state
    fn render(&mut self) -> Result<()> {
        self.terminal
            .draw(|frame| {
                self.root.render(frame, &self.app);
            })
            .map_err(|e| anyhow::anyhow!("Render error: {}", e))?;

        Ok(())
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        // Restore terminal on exit
        let _ = disable_raw_mode();
        let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen);
        let _ = self.terminal.show_cursor();

        tracing::debug!("Terminal restored");
    }
}
