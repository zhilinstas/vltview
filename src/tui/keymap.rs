//! Key hints, in one place.
//!
//! Footers and the help overlay are both generated from the tables below, so
//! a hint cannot be shown where its key does nothing. The tables describe the
//! bindings; the dispatch itself lives in each component's `handle_key`, and
//! a binding changed there must be changed here too.

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

/// A place in the UI with its own set of keys
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Context {
    /// Keys that work on the main screen whichever pane has focus
    Global,
    /// Navigation pane
    Navigation,
    /// Navigation pane while the filter is being typed
    Filter,
    /// Navigation pane with a filter applied
    Filtered,
    /// Content pane showing a secret's fields
    Secret,
    /// Content pane showing a secret's version history
    Versions,
    /// Full value of one field
    ValueViewer,
    /// Edit view, moving between fields
    Edit,
    /// Edit view, typing a key or a value
    EditField,
    /// Edit view asking whether to delete a field
    FieldDeleteConfirm,
    /// Name prompt for a new secret
    CreatePrompt,
    /// Question before deleting a secret
    DeleteConfirm,
    /// Question before closing an editor that has unsaved changes
    DiscardConfirm,
    /// Question before quitting while an editor has unsaved changes
    QuitConfirm,
    /// An error message over the main screen
    Notice,
    /// Content pane saying that the selected secret could not be loaded
    LoadError,
    /// Policy list
    Policies,
    /// Content of one policy
    PolicyContent,
    /// Log viewer
    Logs,
    /// The help overlay itself
    Help,
    /// Startup screen, while the first folder is being listed
    Connecting,
}

impl Context {
    /// Every context, in the order the help overlay lists them
    pub const ALL: [Context; 21] = [
        Context::Global,
        Context::Navigation,
        Context::Filter,
        Context::Filtered,
        Context::Secret,
        Context::ValueViewer,
        Context::Versions,
        Context::Edit,
        Context::EditField,
        Context::FieldDeleteConfirm,
        Context::CreatePrompt,
        Context::DeleteConfirm,
        Context::DiscardConfirm,
        Context::QuitConfirm,
        Context::Notice,
        Context::LoadError,
        Context::Policies,
        Context::PolicyContent,
        Context::Logs,
        Context::Help,
        Context::Connecting,
    ];

    /// Section heading in the help overlay
    pub fn title(self) -> &'static str {
        match self {
            Context::Global => "Everywhere on the main screen",
            Context::Navigation => "Navigation pane",
            Context::Filter => "Filter (while typing)",
            Context::Filtered => "Navigation pane with a filter on",
            Context::Secret => "Secret",
            Context::Versions => "Version history",
            Context::ValueViewer => "Value viewer",
            Context::Edit => "Editing a secret",
            Context::EditField => "Editing a key or value",
            Context::FieldDeleteConfirm => "Deleting a field",
            Context::CreatePrompt => "New secret name",
            Context::DeleteConfirm => "Deleting a secret",
            Context::DiscardConfirm => "Closing an editor with unsaved changes",
            Context::QuitConfirm => "Quitting with unsaved changes",
            Context::Notice => "An error message",
            Context::LoadError => "A secret that could not be loaded",
            Context::Policies => "Policy list",
            Context::PolicyContent => "Policy content",
            Context::Logs => "Logs",
            Context::Help => "This help",
            Context::Connecting => "While connecting",
        }
    }
}

/// One key binding
pub struct Binding {
    /// Keys as shown to the user
    pub keys: &'static str,
    /// Short label for the footer
    pub label: &'static str,
    /// Description for the help overlay
    pub help: &'static str,
    /// Footer priority: 0 keeps the binding out of the footer, 1 is shown
    /// first, and larger numbers are dropped first when space runs out
    pub footer: u8,
}

const fn bind(keys: &'static str, label: &'static str, help: &'static str, footer: u8) -> Binding {
    Binding {
        keys,
        label,
        help,
        footer,
    }
}

const GLOBAL: &[Binding] = &[
    bind("Tab", "pane", "Switch between the list and the secret", 4),
    bind("h/l ←/→", "pane", "Focus the list / the secret", 0),
    bind("r", "refresh", "Reload the current folder", 6),
    bind("p", "policies", "Open the policy screen", 7),
    bind("~", "logs", "Open the application logs", 8),
    bind("?", "help", "Show this help", 2),
    bind("q", "quit", "Quit (asks if an edit has changes)", 3),
    bind("Ctrl+c", "quit", "Quit from anywhere, at once", 0),
];

const NAVIGATION: &[Binding] = &[
    bind(
        "j/k ↑/↓",
        "move",
        "Move the cursor; the secret under it is shown",
        0,
    ),
    bind("PgUp/PgDn", "page", "Move one page up / down", 0),
    bind("g/G Home/End", "ends", "Jump to the first / last item", 0),
    bind("Enter", "open", "Open the folder, or go into the secret", 1),
    bind("Backspace", "up", "Go to the parent folder", 5),
    bind("b/f", "history", "Go back / forward in history", 0),
    bind("/", "filter", "Filter this folder (fuzzy)", 1),
    bind("n", "new", "Create a secret in this folder", 2),
    bind("d", "delete", "Delete the selected secret", 3),
    bind("y", "copy path", "Copy the selected path", 5),
];

const FILTER: &[Binding] = &[
    bind("type", "filter", "Narrow the list as you type", 1),
    bind(
        "Enter",
        "keep",
        "Keep the filter and go back to the list",
        1,
    ),
    bind("Esc", "clear", "Clear the filter", 1),
];

const FILTERED: &[Binding] = &[
    bind("/", "edit filter", "Change the filter", 1),
    bind("Esc", "clear filter", "Show the whole folder again", 1),
];

const SECRET: &[Binding] = &[
    bind("j/k ↑/↓", "move", "Move between fields", 0),
    bind("PgUp/PgDn g/G", "page", "Page up / down, first / last", 0),
    bind(
        "Enter",
        "view",
        "Show the whole value (long and multi-line values)",
        1,
    ),
    bind("Space", "peek", "Reveal or hide the selected value", 1),
    bind("v", "show all", "Reveal or hide every value", 2),
    bind("c", "copy", "Copy the selected value", 1),
    bind("y", "copy", "The same as c", 0),
    bind("e", "edit", "Edit the secret", 1),
    bind("l", "history", "Show the version history", 2),
    bind("Esc", "list", "Go back to the list", 5),
];

const VERSIONS: &[Binding] = &[
    bind("j/k ↑/↓", "move", "Move between versions", 0),
    bind("Enter", "view", "Show the selected version", 1),
    bind(
        "n",
        "restore as new",
        "Edit a copy of this version; saving makes it current",
        1,
    ),
    bind("l/Esc", "fields", "Go back to the fields", 1),
];

const VALUE_VIEWER: &[Binding] = &[
    bind("j/k ↑/↓", "scroll", "Scroll", 1),
    bind("PgUp/PgDn g/G", "page", "Page up / down, top / bottom", 0),
    bind("c", "copy", "Copy the value", 1),
    bind("y", "copy", "The same as c", 0),
    bind("Esc", "close", "Close", 1),
    bind("Enter/q", "close", "The same as Esc", 0),
];

const EDIT: &[Binding] = &[
    bind("j/k ↑/↓", "move", "Move between fields", 0),
    bind("PgUp/PgDn g/G", "page", "Page up / down, first / last", 0),
    bind("Enter", "edit", "Edit the selected value", 1),
    bind("e", "edit", "The same as Enter", 0),
    bind("r", "rename", "Rename the selected key", 4),
    bind("a", "add", "Add a field", 2),
    bind("d", "delete", "Delete the selected field", 3),
    bind("Space", "peek", "Reveal or hide the selected value", 2),
    bind("v", "show all", "Reveal or hide every value", 2),
    bind("Ctrl+s", "save", "Save the secret to Vault", 1),
    bind(
        "Esc",
        "cancel",
        "Close the editor (asks if there are changes)",
        1,
    ),
    bind("Tab", "pane", "Switch to the list; the edit stays open", 5),
    bind("h ←", "list", "Focus the list; the edit stays open", 0),
    bind("?", "help", "Show this help", 0),
];

const EDIT_FIELD: &[Binding] = &[
    bind(
        "Enter",
        "done",
        "Keep the text (from a key, moves on to its value)",
        1,
    ),
    bind("Alt+Enter", "new line", "Insert a line break in a value", 3),
    bind("Ctrl+j", "new line", "The same as Alt+Enter", 0),
    bind(
        "Tab",
        "key/value",
        "Switch between the key and the value",
        2,
    ),
    bind("Ctrl+s", "save", "Keep the text and save the secret", 1),
    bind("Esc", "undo", "Discard what was typed in this field", 1),
];

const CREATE_PROMPT: &[Binding] = &[
    bind(
        "Enter",
        "create",
        "Open the editor; a/b also creates folder a",
        1,
    ),
    bind("Esc", "cancel", "Cancel", 1),
];

const FIELD_DELETE_CONFIRM: &[Binding] = &[
    bind("y", "delete", "Remove the field; Vault changes on save", 1),
    bind("n/Esc", "cancel", "Keep the field", 1),
];

const DELETE_CONFIRM: &[Binding] = &[
    bind("y", "delete", "Delete every version and the metadata", 1),
    bind("n/Esc", "cancel", "Keep the secret", 1),
];

const DISCARD_CONFIRM: &[Binding] = &[
    bind("y", "discard", "Close the editor and lose the changes", 1),
    bind("n/Esc", "keep editing", "Go back to the editor", 1),
];

const QUIT_CONFIRM: &[Binding] = &[
    bind("y", "quit", "Quit and lose the unsaved changes", 1),
    bind("n/Esc", "stay", "Go back to where you were", 1),
];

const NOTICE: &[Binding] = &[bind("Esc/Enter", "close", "Close the message", 1)];

const LOAD_ERROR: &[Binding] = &[bind("Esc", "dismiss", "Dismiss the error", 1)];

const HELP: &[Binding] = &[
    bind("j/k ↑/↓", "scroll", "Scroll", 1),
    bind("PgUp/PgDn g/G", "page", "Page up / down, top / bottom", 0),
    bind("Space", "page", "The same as PgDn", 0),
    bind("Esc/?/q", "close", "Close the help", 1),
];

const CONNECTING: &[Binding] = &[bind("q", "quit", "Give up and quit (also Ctrl+c)", 1)];

const POLICIES: &[Binding] = &[
    bind("j/k ↑/↓", "move", "Move between policies", 1),
    bind("PgUp/PgDn g/G", "page", "Page up / down, first / last", 0),
    bind("Enter", "view", "Show the policy", 1),
    bind("Esc", "back", "Back to the main screen", 1),
    bind("?", "help", "Show this help", 2),
    bind("q", "quit", "Quit", 2),
];

const POLICY_CONTENT: &[Binding] = &[
    bind("j/k ↑/↓", "scroll", "Scroll", 1),
    bind("PgUp/PgDn g/G", "page", "Page up / down, top / bottom", 0),
    bind("y", "copy", "Copy the policy", 1),
    bind("Esc", "back", "Back to the policy list", 1),
    bind("?", "help", "Show this help", 2),
    bind("q", "quit", "Quit", 2),
];

const LOGS: &[Binding] = &[
    bind("j/k ↑/↓", "scroll", "Scroll", 1),
    bind(
        "PgUp/PgDn g/G",
        "page",
        "Page up / down, oldest / newest",
        0,
    ),
    bind("a", "follow", "Follow new entries", 1),
    bind("c", "clear", "Clear the log", 1),
    bind("Esc", "close", "Back to the main screen", 1),
    bind("~", "close", "The same as Esc", 0),
    bind("q", "quit", "Quit", 2),
];

/// Bindings of one context
pub fn bindings(context: Context) -> &'static [Binding] {
    match context {
        Context::Global => GLOBAL,
        Context::Navigation => NAVIGATION,
        Context::Filter => FILTER,
        Context::Filtered => FILTERED,
        Context::Secret => SECRET,
        Context::Versions => VERSIONS,
        Context::ValueViewer => VALUE_VIEWER,
        Context::Edit => EDIT,
        Context::EditField => EDIT_FIELD,
        Context::FieldDeleteConfirm => FIELD_DELETE_CONFIRM,
        Context::CreatePrompt => CREATE_PROMPT,
        Context::DeleteConfirm => DELETE_CONFIRM,
        Context::DiscardConfirm => DISCARD_CONFIRM,
        Context::QuitConfirm => QUIT_CONFIRM,
        Context::Notice => NOTICE,
        Context::LoadError => LOAD_ERROR,
        Context::Policies => POLICIES,
        Context::PolicyContent => POLICY_CONTENT,
        Context::Logs => LOGS,
        Context::Help => HELP,
        Context::Connecting => CONNECTING,
    }
}

const SEPARATOR: &str = "  ";

fn hint_width(binding: &Binding) -> usize {
    binding.keys.chars().count() + 1 + binding.label.chars().count()
}

/// Bindings of `contexts` that fit in `width` columns, in table order.
///
/// When they do not all fit, the lowest-priority ones are left out; nothing
/// is ever cut in the middle.
fn footer_bindings(contexts: &[Context], width: usize) -> Vec<&'static Binding> {
    // A key means what the first context that binds it says
    let mut shown: Vec<&'static Binding> = Vec::new();
    for binding in contexts.iter().flat_map(|context| bindings(*context)) {
        if binding.footer > 0 && !shown.iter().any(|seen| seen.keys == binding.keys) {
            shown.push(binding);
        }
    }

    let total = |shown: &[&Binding]| {
        shown.iter().map(|b| hint_width(b)).sum::<usize>()
            + SEPARATOR.len() * shown.len().saturating_sub(1)
    };

    while shown.len() > 1 && total(&shown) > width {
        // Drop the last of the least important hints
        let (index, _) = shown
            .iter()
            .enumerate()
            .max_by_key(|(index, binding)| (binding.footer, *index))
            .expect("shown is not empty");
        shown.remove(index);
    }

    shown
}

/// One footer line of key hints for the given contexts
pub fn footer_line(contexts: &[Context], width: u16) -> Line<'static> {
    let key_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let label_style = Style::default().fg(Color::Gray);

    let mut spans = Vec::new();
    for binding in footer_bindings(contexts, width as usize) {
        if !spans.is_empty() {
            spans.push(Span::raw(SEPARATOR));
        }
        spans.push(Span::styled(binding.keys, key_style));
        spans.push(Span::styled(format!(" {}", binding.label), label_style));
    }

    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(line: &Line) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn every_context_has_bindings_and_a_footer() {
        for context in Context::ALL {
            assert!(!bindings(context).is_empty(), "{context:?}");
            if context != Context::Global {
                let line = footer_line(&[context], 200);
                assert!(!text(&line).is_empty(), "{context:?} has no footer hints");
            }
        }
    }

    #[test]
    fn footer_fits_the_width_and_keeps_whole_hints() {
        // Regression: fixed hint strings were cut off at 80 columns, hiding
        // "Ctrl+s: Save secret" in a 50-column pane.
        for context in Context::ALL {
            for width in [40_u16, 60, 78] {
                let contexts = [context, Context::Global];
                let line = text(&footer_line(&contexts, width));
                assert!(
                    line.chars().count() <= width as usize,
                    "{context:?} at {width}: {line}"
                );
                for hint in line.split(SEPARATOR) {
                    assert!(
                        contexts
                            .iter()
                            .flat_map(|c| bindings(*c))
                            .any(|b| format!("{} {}", b.keys, b.label) == hint),
                        "{context:?} at {width}: partial hint {hint:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_key_is_hinted_once_and_the_first_context_wins() {
        let line = text(&footer_line(
            &[Context::Filtered, Context::Navigation, Context::Global],
            200,
        ));
        assert!(line.contains("/ edit filter"), "{line}");
        assert!(line.contains("Esc clear filter"), "{line}");
        assert!(!line.contains("/ filter"), "{line}");
    }

    #[test]
    fn narrow_footer_keeps_the_most_important_hints() {
        let line = text(&footer_line(&[Context::Edit], 40));
        assert!(line.contains("Ctrl+s save"), "{line}");
        assert!(line.contains("Esc cancel"), "{line}");

        let line = text(&footer_line(&[Context::Navigation, Context::Global], 80));
        assert!(line.contains("Enter open"), "{line}");
        assert!(line.contains("? help"), "{line}");
        assert!(line.contains("q quit"), "{line}");
    }
}
