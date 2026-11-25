use anyhow::Result;
use crossterm::{
    cursor::Show,
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
use std::sync::atomic::{AtomicBool, Ordering};
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

/// Shown in the footer when the UI opens with TLS verification off, until
/// the first key. The warning printed before the UI took the screen is out
/// of sight by then
const INSECURE_NOTICE: &str =
    "WARNING: TLS certificate verification is off, the connection is not secure";

/// Say inside the UI that TLS verification is off
fn show_insecure_notice(root: &mut Root, app: &mut App) -> Result<()> {
    root.process_action(
        action::Action::SetStatus {
            message: INSECURE_NOTICE.to_string(),
        },
        app,
    )
}

/// True while the terminal is in raw mode on the alternate screen
static TERMINAL_TAKEN: AtomicBool = AtomicBool::new(false);

/// Put the terminal into raw mode on the alternate screen.
///
/// From here on it is given back by `restore_terminal`: when the `Tui` is
/// dropped, which covers every error, and by the panic hook.
fn take_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    install_panic_hook();
    TERMINAL_TAKEN.store(true, Ordering::SeqCst);

    let taken = enable_raw_mode()
        // Bracketed paste: the terminal marks pasted text as such, so it
        // arrives as one `Event::Paste` instead of as typed keys
        .and_then(|()| execute!(io::stdout(), EnterAlternateScreen, EnableBracketedPaste))
        .and_then(|()| Terminal::new(CrosstermBackend::new(io::stdout())));

    if taken.is_err() {
        restore_terminal();
    }
    Ok(taken?)
}

/// Give the terminal back: main screen, cooked mode, cursor visible.
///
/// Does nothing the second time. Leaving the alternate screen twice would
/// move the cursor back to where it was before the UI started, on top of
/// whatever has been printed since (a panic message).
fn restore_terminal() {
    if !TERMINAL_TAKEN.swap(false, Ordering::SeqCst) {
        return;
    }

    let _ = disable_raw_mode();
    let _ = execute!(
        io::stdout(),
        DisableBracketedPaste,
        LeaveAlternateScreen,
        Show
    );
}

/// Report a panic where it can be read.
///
/// A panic on the thread that runs the UI ends the program: the terminal is
/// given back first, then the message and its location are printed as usual.
/// A panic in a background task ends only that task, so the UI keeps the
/// terminal and the message goes to the application log.
fn install_panic_hook() {
    let ui_thread = std::thread::current().id();
    let print = std::panic::take_hook();

    std::panic::set_hook(Box::new(move |info| {
        if std::thread::current().id() == ui_thread {
            restore_terminal();
            print(info);
        } else {
            // On one line: the default report has the message on a second
            let location = info.location().map(|at| at.to_string());
            tracing::error!(
                "Background task panicked at {}: {}",
                location.as_deref().unwrap_or("unknown location"),
                info.payload_as_str().unwrap_or("no message"),
            );
        }
    }));
}

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

    /// TLS certificate verification is off
    insecure: bool,
}

impl Tui {
    /// Create a new TUI instance
    ///
    /// The terminal is taken last. Whatever fails before that is reported on
    /// a terminal nobody has touched.
    pub fn new(config: Config, initial_path: String) -> Result<Self> {
        let insecure = config.insecure;

        // Create App layer
        let app = App::new(config, initial_path)?;

        // Try to create clipboard manager (may fail on headless systems)
        let clipboard = ClipboardManager::new().ok();

        // Create root component with runtime handle
        let runtime = tokio::runtime::Handle::current();
        let root = Root::new(runtime);

        let terminal = take_terminal()?;

        Ok(Self {
            terminal,
            app,
            clipboard,
            root,
            insecure,
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

        if self.insecure {
            show_insecure_notice(&mut self.root, &mut self.app)?;
        }

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
                            format!("  {}s  ", started.elapsed().as_secs()),
                            Style::default().fg(Color::DarkGray),
                        ),
                    ]);
                    // The keys that work here, from the same table as
                    // every other footer
                    let mut line = line;
                    line.spans.extend(
                        keymap::footer_line(&[keymap::Context::Connecting], frame.area().width)
                            .spans,
                    );
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
        // Route event to root component (Ctrl+C included)
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
        // Restore terminal on exit, whether it is a quit or an error
        restore_terminal();

        tracing::debug!("Terminal restored");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;

    #[tokio::test]
    async fn insecure_notice_is_whole_in_the_footer_at_80x24() {
        // The warning printed at startup is hidden behind the UI, so the UI
        // has to carry one of its own
        let config = Config {
            vault_addr: "http://127.0.0.1:1".to_string(),
            insecure: true,
            ..Config::default()
        };
        let mut app = App::new(config, "/".to_string()).unwrap();
        let mut root = Root::new(tokio::runtime::Handle::current());
        root.init(&app).unwrap();

        show_insecure_notice(&mut root, &mut app).unwrap();

        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|frame| root.render(frame, &app)).unwrap();
        let buffer = terminal.backend().buffer();
        let footer: String = (0..80).map(|x| buffer[(x, 23)].symbol()).collect();
        assert_eq!(
            footer.trim(),
            "WARNING: TLS certificate verification is off, the connection is not secure"
        );
    }
}
