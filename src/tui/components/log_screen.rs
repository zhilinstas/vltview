use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use super::popup;
use crate::tui::action::{Action, ScrollDirection};
use crate::tui::component::Component;
use crate::tui::context::{ComponentContext, RenderContext};
use crate::tui::keymap::{self, Context};
use crate::tui::log_buffer::{LogBuffer, LogEntry};

/// Columns before the text of an entry: "HH:MM:SS.mmm LEVEL "
const PREFIX_WIDTH: usize = 19;

/// Log viewer screen component
///
/// Displays application logs with scrolling and auto-scroll functionality.
/// An entry that is wider than the screen continues on the next rows, so
/// scrolling counts rows, not entries.
pub struct LogScreen {
    /// Entries to show
    logs: LogBuffer,
    /// First row shown when not following
    scroll_offset: usize,
    /// Whether to automatically scroll to newest logs
    auto_scroll: bool,
    /// Rows of log shown the last time the screen was drawn
    viewport_height: usize,
    /// All entries laid out in rows, as of the last time the screen was drawn
    rows: Vec<Line<'static>>,
    /// Number of entries in `rows`
    entry_count: usize,
    /// Buffer revision and text width `rows` were laid out for
    laid_out: Option<(u64, usize)>,
}

impl LogScreen {
    /// Create a new log screen
    pub fn new() -> Self {
        Self::with_buffer(LogBuffer::global())
    }

    /// Create a log screen showing the given buffer
    pub fn with_buffer(logs: LogBuffer) -> Self {
        Self {
            logs,
            scroll_offset: 0,
            auto_scroll: true,
            viewport_height: 20,
            rows: Vec::new(),
            entry_count: 0,
            laid_out: None,
        }
    }

    /// Scroll up by a few lines
    fn scroll_up(&mut self) {
        self.auto_scroll = false;
        if self.scroll_offset > 0 {
            self.scroll_offset = self.scroll_offset.saturating_sub(5);
        }
    }

    /// Scroll down by a few lines
    fn scroll_down(&mut self, total_lines: usize, viewport_height: usize) {
        self.auto_scroll = false;
        let max_offset = total_lines.saturating_sub(viewport_height);
        if self.scroll_offset < max_offset {
            self.scroll_offset = (self.scroll_offset + 5).min(max_offset);
        }
    }

    /// Enable auto-scroll (follow latest logs)
    fn enable_auto_scroll(&mut self) {
        self.auto_scroll = true;
    }

    /// Clear logs and reset state
    fn clear(&mut self) {
        self.logs.clear();
        self.scroll_offset = 0;
        self.auto_scroll = true;
    }

    fn render_content(&mut self, frame: &mut Frame, area: Rect) {
        let text_width = (area.width.saturating_sub(2) as usize).saturating_sub(PREFIX_WIDTH);

        // The screen is drawn many times a second and the buffer can hold
        // thousands of entries: lay them out again only when they or the
        // width have changed
        let revision = self.logs.revision();
        if self.laid_out != Some((revision, text_width)) {
            let entries = self.logs.entries();
            self.rows = entries
                .iter()
                .flat_map(|entry| entry_rows(entry, text_width))
                .collect();
            self.entry_count = entries.len();
            self.laid_out = Some((revision, text_width));
        }

        self.viewport_height = area.height.saturating_sub(2) as usize;

        let last_page = self.rows.len().saturating_sub(self.viewport_height);
        if !self.auto_scroll {
            self.scroll_offset = self.scroll_offset.min(last_page);
        }
        let first = if self.auto_scroll {
            last_page
        } else {
            self.scroll_offset
        };
        let last = (first + self.viewport_height).min(self.rows.len());

        let position = if self.auto_scroll {
            "following".to_string()
        } else {
            format!("rows {}-{} of {}", first + 1, last, self.rows.len())
        };
        let noun = if self.entry_count == 1 {
            "entry"
        } else {
            "entries"
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" Logs: {} {noun}, {position} ", self.entry_count));

        let content = if self.rows.is_empty() {
            Paragraph::new(Span::styled(
                "No logs to display",
                Style::default().fg(Color::DarkGray),
            ))
        } else {
            Paragraph::new(self.rows[first..last].to_vec())
        };
        frame.render_widget(content.block(block), area);
    }

    fn render_footer(&self, frame: &mut Frame, area: Rect) {
        let mut line = keymap::footer_line(&[Context::Logs], area.width.saturating_sub(2));
        line.spans.insert(0, Span::raw(" "));
        frame.render_widget(Paragraph::new(line), area);
    }
}

impl Component for LogScreen {
    fn handle_key(&mut self, key: KeyEvent, _ctx: &ComponentContext) -> Result<Option<Action>> {
        match (key.code, key.modifiers) {
            // Scrolling
            (KeyCode::Char('j'), KeyModifiers::NONE) | (KeyCode::Down, KeyModifiers::NONE) => {
                Ok(Some(Action::ScrollLogs {
                    direction: ScrollDirection::Down,
                }))
            }
            (KeyCode::Char('k'), KeyModifiers::NONE) | (KeyCode::Up, KeyModifiers::NONE) => {
                Ok(Some(Action::ScrollLogs {
                    direction: ScrollDirection::Up,
                }))
            }

            (KeyCode::PageDown, _) => Ok(Some(Action::ScrollLogs {
                direction: ScrollDirection::PageDown,
            })),
            (KeyCode::PageUp, _) => Ok(Some(Action::ScrollLogs {
                direction: ScrollDirection::PageUp,
            })),
            (KeyCode::Home, _) | (KeyCode::Char('g'), KeyModifiers::NONE) => {
                Ok(Some(Action::ScrollLogs {
                    direction: ScrollDirection::Top,
                }))
            }
            (KeyCode::End, _) | (KeyCode::Char('G'), _) => Ok(Some(Action::ScrollLogs {
                direction: ScrollDirection::Bottom,
            })),

            // Auto-scroll toggle
            (KeyCode::Char('a'), KeyModifiers::NONE) => Ok(Some(Action::ToggleAutoScroll)),

            // Clear logs
            (KeyCode::Char('c'), KeyModifiers::NONE) => Ok(Some(Action::ClearLogs)),

            // Close log screen
            (KeyCode::Esc, KeyModifiers::NONE) | (KeyCode::Char('~'), KeyModifiers::NONE) => {
                Ok(Some(Action::ScreenBack))
            }

            // Quit
            (KeyCode::Char('q'), KeyModifiers::NONE) => Ok(Some(Action::Quit)),

            _ => Ok(None),
        }
    }

    fn update(&mut self, action: Action, _ctx: &mut ComponentContext) -> Result<Option<Action>> {
        match action {
            Action::ScrollLogs { direction } => {
                let total_rows = self.rows.len();
                let viewport_height = self.viewport_height;
                let page_steps = (viewport_height / 5).max(1);

                // While following, the offset is not kept up to date. Start
                // from what is on screen, so the first scroll moves from
                // there instead of jumping to the oldest entry.
                if self.auto_scroll {
                    self.scroll_offset = total_rows.saturating_sub(viewport_height);
                }

                match direction {
                    ScrollDirection::Up => self.scroll_up(),
                    ScrollDirection::Down => self.scroll_down(total_rows, viewport_height),
                    ScrollDirection::PageUp => {
                        for _ in 0..page_steps {
                            self.scroll_up();
                        }
                    }
                    ScrollDirection::PageDown => {
                        for _ in 0..page_steps {
                            self.scroll_down(total_rows, viewport_height);
                        }
                    }
                    ScrollDirection::Top => {
                        self.scroll_offset = 0;
                        self.auto_scroll = false;
                    }
                    ScrollDirection::Bottom => {
                        self.auto_scroll = true;
                    }
                }
                Ok(None)
            }

            Action::ToggleAutoScroll => {
                self.enable_auto_scroll();
                Ok(None)
            }

            Action::ClearLogs => {
                self.clear();
                Ok(None)
            }

            _ => Ok(None),
        }
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect, _ctx: &RenderContext) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(0),    // Content
                Constraint::Length(1), // Footer
            ])
            .split(area);

        self.render_content(frame, chunks[0]);
        self.render_footer(frame, chunks[1]);
    }
}

/// The rows of one entry: time and level, then what it says in at most
/// `width` columns per row.
///
/// The action comes first, then the message, then the other fields as
/// `name=value`. Words that do not fit move to the next row, under the text
/// of the first; nothing is cut off.
fn entry_rows(entry: &LogEntry, width: usize) -> Vec<Line<'static>> {
    let level_color = match entry.level.as_str() {
        "ERROR" => Color::Red,
        "WARN" => Color::Yellow,
        "INFO" => Color::Green,
        "DEBUG" => Color::Cyan,
        "TRACE" => Color::Gray,
        _ => Color::White,
    };
    let prefix = vec![
        Span::styled(
            format!("{} ", entry.timestamp),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            format!("{:<5} ", entry.level),
            Style::default()
                .fg(level_color)
                .add_modifier(Modifier::BOLD),
        ),
    ];

    let action = entry.field("action").map(|action| {
        let style = Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD);
        (action.to_string(), style)
    });
    let message = entry
        .message
        .split_whitespace()
        .map(|word| (word.to_string(), Style::default().fg(Color::White)));
    let fields = entry
        .fields
        .iter()
        .filter(|(name, _)| name != "action")
        .map(|(name, value)| (format!("{name}={value}"), Style::default().fg(Color::Gray)));

    let mut rows = Rows {
        rows: vec![prefix],
        used: 0,
        width,
    };
    for (word, style) in action.into_iter().chain(message).chain(fields) {
        let mut rest = word.as_str();

        if popup::display_width(rest) > width && rows.used > 0 && rows.room() >= 2 {
            // Longer than a whole row, like a deep path: start it here
            // rather than leave most of this row empty
            let head = popup::wrap(rest, rows.room()).swap_remove(0);
            rest = &rest[head.len()..];
            rows.push(head, style);
        }

        for piece in popup::wrap(rest, width) {
            if rows.used > 0 && popup::display_width(&piece) > rows.room() {
                rows.start_row();
            }
            rows.push(piece, style);
        }
    }

    rows.rows.into_iter().map(Line::from).collect()
}

/// The rows of an entry while words are put on them
struct Rows {
    rows: Vec<Vec<Span<'static>>>,
    /// Columns of text on the last row
    used: usize,
    /// Columns of text a row holds
    width: usize,
}

impl Rows {
    /// Columns the next word can take on the last row
    fn room(&self) -> usize {
        match self.used {
            0 => self.width,
            // A space goes before it
            used => self.width.saturating_sub(used + 1),
        }
    }

    /// Continue under the text of the first row
    fn start_row(&mut self) {
        self.rows.push(vec![Span::raw(" ".repeat(PREFIX_WIDTH))]);
        self.used = 0;
    }

    fn push(&mut self, word: String, style: Style) {
        let Some(row) = self.rows.last_mut() else {
            return;
        };
        if self.used > 0 {
            row.push(Span::raw(" "));
            self.used += 1;
        }
        self.used += popup::display_width(&word);
        row.push(Span::styled(word, style));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use crate::config::Config;
    use crate::tui::log_buffer::capture;
    use crate::vault::audit_log;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn test_log_screen_creation() {
        let screen = LogScreen::new();
        assert_eq!(screen.scroll_offset, 0);
        assert!(screen.auto_scroll);
    }

    #[test]
    fn test_scroll_up() {
        let mut screen = LogScreen::new();
        screen.scroll_offset = 10;

        screen.scroll_up();
        assert_eq!(screen.scroll_offset, 5);
        assert!(!screen.auto_scroll);
    }

    #[test]
    fn test_scroll_down() {
        let mut screen = LogScreen::new();
        screen.scroll_offset = 0;

        screen.scroll_down(100, 20);
        assert_eq!(screen.scroll_offset, 5);
        assert!(!screen.auto_scroll);
    }

    #[test]
    fn test_auto_scroll() {
        let mut screen = LogScreen::new();
        screen.auto_scroll = false;

        screen.enable_auto_scroll();
        assert!(screen.auto_scroll);
    }

    fn test_app() -> App {
        let config = Config {
            vault_addr: "http://127.0.0.1:1".to_string(),
            vault_token: "test-token".to_string().into(),
            ..Config::default()
        };
        App::new(config, "/".to_string()).unwrap()
    }

    /// Text on screen after drawing the log screen at 80x24, one string per row
    fn rendered(screen: &mut LogScreen) -> Vec<String> {
        let (width, height) = (80, 24);
        let app = test_app();
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                let ctx = RenderContext::new(&app, true);
                screen.draw(frame, area, &ctx);
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

    /// What `emit` logs, in a buffer of its own
    fn logged(emit: impl FnOnce()) -> LogBuffer {
        let (buffer, _guard) = capture();
        emit();
        buffer
    }

    fn send(screen: &mut LogScreen, action: Action) {
        let app = test_app();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut ctx = ComponentContext::new(&app, &tx);
        screen.update(action, &mut ctx).unwrap();
    }

    #[test]
    fn an_audit_entry_shows_its_action_path_and_count_at_80_columns() {
        let buffer = logged(|| {
            audit_log::log_secret_list("/team/billing/prod", 3);
            audit_log::log_secret_read("/team/billing/prod/database");
            audit_log::log_secret_write("/team/billing/prod/database", 4);
            audit_log::log_clipboard_copy(24, 30);
        });

        let screen = rendered(&mut LogScreen::with_buffer(buffer)).join("\n");

        assert!(
            screen.contains("INFO  secret.list path=/team/billing/prod count=3"),
            "{screen}"
        );
        assert!(
            screen.contains("INFO  secret.read path=/team/billing/prod/database "),
            "{screen}"
        );
        assert!(
            screen.contains("INFO  secret.write path=/team/billing/prod/database fields=4"),
            "{screen}"
        );
        assert!(
            screen.contains("INFO  clipboard.copy bytes=24 timeout_secs=30"),
            "{screen}"
        );
    }

    #[test]
    fn an_ordinary_entry_shows_its_message_and_its_fields() {
        let buffer = logged(|| tracing::warn!(attempt = 2, "Failed to load metadata"));

        let screen = rendered(&mut LogScreen::with_buffer(buffer)).join("\n");

        assert!(
            screen.contains("WARN  Failed to load metadata attempt=2"),
            "{screen}"
        );
    }

    #[test]
    fn a_path_longer_than_the_line_is_wrapped_not_cut() {
        let path = "/team/platform/billing/production/eu-central-1/postgres/primary/replication/credentials";
        let buffer = logged(|| {
            audit_log::log_secret_read(path);
            tracing::info!("The entry after it");
        });

        let rows = rendered(&mut LogScreen::with_buffer(buffer));

        // The text between the borders, with the indent of continuation
        // rows removed, reads as the whole entry
        let first = rows
            .iter()
            .position(|row| row.contains("secret.read"))
            .expect("the entry is on screen");
        let last = rows
            .iter()
            .position(|row| row.contains("The entry after it"))
            .expect("the next entry is on screen");
        assert!(last > first + 1, "the entry takes more than one row");
        let entry: String = rows[first..last]
            .iter()
            .map(|row| row.trim_matches(|c: char| c == '│' || c == ' '))
            .collect();
        assert!(
            entry.ends_with(&format!("INFO  secret.read path={path}")),
            "{entry}"
        );
    }

    #[test]
    fn the_newest_entry_is_on_screen_when_older_entries_take_several_rows() {
        let buffer = logged(|| {
            for i in 0..30 {
                tracing::info!(
                    "Entry {i} is a long sentence that does not fit into a single row of \
                     a terminal that is eighty columns wide"
                );
            }
            tracing::info!("The newest entry");
        });

        let screen = rendered(&mut LogScreen::with_buffer(buffer)).join("\n");

        assert!(screen.contains("The newest entry"), "{screen}");
        assert!(screen.contains("31 entries"), "{screen}");
    }

    #[test]
    fn first_scroll_starts_from_what_is_on_screen() {
        // Regression: while following, the offset stayed at 0, so the first
        // `k` jumped from the newest entries to the oldest.
        let buffer = logged(|| {
            for i in 0..40 {
                tracing::info!("entry {i:02}.");
            }
        });
        let mut screen = LogScreen::with_buffer(buffer);

        let following = rendered(&mut screen).join("\n");
        assert!(following.contains("entry 39."), "{following}");

        let up = Action::ScrollLogs {
            direction: ScrollDirection::Up,
        };
        send(&mut screen, up);
        let scrolled = rendered(&mut screen).join("\n");

        // One step (five rows) above the last page, not at the top
        assert!(scrolled.contains("entry 34."), "{scrolled}");
        assert!(!scrolled.contains("entry 35."), "{scrolled}");
        assert!(!scrolled.contains("entry 00."), "{scrolled}");
    }

    #[test]
    fn an_entry_logged_while_the_screen_is_open_is_on_the_next_draw() {
        let (buffer, _guard) = capture();
        tracing::info!("The first entry");
        let mut screen = LogScreen::with_buffer(buffer);
        let before = rendered(&mut screen).join("\n");
        assert!(before.contains("The first entry"), "{before}");

        tracing::info!("The second entry");
        let after = rendered(&mut screen).join("\n");

        assert!(after.contains("The second entry"), "{after}");
    }

    #[test]
    fn clearing_empties_the_screen() {
        let buffer = logged(|| tracing::info!("The only entry"));
        let mut screen = LogScreen::with_buffer(buffer);
        let before = rendered(&mut screen).join("\n");
        assert!(before.contains("The only entry"), "{before}");

        send(&mut screen, Action::ClearLogs);
        let after = rendered(&mut screen).join("\n");

        assert!(!after.contains("The only entry"), "{after}");
        assert!(after.contains("No logs to display"), "{after}");
    }
}
