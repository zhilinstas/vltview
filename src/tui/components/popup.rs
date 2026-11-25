//! Helpers shared by the popups and panes.

use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::tui::keymap::{self, Context};
use std::time::{SystemTime, UNIX_EPOCH};

/// A rectangle of the given size centred in `area`.
///
/// The size is fixed rather than a percentage of the screen, so a dialog
/// keeps all of its lines on a small terminal. It only shrinks when the
/// terminal itself is smaller than the dialog.
pub fn centered(width: u16, height: u16, area: Rect) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    }
}

/// Width of the dialogs drawn over the main screen
const DIALOG_WIDTH: u16 = 62;

/// Columns of text a dialog over `area` has
pub fn dialog_text_width(area: Rect) -> usize {
    (DIALOG_WIDTH.min(area.width) as usize).saturating_sub(4)
}

/// Draw a dialog over `area`: centred, of fixed width, as tall as its text.
///
/// Its last line is the keys that answer it, taken from the key table for
/// `keys` like every other hint, followed by `note` if there is one.
pub fn dialog(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    color: Color,
    body: Vec<Line<'static>>,
    keys: Context,
    note: Option<String>,
) {
    let mut hints = keymap::footer_line(&[keys], dialog_text_width(area) as u16);
    if let Some(note) = note {
        hints.spans.push(Span::styled(
            format!("   {note}"),
            Style::default().fg(Color::DarkGray),
        ));
    }

    let mut text = vec![Line::from("")];
    text.extend(body);
    text.extend([Line::from(""), hints]);

    let popup = centered(DIALOG_WIDTH, text.len() as u16 + 2, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(color))
        .title(Span::styled(
            format!(" {title} "),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ));

    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(text)
            .alignment(Alignment::Center)
            .block(block),
        popup,
    );
}

/// Border colour of a pane
pub fn border_color(focused: bool) -> Color {
    if focused {
        Color::Cyan
    } else {
        Color::DarkGray
    }
}

/// Style of the selected row. Only the focused pane gets the bright cursor,
/// so two panes never look active at once.
pub fn selection_style(focused: bool) -> Style {
    if focused {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White).bg(Color::DarkGray)
    }
}

/// Whether values are masked, for the bottom border of a pane that shows
/// them. Anything in clear is said in red.
pub fn visibility(show_all: bool, revealed: usize) -> Span<'static> {
    if show_all {
        Span::styled(
            "values VISIBLE",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )
    } else if revealed == 0 {
        Span::styled("values masked", Style::default().fg(Color::Green))
    } else {
        Span::styled(
            format!("{revealed} revealed"),
            Style::default().fg(Color::Red),
        )
    }
}

/// Current frame of the activity spinner
pub fn spinner_frame() -> &'static str {
    const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis())
        .unwrap_or(0);
    FRAMES[(millis / 100) as usize % FRAMES.len()]
}

/// Display width of a string in terminal columns
pub fn display_width(text: &str) -> usize {
    Span::raw(text).width()
}

/// Cut `text` to at most `width` columns, ending in "…" when it was cut
pub fn truncate(text: &str, width: usize) -> String {
    if display_width(text) <= width {
        return text.to_string();
    }
    if width == 0 {
        return String::new();
    }

    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let w = display_width(c.encode_utf8(&mut [0; 4]));
        if used + w > width.saturating_sub(1) {
            break;
        }
        out.push(c);
        used += w;
    }
    out.push('…');
    out
}

/// Break `text` into lines of at most `width` columns.
///
/// Line breaks in the text are kept; longer lines are cut at the column
/// limit, which suits secret values (keys, tokens, certificates) better than
/// breaking at spaces.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();

    for source in text.split('\n') {
        let mut current = String::new();
        let mut used = 0;
        for c in source.chars() {
            let w = display_width(c.encode_utf8(&mut [0; 4]));
            if used + w > width && !current.is_empty() {
                lines.push(std::mem::take(&mut current));
                used = 0;
            }
            current.push(c);
            used += w;
        }
        lines.push(current);
    }

    lines
}

/// Break a message into lines of at most `width` columns, at spaces.
///
/// For text that is read as sentences (errors, warnings): no word is split
/// unless it is longer than a whole line, and nothing is left out.
pub fn wrap_words(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();

    for source in text.split('\n') {
        let mut current = String::new();
        for word in source.split_whitespace() {
            let needed = display_width(&current) + usize::from(!current.is_empty());
            if !current.is_empty() && needed + display_width(word) > width {
                lines.push(std::mem::take(&mut current));
            }
            if !current.is_empty() {
                current.push(' ');
            }
            // A word longer than a line (a path, a URL) is cut at the edge
            let mut pieces = wrap(word, width);
            let last = pieces.pop().unwrap_or_default();
            if !pieces.is_empty() && !current.is_empty() {
                lines.push(std::mem::take(&mut current));
            }
            lines.extend(pieces);
            current.push_str(&last);
        }
        lines.push(current);
    }

    lines
}

/// Test helper: the words of `message` are on `screen` in order, none of
/// them cut. Borders and line breaks may come between them.
#[cfg(test)]
pub fn assert_shown_whole(screen: &str, message: &str) {
    let shown: Vec<&str> = screen
        .split(|c: char| c.is_whitespace() || c == '│')
        .filter(|word| !word.is_empty())
        .collect();
    let mut from = 0;
    for word in message.split_whitespace() {
        let found = shown[from..].iter().position(|shown| *shown == word);
        from += found.unwrap_or_else(|| panic!("{word:?} is missing or cut\n{screen}")) + 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centered_keeps_its_size_on_a_small_terminal() {
        let popup = centered(60, 9, Rect::new(0, 0, 80, 24));
        assert_eq!((popup.width, popup.height), (60, 9));
        assert_eq!((popup.x, popup.y), (10, 7));

        // Only a terminal smaller than the dialog shrinks it
        let popup = centered(60, 9, Rect::new(0, 0, 40, 6));
        assert_eq!((popup.width, popup.height), (40, 6));
    }

    #[test]
    fn truncate_marks_the_cut() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("a_rather_long_key", 8), "a_rathe…");
        assert_eq!(truncate("пароль-базы", 6), "парол…");
    }

    #[test]
    fn wrap_keeps_line_breaks_and_cuts_long_lines() {
        assert_eq!(wrap("ab\ncd", 10), vec!["ab", "cd"]);
        assert_eq!(wrap("abcdefgh", 3), vec!["abc", "def", "gh"]);
        assert_eq!(wrap("", 5), vec![""]);
        assert_eq!(wrap("a\n\nb", 5), vec!["a", "", "b"]);
    }

    #[test]
    fn messages_wrap_at_spaces_and_lose_nothing() {
        let text = "Could not open /team/payments: permission denied by the policy";
        let lines = wrap_words(text, 24);
        assert_eq!(
            lines,
            [
                "Could not open",
                "/team/payments:",
                "permission denied by the",
                "policy"
            ]
        );
        assert!(lines.iter().all(|line| display_width(line) <= 24));
        assert_eq!(lines.join(" "), text);
    }

    #[test]
    fn a_word_longer_than_a_line_is_cut_at_the_edge() {
        let lines = wrap_words("see /a/very/long/path/to/a/secret now", 10);
        assert_eq!(
            lines,
            ["see", "/a/very/lo", "ng/path/to", "/a/secret", "now"]
        );
    }
}
