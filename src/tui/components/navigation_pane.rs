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
use crate::tui::action::{Action, FocusedPane};
use crate::tui::component::Component;
use crate::tui::context::{ComponentContext, RenderContext};

/// Navigation pane component for browsing Vault paths and secrets
///
/// Features:
/// - Vertical scrolling with viewport (shows subset of items)
/// - Filter mode with fuzzy matching
/// - Auto-preview on selection change
/// - Item selection and navigation (directories, secrets, parent "..")
pub struct NavigationPane {
    /// Cached items from App state (already sorted and filtered)
    items: Vec<String>,

    /// Currently selected index (into items list)
    selected_index: usize,

    /// Vertical scroll offset (first visible item index)
    scroll_offset: usize,

    /// Filter input text
    filter_input: String,

    /// Whether currently in filter mode (actively typing)
    filter_mode: bool,

    /// Loading state
    is_loading: bool,
}

impl NavigationPane {
    /// Create a new navigation pane
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            selected_index: 0,
            scroll_offset: 0,
            filter_input: String::new(),
            filter_mode: false,
            is_loading: false,
        }
    }

    /// Get items from app state (with ".." prepended if not at root, filtered if active)
    fn update_items_from_app(&mut self, app: &App) {
        let mut items = app.state().list_items();
        let current_path = app.state().current_path();

        // Sort items alphabetically (case-insensitive)
        items.sort_by_key(|a| a.to_lowercase());

        // Apply filter if filter text is present
        if !self.filter_input.is_empty() {
            use crate::app::FilterEngine;
            let mut filter = FilterEngine::new();
            filter.set_filter(self.filter_input.clone());
            items = filter.filter(&items);
        }

        // Add ".." at the beginning if not at root
        if current_path != "/" && !current_path.is_empty() {
            items.insert(0, "..".to_string());
        }

        self.items = items;

        // Ensure selected index is valid
        if self.selected_index >= self.items.len() && !self.items.is_empty() {
            self.selected_index = self.items.len() - 1;
        }
    }

    /// Adjust scroll offset to keep selected item visible within viewport
    fn adjust_scroll(&mut self, viewport_height: usize) {
        if self.items.is_empty() {
            self.scroll_offset = 0;
            return;
        }

        // Ensure selected item is visible
        if self.selected_index < self.scroll_offset {
            // Selected item is above viewport, scroll up
            self.scroll_offset = self.selected_index;
        } else if self.selected_index >= self.scroll_offset + viewport_height {
            // Selected item is below viewport, scroll down
            self.scroll_offset = self.selected_index.saturating_sub(viewport_height - 1);
        }
    }

    /// Move selection up
    fn move_up(&mut self) -> Option<Action> {
        if self.items.is_empty() {
            return None;
        }

        if self.selected_index > 0 {
            self.selected_index -= 1;
            Some(Action::SelectPrevious)
        } else {
            None
        }
    }

    /// Move selection down
    fn move_down(&mut self) -> Option<Action> {
        if self.items.is_empty() {
            return None;
        }

        if self.selected_index + 1 < self.items.len() {
            self.selected_index += 1;
            Some(Action::SelectNext)
        } else {
            None
        }
    }

    /// Get currently selected item
    fn selected_item(&self) -> Option<&str> {
        self.items.get(self.selected_index).map(|s| s.as_str())
    }

    /// Check if selected item is a secret (not ".." or directory)
    fn is_selected_secret(&self) -> bool {
        if let Some(item) = self.selected_item() {
            item != ".." && !item.ends_with('/')
        } else {
            false
        }
    }

    /// Handle selection confirmation (Enter key)
    fn handle_select_confirmation(&self, ctx: &ComponentContext) -> Result<Option<Action>> {
        let Some(item) = self.selected_item() else {
            return Ok(None);
        };

        if item == ".." {
            // Navigate to parent
            Ok(Some(Action::NavigateUp))
        } else if item.ends_with('/') {
            // Navigate into directory
            let current_path = ctx.app.state().current_path();
            let new_path = format!(
                "{}/{}",
                current_path.trim_end_matches('/'),
                item.trim_end_matches('/')
            );
            Ok(Some(Action::NavigateTo { path: new_path }))
        } else {
            // Already previewed, Enter focuses on it
            Ok(Some(Action::FocusPane {
                pane: FocusedPane::Content,
            }))
        }
    }

    /// Handle filter mode input
    fn handle_filter_input(&mut self, key: KeyEvent) -> Result<Option<Action>> {
        match key.code {
            KeyCode::Char('/') => {
                // '/' again exits filter mode but keeps filter active
                self.filter_mode = false;
                Ok(Some(Action::ConfirmFilter))
            }
            KeyCode::Char(c) => {
                self.filter_input.push(c);
                self.selected_index = 0;
                Ok(Some(Action::UpdateFilter {
                    text: self.filter_input.clone(),
                }))
            }
            KeyCode::Backspace => {
                self.filter_input.pop();
                self.selected_index = 0;
                Ok(Some(Action::UpdateFilter {
                    text: self.filter_input.clone(),
                }))
            }
            KeyCode::Enter => {
                // Confirm filter - exit filter mode but keep filter active
                self.filter_mode = false;
                Ok(Some(Action::ConfirmFilter))
            }
            KeyCode::Esc => {
                // Cancel filter - clear filter text and exit filter mode
                self.filter_input.clear();
                self.filter_mode = false;
                self.selected_index = 0;
                Ok(Some(Action::CancelFilter))
            }
            _ => Ok(None),
        }
    }

    /// Handle delete action
    fn handle_delete(&self, ctx: &ComponentContext) -> Result<Option<Action>> {
        let Some(item) = self.selected_item() else {
            return Ok(None);
        };

        if item == ".." || item.ends_with('/') {
            // Can't delete ".." or directories
            Ok(Some(Action::SetStatus {
                message: "Cannot delete directories".to_string(),
            }))
        } else {
            let current_path = ctx.app.state().current_path();
            let path = format!("{}/{}", current_path.trim_end_matches('/'), item);
            Ok(Some(Action::ShowDeleteConfirmation { item_name: path }))
        }
    }

    /// Handle copy path action
    fn handle_copy_path(&self, ctx: &ComponentContext) -> Result<Option<Action>> {
        let Some(item) = self.selected_item() else {
            return Ok(None);
        };

        let current_path = ctx.app.state().current_path();
        let path = format!(
            "{}/{}",
            current_path.trim_end_matches('/'),
            item.trim_end_matches('/')
        );

        Ok(Some(Action::CopyToClipboard { text: path }))
    }

    /// Create block with title and optional filter footer
    fn create_block(&self, border_color: Color) -> Block<'_> {
        let title = if !self.filter_input.is_empty() {
            format!("Navigation ({} filtered)", self.items.len())
        } else {
            format!("Navigation ({} items)", self.items.len())
        };

        let mut block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title(title);

        // Add filter footer if in filter mode or filter is active
        if self.filter_mode || !self.filter_input.is_empty() {
            let filter_footer = if self.filter_mode {
                // Actively editing filter
                let filter_text = if self.filter_input.is_empty() {
                    "_"
                } else {
                    &self.filter_input
                };
                Line::from(vec![
                    Span::styled("Filter: ", Style::default().fg(Color::Cyan)),
                    Span::styled(filter_text, Style::default().fg(Color::Yellow)),
                    Span::styled(
                        " (esc to clear, / to exit)",
                        Style::default().fg(Color::DarkGray),
                    ),
                ])
            } else {
                // Filter active but not editing
                Line::from(vec![
                    Span::styled("Filter: ", Style::default().fg(Color::Cyan)),
                    Span::styled(&self.filter_input, Style::default().fg(Color::White)),
                    Span::styled(
                        " (/ to edit, esc to clear)",
                        Style::default().fg(Color::DarkGray),
                    ),
                ])
            };
            block = block.title_bottom(filter_footer);
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
        // Load initial items from app state
        self.update_items_from_app(ctx.app);
        Ok(())
    }

    fn handle_key(&mut self, key: KeyEvent, ctx: &ComponentContext) -> Result<Option<Action>> {
        // Filter mode captures input first
        if self.filter_mode {
            return self.handle_filter_input(key);
        }

        match (key.code, key.modifiers) {
            // Navigate up/down
            (KeyCode::Up, KeyModifiers::NONE) | (KeyCode::Char('k'), KeyModifiers::NONE) => {
                let action = self.move_up();
                // Auto-preview if selected item is a secret
                if self.is_selected_secret()
                    && let Some(item) = self.selected_item()
                {
                    let current_path = ctx.app.state().current_path();
                    let path = format!("{}/{}", current_path.trim_end_matches('/'), item);
                    return Ok(Some(Action::ViewSecret {
                        path,
                        should_focus: false,
                    }));
                }
                Ok(action)
            }
            (KeyCode::Down, KeyModifiers::NONE) | (KeyCode::Char('j'), KeyModifiers::NONE) => {
                let action = self.move_down();
                // Auto-preview if selected item is a secret
                if self.is_selected_secret()
                    && let Some(item) = self.selected_item()
                {
                    let current_path = ctx.app.state().current_path();
                    let path = format!("{}/{}", current_path.trim_end_matches('/'), item);
                    return Ok(Some(Action::ViewSecret {
                        path,
                        should_focus: false,
                    }));
                }
                Ok(action)
            }

            // Select/Enter - navigate into directories or parent
            (KeyCode::Enter, KeyModifiers::NONE) => self.handle_select_confirmation(ctx),

            // Go to parent
            (KeyCode::Backspace, KeyModifiers::NONE) => Ok(Some(Action::NavigateUp)),

            // History navigation
            (KeyCode::Char('b'), KeyModifiers::NONE) => Ok(Some(Action::HistoryBack)),
            (KeyCode::Char('f'), KeyModifiers::NONE) => Ok(Some(Action::HistoryForward)),

            // Create new secret (triggers input mode for name)
            (KeyCode::Char('n'), KeyModifiers::NONE) => Ok(Some(Action::ShowCreateSecretInput)),

            // Delete
            (KeyCode::Char('d'), KeyModifiers::NONE) => self.handle_delete(ctx),

            // Copy path
            (KeyCode::Char('y'), KeyModifiers::NONE) => self.handle_copy_path(ctx),

            // Start filter mode
            (KeyCode::Char('/'), KeyModifiers::NONE) => {
                self.filter_input.clear();
                self.filter_mode = true;
                self.selected_index = 0;
                Ok(Some(Action::StartFilter))
            }

            // Escape clears active filter, then detail view
            (KeyCode::Esc, KeyModifiers::NONE) => {
                if !self.filter_input.is_empty() {
                    self.filter_input.clear();
                    self.selected_index = 0;
                    Ok(Some(Action::CancelFilter))
                } else {
                    // Clear detail/edit views - handled by parent
                    Ok(None)
                }
            }

            _ => Ok(None),
        }
    }

    fn update(&mut self, action: Action, ctx: &mut ComponentContext) -> Result<Option<Action>> {
        match action {
            Action::Refresh
            | Action::StateUpdated
            | Action::NavigateTo { .. }
            | Action::NavigateUp
            | Action::HistoryBack
            | Action::HistoryForward => {
                // Reload items from app state
                self.update_items_from_app(ctx.app);
                self.selected_index = 0;
                self.scroll_offset = 0;
            }

            Action::UpdateFilter { ref text } => {
                self.filter_input = text.clone();
                self.update_items_from_app(ctx.app);
            }

            Action::CancelFilter => {
                self.filter_input.clear();
                self.filter_mode = false;
                self.update_items_from_app(ctx.app);
            }

            Action::SelectNext => {
                self.move_down();
            }

            Action::SelectPrevious => {
                self.move_up();
            }

            _ => {}
        }

        Ok(None)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect, ctx: &RenderContext) {
        let is_focused = ctx.is_focused;

        let border_color = if is_focused {
            Color::Cyan
        } else {
            Color::DarkGray
        };

        if self.is_loading {
            let loading = Paragraph::new("Loading...")
                .style(Style::default().fg(Color::Yellow))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(border_color))
                        .title("Navigation"),
                );
            frame.render_widget(loading, area);
            return;
        }

        if self.items.is_empty() {
            let block = self.create_block(border_color);
            let empty = Paragraph::new("No secrets at this path")
                .style(Style::default().fg(Color::Yellow))
                .block(block);
            frame.render_widget(empty, area);
            return;
        }

        // Calculate viewport height (subtract 2 for borders)
        let viewport_height = area.height.saturating_sub(2) as usize;

        // Adjust scroll to keep selected item visible
        self.adjust_scroll(viewport_height);

        // Calculate visible range
        let end_index = (self.scroll_offset + viewport_height).min(self.items.len());
        let visible_items = &self.items[self.scroll_offset..end_index];

        // Render visible items
        let list_items: Vec<ListItem> = visible_items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let actual_index = self.scroll_offset + i;
                let is_selected = actual_index == self.selected_index;
                let style = if is_selected {
                    Style::default()
                        .fg(Color::Black)
                        .bg(if is_focused {
                            Color::Cyan
                        } else {
                            Color::DarkGray
                        })
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };

                let icon = " ";

                ListItem::new(Line::from(vec![Span::raw(icon), Span::styled(item, style)]))
            })
            .collect();

        let block = self.create_block(border_color);
        let list = List::new(list_items).block(block);

        frame.render_widget(list, area);
    }

    fn captures_input(&self) -> bool {
        // Capture input when in filter mode
        self.filter_mode
    }
}
