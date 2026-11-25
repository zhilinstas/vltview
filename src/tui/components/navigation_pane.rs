use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

use crate::app::App;
use crate::app::navigation::{child_path, parent_path};
use crate::tui::action::{Action, SelectionSource};
use crate::tui::component::Component;
use crate::tui::components::popup::{border_color, selection_style};
use crate::tui::context::{ComponentContext, RenderContext};

/// Navigation pane component for browsing Vault paths and secrets
///
/// The cursor is the selection: whenever it comes to rest on a different
/// item, for whatever reason, the pane announces it with
/// `Action::SelectionChanged` and the content pane follows. That covers the
/// cursor keys, reloads, filtering and moving between folders alike, so the
/// two panes cannot name different secrets.
pub struct NavigationPane {
    /// Items shown: `..` first when there is a parent, then the folder's
    /// entries, filtered if a filter is set
    items: Vec<String>,

    /// Number of entries in the folder before filtering
    total: usize,

    /// Currently selected index (into items list)
    selected_index: usize,

    /// Vertical scroll offset (first visible item index)
    scroll_offset: usize,

    /// Filter input text
    filter_input: String,

    /// Whether currently in filter mode (actively typing)
    filter_mode: bool,

    /// Folder the items belong to
    loaded_path: String,

    /// Selection last announced to the rest of the UI
    announced: Option<String>,

    /// Secret to put the cursor on when it shows up in the list (a secret
    /// that was just created)
    select_after_reload: Option<String>,

    /// Rows available for items the last time the pane was drawn
    viewport_height: usize,
}

impl NavigationPane {
    /// Create a new navigation pane
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            total: 0,
            selected_index: 0,
            scroll_offset: 0,
            filter_input: String::new(),
            filter_mode: false,
            loaded_path: String::new(),
            announced: None,
            select_after_reload: None,
            viewport_height: 10,
        }
    }

    /// Whether the filter is being typed
    pub fn is_filtering(&self) -> bool {
        self.filter_mode
    }

    /// Current filter text
    pub fn filter_text(&self) -> &str {
        &self.filter_input
    }

    /// Whether the folder has a parent, shown as `..`
    fn has_parent(&self) -> bool {
        self.loaded_path != "/" && !self.loaded_path.is_empty()
    }

    /// Rebuild the item list of the loaded folder from app state
    fn rebuild_items(&mut self, app: &App) {
        let mut items = app.state().list_items();
        self.total = items.len();

        if self.filter_input.is_empty() {
            // Folders first, then names, both case-insensitive
            items.sort_by_cached_key(|item| (!item.ends_with('/'), item.to_lowercase()));
        } else {
            use crate::app::FilterEngine;
            let mut filter = FilterEngine::new();
            filter.set_filter(self.filter_input.clone());
            // Best match first; names break ties
            items.sort_by_cached_key(|item| item.to_lowercase());
            items = filter.filter(&items);
        }

        if self.has_parent() {
            items.insert(0, "..".to_string());
        }

        self.items = items;
    }

    fn position(&self, name: &str) -> Option<usize> {
        self.items.iter().position(|item| item == name)
    }

    /// Reload the list after app state changed, keeping the cursor where the
    /// user left it:
    /// - same folder: on the same name, or on its neighbour if it is gone;
    /// - went up: on the folder just left;
    /// - anywhere else: on the first row.
    fn reload(&mut self, app: &App) {
        let path = app.state().current_path();
        let previous_name = self.selected_item().map(str::to_owned);
        let previous_index = self.selected_index;
        let previous_path = std::mem::replace(&mut self.loaded_path, path.clone());
        let path_changed = previous_path != path;

        if path_changed {
            // A filter belongs to the folder it was typed in
            self.filter_input.clear();
            self.filter_mode = false;
            self.scroll_offset = 0;
        }

        self.rebuild_items(app);

        let created = self
            .select_after_reload
            .take()
            .filter(|wanted| parent_path(wanted) == path)
            .and_then(|wanted| self.position(wanted.rsplit('/').next().unwrap_or_default()));

        let folder_left = (path_changed && parent_path(&previous_path) == path)
            .then(|| previous_path.rsplit('/').next().unwrap_or_default())
            .and_then(|name| self.position(&format!("{name}/")));

        let same_name = (!path_changed)
            .then_some(previous_name)
            .flatten()
            .and_then(|name| self.position(&name));

        let fallback = if path_changed {
            0
        } else {
            previous_index.min(self.items.len().saturating_sub(1))
        };

        self.selected_index = created.or(folder_left).or(same_name).unwrap_or(fallback);
    }

    /// Re-apply the filter and put the cursor on the best match
    fn filter_changed(&mut self, app: &App) -> Option<Action> {
        self.rebuild_items(app);
        // Row 0 is `..` when there is a parent; the first match comes after it
        self.selected_index = usize::from(self.has_parent() && self.items.len() > 1);
        self.scroll_offset = 0;
        self.announce(SelectionSource::ListChange)
    }

    /// Drop the filter, keeping the cursor on the item it was on
    fn clear_filter(&mut self, app: &App) -> Option<Action> {
        let selected = self.selected_item().map(str::to_owned);
        self.filter_input.clear();
        self.filter_mode = false;
        self.rebuild_items(app);
        self.selected_index = selected.and_then(|name| self.position(&name)).unwrap_or(0);
        self.announce(SelectionSource::ListChange)
    }

    /// Adjust scroll offset to keep selected item visible within viewport
    fn adjust_scroll(&mut self, viewport_height: usize) {
        if self.items.is_empty() || viewport_height == 0 {
            self.scroll_offset = 0;
            return;
        }

        if self.selected_index < self.scroll_offset {
            self.scroll_offset = self.selected_index;
        } else if self.selected_index >= self.scroll_offset + viewport_height {
            self.scroll_offset = self.selected_index + 1 - viewport_height;
        }
    }

    /// Move the cursor to `index`, clamped to the list
    fn select(&mut self, index: usize) -> Option<Action> {
        self.selected_index = index.min(self.items.len().saturating_sub(1));
        self.announce(SelectionSource::Cursor)
    }

    /// Get currently selected item
    fn selected_item(&self) -> Option<&str> {
        self.items.get(self.selected_index).map(|s| s.as_str())
    }

    /// Full path of the secret under the cursor, if the cursor is on one
    fn selected_secret(&self) -> Option<String> {
        let item = self.selected_item()?;
        (item != ".." && !item.ends_with('/')).then(|| child_path(&self.loaded_path, item))
    }

    /// Tell the rest of the UI about the selection if it changed
    fn announce(&mut self, source: SelectionSource) -> Option<Action> {
        let secret = self.selected_secret();
        if secret == self.announced {
            return None;
        }
        self.announced = secret.clone();
        Some(Action::SelectionChanged { secret, source })
    }

    /// Handle selection confirmation (Enter key)
    fn handle_select_confirmation(&self) -> Option<Action> {
        let item = self.selected_item()?;

        if item == ".." {
            Some(Action::NavigateUp)
        } else if item.ends_with('/') {
            Some(Action::NavigateTo {
                path: child_path(&self.loaded_path, item),
            })
        } else {
            // Go into the secret; it is loaded first if it is not shown yet
            Some(Action::OpenSecret {
                path: child_path(&self.loaded_path, item),
            })
        }
    }

    /// Handle filter mode input
    fn handle_filter_input(&mut self, key: KeyEvent, app: &App) -> Option<Action> {
        match key.code {
            // Enter, or '/' again, keeps the filter and returns to the list
            KeyCode::Enter | KeyCode::Char('/') => {
                self.filter_mode = false;
                None
            }
            KeyCode::Char(c) => {
                self.filter_input.push(c);
                self.filter_changed(app)
            }
            KeyCode::Backspace => {
                self.filter_input.pop();
                self.filter_changed(app)
            }
            KeyCode::Esc => self.clear_filter(app),
            _ => None,
        }
    }

    /// Handle delete action
    fn handle_delete(&self) -> Option<Action> {
        match self.selected_secret() {
            Some(path) => Some(Action::ShowDeleteConfirmation { item_name: path }),
            None => self.selected_item().map(|_| Action::SetStatus {
                message: "Only secrets can be deleted".to_string(),
            }),
        }
    }

    /// Handle copy path action
    fn handle_copy_path(&self) -> Option<Action> {
        let item = self.selected_item()?;
        let path = if item == ".." {
            parent_path(&self.loaded_path)
        } else {
            child_path(&self.loaded_path, item)
        };
        Some(Action::CopyToClipboard { text: path })
    }

    /// Create block with title, filter line and position
    fn create_block(&self, focused: bool) -> Block<'_> {
        let shown = self.items.len() - usize::from(self.has_parent());
        let title = if self.filter_input.is_empty() {
            format!(" Navigation ({}) ", self.total)
        } else {
            format!(" Navigation ({} of {}) ", shown, self.total)
        };

        let mut block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color(focused)))
            .title(title);

        if self.filter_mode || !self.filter_input.is_empty() {
            let mut spans = vec![Span::styled(" /", Style::default().fg(Color::Cyan))];
            if self.filter_mode {
                spans.push(Span::styled(
                    self.filter_input.as_str(),
                    Style::default().fg(Color::Yellow),
                ));
                spans.push(Span::styled(
                    " ",
                    Style::default().bg(Color::Yellow).fg(Color::Black),
                ));
                spans.push(Span::raw(" "));
            } else {
                // How to change or clear it is in the footer; the border
                // has no room for that in a narrow pane
                spans.push(Span::styled(
                    self.filter_input.as_str(),
                    Style::default().fg(Color::White),
                ));
                spans.push(Span::raw(" "));
            }
            block = block.title_bottom(Line::from(spans));
        }

        if !self.items.is_empty() {
            let position = format!(" {}/{} ", self.selected_index + 1, self.items.len());
            block = block.title_bottom(
                Line::from(Span::styled(position, Style::default().fg(Color::DarkGray)))
                    .right_aligned(),
            );
        }

        block
    }
}

impl Default for NavigationPane {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for NavigationPane {
    fn init(&mut self, ctx: &mut ComponentContext) -> Result<()> {
        // Load initial items from app state. If the cursor starts on a
        // secret, the content pane shows it like any other selection.
        self.reload(ctx.app);
        if let Some(action) = self.announce(SelectionSource::ListChange) {
            let _ = ctx.action_tx.send(action);
        }
        Ok(())
    }

    fn handle_key(&mut self, key: KeyEvent, ctx: &ComponentContext) -> Result<Option<Action>> {
        // Filter mode captures input first
        if self.filter_mode {
            return Ok(self.handle_filter_input(key, ctx.app));
        }

        let page = self.viewport_height.max(1);
        let last = self.items.len().saturating_sub(1);

        let action = match (key.code, key.modifiers) {
            // Cursor movement
            (KeyCode::Up, KeyModifiers::NONE) | (KeyCode::Char('k'), KeyModifiers::NONE) => {
                self.select(self.selected_index.saturating_sub(1))
            }
            (KeyCode::Down, KeyModifiers::NONE) | (KeyCode::Char('j'), KeyModifiers::NONE) => {
                self.select(self.selected_index + 1)
            }
            (KeyCode::PageUp, _) => self.select(self.selected_index.saturating_sub(page)),
            (KeyCode::PageDown, _) => self.select(self.selected_index + page),
            (KeyCode::Home, _) | (KeyCode::Char('g'), KeyModifiers::NONE) => self.select(0),
            (KeyCode::End, _) | (KeyCode::Char('G'), _) => self.select(last),

            // Open the folder, or go into the secret
            (KeyCode::Enter, KeyModifiers::NONE) => self.handle_select_confirmation(),

            // Go to parent
            (KeyCode::Backspace, KeyModifiers::NONE) => Some(Action::NavigateUp),

            // History navigation
            (KeyCode::Char('b'), KeyModifiers::NONE) => Some(Action::HistoryBack),
            (KeyCode::Char('f'), KeyModifiers::NONE) => Some(Action::HistoryForward),

            // Create new secret (triggers input mode for name)
            (KeyCode::Char('n'), KeyModifiers::NONE) => Some(Action::ShowCreateSecretInput),

            // Delete
            (KeyCode::Char('d'), KeyModifiers::NONE) => self.handle_delete(),

            // Copy path
            (KeyCode::Char('y'), KeyModifiers::NONE) => self.handle_copy_path(),

            // Start typing the filter; an existing filter is edited
            (KeyCode::Char('/'), KeyModifiers::NONE) => {
                self.filter_mode = true;
                None
            }

            // Escape clears an active filter
            (KeyCode::Esc, KeyModifiers::NONE) if !self.filter_input.is_empty() => {
                self.clear_filter(ctx.app)
            }

            _ => None,
        };

        Ok(action)
    }

    fn handle_paste(&mut self, text: &str, ctx: &ComponentContext) -> Result<Option<Action>> {
        if !self.filter_mode {
            return Ok(None);
        }
        // A filter is one line; take the first of whatever was pasted
        let line = text.lines().next().unwrap_or_default();
        self.filter_input
            .extend(line.chars().filter(|c| !c.is_control()));
        Ok(self.filter_changed(ctx.app))
    }

    fn update(&mut self, action: Action, ctx: &mut ComponentContext) -> Result<Option<Action>> {
        match action {
            // App state changed: a folder was loaded or reloaded
            Action::StateUpdated => {
                self.reload(ctx.app);
                Ok(self.announce(SelectionSource::ListChange))
            }

            // Put the cursor on a secret that was just created, once the
            // reload that follows the save brings it into the list. A secret
            // that is already listed was edited: the cursor stays where the
            // user has it.
            Action::SecretSaved { path } => {
                let listed = ctx
                    .app
                    .state()
                    .list_items()
                    .iter()
                    .any(|item| child_path(&self.loaded_path, item) == path);
                if !listed {
                    self.select_after_reload = Some(path);
                }
                Ok(None)
            }

            _ => Ok(None),
        }
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect, ctx: &RenderContext) {
        let focused = ctx.is_focused;

        // Viewport height (subtract 2 for borders)
        let viewport_height = area.height.saturating_sub(2) as usize;
        self.viewport_height = viewport_height;

        // Adjust scroll to keep selected item visible
        self.adjust_scroll(viewport_height);

        let block = self.create_block(focused);

        if self.items.is_empty() {
            let message = if self.filter_input.is_empty() {
                "No secrets at this path".to_string()
            } else {
                format!("Nothing matches \"{}\"", self.filter_input)
            };
            let empty = Paragraph::new(message)
                .style(Style::default().fg(Color::Yellow))
                .block(block);
            frame.render_widget(empty, area);
            return;
        }

        let end_index = (self.scroll_offset + viewport_height).min(self.items.len());
        let visible_items = &self.items[self.scroll_offset..end_index];

        let mut list_items: Vec<ListItem> = visible_items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let is_selected = self.scroll_offset + i == self.selected_index;

                // Folders, secrets and `..` each look different
                let style = if is_selected {
                    selection_style(focused)
                } else if item == ".." {
                    Style::default().fg(Color::DarkGray)
                } else if item.ends_with('/') {
                    Style::default()
                        .fg(Color::LightBlue)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };

                // The style is on the item, so the whole row is highlighted
                ListItem::new(Line::from(format!(" {item}"))).style(style)
            })
            .collect();

        // Only `..` left: say why the folder looks empty
        if !self.filter_input.is_empty() && self.has_parent() && self.items.len() == 1 {
            list_items.push(
                ListItem::new(Line::from(format!(
                    " Nothing matches \"{}\"",
                    self.filter_input
                )))
                .style(Style::default().fg(Color::Yellow)),
            );
        }

        frame.render_widget(List::new(list_items).block(block), area);
    }

    fn captures_input(&self) -> bool {
        // Capture input when in filter mode
        self.filter_mode
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use tokio::sync::mpsc::unbounded_channel;

    fn test_app(path: &str, items: &[&str]) -> App {
        let config = Config {
            vault_addr: "http://127.0.0.1:1".to_string(),
            vault_token: "test-token".to_string().into(),
            ..Config::default()
        };
        let app = App::new(config, path.to_string()).unwrap();
        set_items(&app, items);
        app
    }

    fn set_items(app: &App, items: &[&str]) {
        let state = app.state();
        state.reload_folder(
            &state.current_path(),
            items.iter().map(|s| s.to_string()).collect(),
        );
    }

    fn show_folder(app: &App, path: &str, items: &[&str]) {
        app.state().show_folder(
            path.to_string(),
            items.iter().map(|s| s.to_string()).collect(),
        );
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    /// Run `body` with a pane that has loaded `app`'s current folder.
    /// The second argument collects what `init` announced.
    fn with_pane(
        app: &App,
        body: impl FnOnce(&mut NavigationPane, &mut ComponentContext, Option<Action>),
    ) {
        let (tx, mut rx) = unbounded_channel();
        let mut ctx = ComponentContext::new(app, &tx);
        let mut pane = NavigationPane::new();
        pane.init(&mut ctx).unwrap();
        let announced = rx.try_recv().ok();
        body(&mut pane, &mut ctx, announced);
    }

    fn selection(action: Option<Action>) -> Option<Option<String>> {
        match action {
            Some(Action::SelectionChanged { secret, .. }) => Some(secret),
            None => None,
            other => panic!("expected a selection change, got {other:?}"),
        }
    }

    #[test]
    fn folders_are_listed_before_secrets() {
        let app = test_app("/app", &["zeta", "beta/", "Alpha", "alpha-dir/"]);
        with_pane(&app, |pane, _, _| {
            assert_eq!(pane.items, ["..", "alpha-dir/", "beta/", "Alpha", "zeta"]);
        });
    }

    #[test]
    fn cursor_on_a_secret_at_startup_is_announced() {
        // Regression: the pane told the user to "press Enter on a secret",
        // and nothing was loaded until the cursor was moved.
        let app = test_app("/", &["db", "web"]);
        with_pane(&app, |_, _, announced| {
            assert_eq!(selection(announced), Some(Some("/db".to_string())));
        });

        // On `..` there is nothing to show
        let app = test_app("/app", &["db"]);
        with_pane(&app, |_, _, announced| assert!(announced.is_none()));
    }

    #[test]
    fn moving_the_cursor_announces_each_new_selection_once() {
        let app = test_app("/app", &["dir/", "db", "web"]);
        with_pane(&app, |pane, ctx, _| {
            // `..` to a folder: still no secret selected
            assert!(pane.handle_key(key(KeyCode::Down), ctx).unwrap().is_none());

            let action = pane.handle_key(key(KeyCode::Char('j')), ctx).unwrap();
            assert_eq!(selection(action), Some(Some("/app/db".to_string())));

            let action = pane.handle_key(key(KeyCode::End), ctx).unwrap();
            assert_eq!(selection(action), Some(Some("/app/web".to_string())));

            // Already at the end: nothing new to announce
            assert!(pane.handle_key(key(KeyCode::Down), ctx).unwrap().is_none());

            // Back onto a folder: the content pane must be told to clear
            let action = pane.handle_key(key(KeyCode::Home), ctx).unwrap();
            assert_eq!(selection(action), Some(None));
        });
    }

    #[test]
    fn reload_keeps_the_cursor_on_the_same_secret() {
        // Regression: every reload put the cursor on row 0 while the content
        // pane kept showing the secret that had been selected.
        let app = test_app("/app", &["a", "m", "z"]);
        with_pane(&app, |pane, ctx, _| {
            pane.handle_key(key(KeyCode::End), ctx).unwrap();
            assert_eq!(pane.selected_item(), Some("z"));

            // New entries shift the rows; the cursor follows the name
            set_items(ctx.app, &["a", "b", "c", "m", "z"]);
            let action = pane.update(Action::StateUpdated, ctx).unwrap();
            assert_eq!(pane.selected_item(), Some("z"));
            assert!(action.is_none(), "same secret, nothing to announce");
        });
    }

    #[test]
    fn reload_after_delete_moves_to_the_neighbour() {
        let app = test_app("/app", &["a", "m", "z"]);
        with_pane(&app, |pane, ctx, _| {
            pane.handle_key(key(KeyCode::Down), ctx).unwrap();
            pane.handle_key(key(KeyCode::Down), ctx).unwrap();
            assert_eq!(pane.selected_item(), Some("m"));

            set_items(ctx.app, &["a", "z"]);
            let action = pane.update(Action::StateUpdated, ctx).unwrap();
            assert_eq!(pane.selected_item(), Some("z"));
            assert_eq!(selection(action), Some(Some("/app/z".to_string())));

            // Deleting the last row moves up instead
            set_items(ctx.app, &["a"]);
            let action = pane.update(Action::StateUpdated, ctx).unwrap();
            assert_eq!(pane.selected_item(), Some("a"));
            assert_eq!(selection(action), Some(Some("/app/a".to_string())));
        });
    }

    #[test]
    fn going_up_lands_on_the_folder_just_left() {
        let app = test_app("/app/team", &["db"]);
        with_pane(&app, |pane, ctx, _| {
            show_folder(ctx.app, "/app", &["alpha/", "team/", "zeta/", "readme"]);
            pane.update(Action::StateUpdated, ctx).unwrap();

            assert_eq!(pane.selected_item(), Some("team/"));
        });
    }

    #[test]
    fn entering_a_folder_starts_at_the_top_and_drops_the_filter() {
        let app = test_app("/app", &["prod/", "prod-db", "staging/"]);
        with_pane(&app, |pane, ctx, _| {
            pane.handle_key(key(KeyCode::Char('/')), ctx).unwrap();
            for c in "prod".chars() {
                pane.handle_key(key(KeyCode::Char(c)), ctx).unwrap();
            }
            pane.handle_key(key(KeyCode::Enter), ctx).unwrap();
            assert_eq!(pane.filter_text(), "prod");

            // Regression: the filter stayed on in the next folder and hid
            // everything in it that did not match
            show_folder(ctx.app, "/app/prod", &["api", "worker"]);
            pane.update(Action::StateUpdated, ctx).unwrap();

            assert_eq!(pane.filter_text(), "");
            assert_eq!(pane.items, ["..", "api", "worker"]);
            assert_eq!(pane.selected_item(), Some(".."));
        });
    }

    #[test]
    fn filter_puts_the_cursor_on_the_first_match() {
        // Regression: after filtering the cursor sat on `..`, so Enter (keep
        // the filter) then Enter (open) went up a folder.
        let app = test_app("/app", &["alpha", "database", "web"]);
        with_pane(&app, |pane, ctx, _| {
            pane.handle_key(key(KeyCode::Char('/')), ctx).unwrap();
            let action = pane.handle_key(key(KeyCode::Char('d')), ctx).unwrap();

            assert_eq!(pane.selected_item(), Some("database"));
            assert_eq!(selection(action), Some(Some("/app/database".to_string())));

            pane.handle_key(key(KeyCode::Enter), ctx).unwrap();
            let action = pane.handle_key(key(KeyCode::Enter), ctx).unwrap();
            assert!(
                matches!(&action, Some(Action::OpenSecret { path }) if path == "/app/database"),
                "{action:?}"
            );
        });
    }

    #[test]
    fn slash_edits_the_filter_and_escape_clears_it() {
        let app = test_app("/app", &["alpha", "database", "web"]);
        with_pane(&app, |pane, ctx, _| {
            pane.handle_key(key(KeyCode::Char('/')), ctx).unwrap();
            pane.handle_key(key(KeyCode::Char('d')), ctx).unwrap();
            pane.handle_key(key(KeyCode::Enter), ctx).unwrap();
            assert!(!pane.is_filtering());

            // Regression: the hint said "/ to edit" but `/` cleared the text
            pane.handle_key(key(KeyCode::Char('/')), ctx).unwrap();
            assert!(pane.is_filtering());
            assert_eq!(pane.filter_text(), "d");
            pane.handle_key(key(KeyCode::Char('a')), ctx).unwrap();
            assert_eq!(pane.filter_text(), "da");

            // Esc drops the filter and keeps the cursor on the match
            pane.handle_key(key(KeyCode::Esc), ctx).unwrap();
            assert_eq!(pane.filter_text(), "");
            assert_eq!(pane.items.len(), 4);
            assert_eq!(pane.selected_item(), Some("database"));
        });
    }

    #[test]
    fn enter_opens_the_secret_under_the_cursor() {
        let app = test_app("/app", &["dir/", "db"]);
        with_pane(&app, |pane, ctx, _| {
            let action = pane.handle_key(key(KeyCode::Enter), ctx).unwrap();
            assert!(matches!(action, Some(Action::NavigateUp)));

            pane.handle_key(key(KeyCode::Down), ctx).unwrap();
            let action = pane.handle_key(key(KeyCode::Enter), ctx).unwrap();
            assert!(
                matches!(&action, Some(Action::NavigateTo { path }) if path == "/app/dir"),
                "{action:?}"
            );

            pane.handle_key(key(KeyCode::Down), ctx).unwrap();
            let action = pane.handle_key(key(KeyCode::Enter), ctx).unwrap();
            assert!(
                matches!(&action, Some(Action::OpenSecret { path }) if path == "/app/db"),
                "{action:?}"
            );
        });
    }

    #[test]
    fn a_saved_secret_is_selected_when_it_appears() {
        let app = test_app("/app", &["a", "z"]);
        with_pane(&app, |pane, ctx, _| {
            let saved = Action::SecretSaved {
                path: "/app/m".to_string(),
            };
            pane.update(saved, ctx).unwrap();

            set_items(ctx.app, &["a", "m", "z"]);
            let action = pane.update(Action::StateUpdated, ctx).unwrap();
            assert_eq!(pane.selected_item(), Some("m"));
            assert_eq!(selection(action), Some(Some("/app/m".to_string())));
        });
    }

    #[test]
    fn saving_an_existing_secret_does_not_move_the_cursor() {
        // Regression: every save pulled the cursor back to the saved secret
        // on the next reload, even after the user had moved on.
        let app = test_app("/app", &["a", "m", "z"]);
        with_pane(&app, |pane, ctx, _| {
            pane.handle_key(key(KeyCode::End), ctx).unwrap();
            assert_eq!(pane.selected_item(), Some("z"));

            let saved = Action::SecretSaved {
                path: "/app/m".to_string(),
            };
            pane.update(saved, ctx).unwrap();
            let action = pane.update(Action::StateUpdated, ctx).unwrap();

            assert_eq!(pane.selected_item(), Some("z"));
            assert!(action.is_none(), "{action:?}");
        });
    }

    #[test]
    fn page_keys_move_by_the_viewport() {
        let names: Vec<String> = (0..50).map(|i| format!("s{i:02}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let app = test_app("/", &refs);
        with_pane(&app, |pane, ctx, _| {
            pane.viewport_height = 10;
            pane.handle_key(key(KeyCode::PageDown), ctx).unwrap();
            assert_eq!(pane.selected_index, 10);
            pane.handle_key(key(KeyCode::PageUp), ctx).unwrap();
            assert_eq!(pane.selected_index, 0);
            pane.handle_key(KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT), ctx)
                .unwrap();
            assert_eq!(pane.selected_index, 49);
            pane.handle_key(key(KeyCode::Char('g')), ctx).unwrap();
            assert_eq!(pane.selected_index, 0);
        });
    }
}
