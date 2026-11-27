use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::tui::action::Action;
use crate::tui::component::Component;
use crate::tui::components::popup::centered;
use crate::tui::context::{ComponentContext, RenderContext};
use crate::tui::keymap::{self, Context};

/// Width of the key column
const KEY_COLUMN: usize = 16;

/// Help overlay showing keybindings
///
/// A modal overlay listing every key, by the place it works in. The text
/// comes from the same table as the footers (`tui::keymap`). It scrolls, so
/// all of it can be read on a small terminal.
pub struct HelpOverlay {
    /// Whether this overlay is currently visible
    visible: bool,

    /// First line shown
    scroll: usize,

    /// Lines shown the last time it was drawn, for paging
    page: usize,
}

impl HelpOverlay {
    /// Create a new help overlay (hidden by default)
    pub fn new() -> Self {
        Self {
            visible: false,
            scroll: 0,
            page: 10,
        }
    }

    /// Every binding, one section per context
    fn lines() -> Vec<Line<'static>> {
        let mut lines = Vec::new();

        for context in Context::ALL {
            if !lines.is_empty() {
                lines.push(Line::from(""));
            }
            lines.push(Line::from(Span::styled(
                format!(" {}", context.title()),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )));

            for binding in keymap::bindings(context) {
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("   {:<KEY_COLUMN$}", binding.keys),
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(binding.help, Style::default().fg(Color::White)),
                ]));
            }
        }

        lines
    }
}

impl Component for HelpOverlay {
    fn handle_key(&mut self, key: KeyEvent, _ctx: &ComponentContext) -> Result<Option<Action>> {
        let page = self.page.max(1);

        match (key.code, key.modifiers) {
            (KeyCode::Esc, _)
            | (KeyCode::Char('?'), KeyModifiers::NONE)
            | (KeyCode::Char('q'), KeyModifiers::NONE) => return Ok(Some(Action::HideHelp)),

            (KeyCode::Char('j'), KeyModifiers::NONE) | (KeyCode::Down, _) => self.scroll += 1,
            (KeyCode::Char('k'), KeyModifiers::NONE) | (KeyCode::Up, _) => {
                self.scroll = self.scroll.saturating_sub(1)
            }
            (KeyCode::PageDown, _) | (KeyCode::Char(' '), _) => self.scroll += page,
            (KeyCode::PageUp, _) => self.scroll = self.scroll.saturating_sub(page),
            (KeyCode::Home, _) | (KeyCode::Char('g'), KeyModifiers::NONE) => self.scroll = 0,
            // Drawing clamps the offset to the last page
            (KeyCode::End, _) | (KeyCode::Char('G'), _) => self.scroll = usize::MAX / 2,

            // Ignore all other keys (modal behavior)
            _ => {}
        }

        Ok(None)
    }

    fn update(&mut self, action: Action, _ctx: &mut ComponentContext) -> Result<Option<Action>> {
        match action {
            Action::ShowHelp => {
                self.visible = true;
                self.scroll = 0;
            }
            Action::HideHelp => self.visible = false,
            Action::ToggleHelp => {
                self.visible = !self.visible;
                self.scroll = 0;
            }
            _ => {}
        }
        Ok(None)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect, _ctx: &RenderContext) {
        if !self.visible {
            return;
        }

        let lines = Self::lines();

        // As tall as the text needs, within the terminal
        let wanted_height = lines.len() as u16 + 2;
        let popup = centered(76, wanted_height.min(area.height.saturating_sub(2)), area);
        let inner_height = popup.height.saturating_sub(2) as usize;

        self.page = inner_height.max(1);
        self.scroll = self.scroll.min(lines.len().saturating_sub(inner_height));

        // Its own keys come from the table too
        let mut hints = vec![Span::raw(" ")];
        hints.extend(keymap::footer_line(&[Context::Help], popup.width.saturating_sub(24)).spans);
        if lines.len() > inner_height {
            let last = (self.scroll + inner_height).min(lines.len());
            hints.push(Span::styled(
                format!("  {}-{} of {}", self.scroll + 1, last, lines.len()),
                Style::default().fg(Color::DarkGray),
            ));
        }
        hints.push(Span::raw(" "));

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title(Span::styled(
                " VltView keys ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ))
            .title_bottom(Line::from(hints));

        let visible: Vec<Line> = lines
            .into_iter()
            .skip(self.scroll)
            .take(inner_height)
            .collect();

        frame.render_widget(Clear, popup);
        frame.render_widget(Paragraph::new(visible).block(block), popup);
    }

    fn captures_input(&self) -> bool {
        // When visible, capture all input (modal behavior)
        self.visible
    }
}

impl Default for HelpOverlay {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
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

    /// Text on screen after drawing the overlay at the given size
    fn rendered(overlay: &mut HelpOverlay, width: u16, height: u16) -> Vec<String> {
        let app = test_app();
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                let ctx = RenderContext::new(&app, true);
                overlay.draw(frame, area, &ctx);
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

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn help_lists_every_binding_from_the_keymap() {
        let text: String = HelpOverlay::lines()
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
                    + "\n"
            })
            .collect();

        for context in Context::ALL {
            assert!(text.contains(context.title()), "{context:?}");
            for binding in keymap::bindings(context) {
                assert!(text.contains(binding.help), "{}", binding.help);
            }
        }
    }

    #[test]
    fn every_line_can_be_reached_on_a_small_terminal() {
        // Regression: the overlay was 80% of the screen with no scrolling,
        // so at 80x24 more than half of the bindings could not be read.
        let app = test_app();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let ctx = ComponentContext::new(&app, &tx);

        let mut overlay = HelpOverlay::new();
        overlay.visible = true;

        let first = rendered(&mut overlay, 80, 24).join("\n");
        assert!(first.contains(Context::Global.title()), "{first}");
        assert!(!first.contains(Context::Connecting.title()));
        assert!(first.contains("j/k ↑/↓ scroll"), "{first}");
        assert!(first.contains("Esc/?/q close"), "{first}");
        assert!(first.contains("1-20 of"), "{first}");

        overlay.handle_key(key(KeyCode::End), &ctx).unwrap();
        let last = rendered(&mut overlay, 80, 24).join("\n");
        assert!(last.contains(Context::Connecting.title()), "{last}");
        assert!(last.contains("Give up and quit"), "{last}");

        overlay.handle_key(key(KeyCode::Home), &ctx).unwrap();
        assert_eq!(overlay.scroll, 0);
    }

    #[test]
    fn help_lines_fit_the_popup() {
        // 76 columns wide, 2 for the border
        for line in HelpOverlay::lines() {
            assert!(line.width() <= 74, "too wide: {line:?}");
        }
    }

    #[test]
    fn escape_and_question_mark_close_it() {
        let app = test_app();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let ctx = ComponentContext::new(&app, &tx);

        let mut overlay = HelpOverlay::new();
        for code in [KeyCode::Esc, KeyCode::Char('?'), KeyCode::Char('q')] {
            let action = overlay.handle_key(key(code), &ctx).unwrap();
            assert!(matches!(action, Some(Action::HideHelp)), "{code:?}");
        }
    }
}
