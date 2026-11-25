use anyhow::Result;
use crossterm::{
    event::{
        self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEvent, KeyModifiers,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};
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
pub mod keymap;
pub mod log_buffer;
pub mod root;

use root::Root;

/// How often the event loop wakes up when there is no input
const TICK: Duration = Duration::from_millis(50);

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
        // Bracketed paste: the terminal marks pasted text as such, so it
        // arrives as one `Event::Paste` instead of as typed keys
        execute!(stdout, EnterAlternateScreen, EnableBracketedPaste)?;
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
        if !self.connect().await? {
            return Ok(());
        }

        // Initialize root component
        self.root.init(&self.app)?;

        // Main event loop
        while !self.root.should_quit() {
            // Render the UI
            self.render()?;

            // Poll for events with timeout
            if event::poll(TICK)? {
                match event::read()? {
                    Event::Key(key) => self.handle_key_event(key).await?,
                    Event::Paste(text) => {
                        if let Some(action) = self.root.handle_paste(&text, &self.app)? {
                            self.process_action(action).await?;
                        }
                    }
                    _ => {}
                }
            }

            // Poll for async operation completions
            let completed_actions = self.root.poll_async_completions();
            for action in completed_actions {
                self.root.process_action(action, &mut self.app)?;
            }

            // Time-based work: the debounced preview, the busy indicator
            self.root.tick(&mut self.app)?;

            // Handle pending clipboard operations
            if let Some(text) = self.root.take_pending_clipboard() {
                if let Some(clipboard) = &mut self.clipboard {
                    match clipboard.copy_text(&text) {
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
                                action::Action::CopyFailed {
                                    message: format!("Could not copy to the clipboard: {}", e),
                                },
                                &mut self.app,
                            )?;
                        }
                    }
                } else {
                    self.root.process_action(
                        action::Action::CopyFailed {
                            message: "Nothing was copied: no clipboard is available".to_string(),
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

    /// Load the initial folder, showing what is being waited for.
    ///
    /// The first request can take as long as the request timeout. The screen
    /// says where it is connecting to meanwhile, and `q` or Ctrl+C gives up.
    /// Returns false if the user quit.
    async fn connect(&mut self) -> Result<bool> {
        let address = self.app.vault_client().base_url().to_string();
        let started = std::time::Instant::now();

        let Self { terminal, app, .. } = self;
        let initialize = app.initialize();
        tokio::pin!(initialize);

        loop {
            terminal
                .draw(|frame| {
                    let line = Line::from(vec![
                        Span::styled(
                            format!(" {} ", components::popup::spinner_frame()),
                            Style::default().fg(Color::Yellow),
                        ),
                        Span::raw("Connecting to "),
                        Span::styled(
                            address.as_str(),
                            Style::default()
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            format!("  {}s  (q to quit)", started.elapsed().as_secs()),
                            Style::default().fg(Color::DarkGray),
                        ),
                    ]);
                    frame.render_widget(Paragraph::new(vec![Line::from(""), line]), frame.area());
                })
                .map_err(|e| anyhow::anyhow!("Render error: {}", e))?;

            tokio::select! {
                result = &mut initialize => {
                    result?;
                    return Ok(true);
                }
                _ = tokio::time::sleep(TICK) => {
                    while event::poll(Duration::ZERO)? {
                        if let Event::Key(key) = event::read()? {
                            let ctrl_c = key.code == KeyCode::Char('c')
                                && key.modifiers == KeyModifiers::CONTROL;
                            if ctrl_c || key.code == KeyCode::Char('q') {
                                return Ok(false);
                            }
                        }
                    }
                }
            }
        }
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
        let _ = execute!(
            self.terminal.backend_mut(),
            DisableBracketedPaste,
            LeaveAlternateScreen
        );
        let _ = self.terminal.show_cursor();

        tracing::debug!("Terminal restored");
    }
}
