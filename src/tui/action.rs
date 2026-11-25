use crate::vault::{SecretData, WriteMode};

/// All possible actions in the VltView application
///
/// Actions represent state changes or commands that flow through the system.
/// Components emit actions in response to events, and the Root component
/// processes them to update application state.
#[derive(Debug, Clone)]
pub enum Action {
    // ═══════════════════════════════════════════════════════════════
    // LIFECYCLE ACTIONS
    // ═══════════════════════════════════════════════════════════════
    /// `q`: quit, after asking if an edit with unsaved changes is open
    Quit,

    /// Quit at once (Ctrl+C, or `q` once it is confirmed)
    QuitNow,

    // ═══════════════════════════════════════════════════════════════
    // NAVIGATION ACTIONS
    // ═══════════════════════════════════════════════════════════════
    /// Navigate to a path in Vault
    NavigateTo { path: String },

    /// Navigate to parent directory
    NavigateUp,

    /// Go back in history
    HistoryBack,

    /// Go forward in history
    HistoryForward,

    /// Refresh current path (reload from Vault)
    Refresh,

    /// Update UI from current state (after async operations complete)
    StateUpdated,

    /// A folder was listed (async completion). `request` is the number
    /// `Root` gave the request, so it can tell whether this listing is
    /// still the one that is wanted.
    FolderListed {
        request: u64,
        path: String,
        items: Vec<String>,
    },

    /// A folder could not be listed (async completion)
    FolderListFailed { request: u64, message: String },

    /// The navigation cursor now rests on a different item.
    ///
    /// `secret` is the full path of the secret under the cursor, or `None`
    /// when the cursor is on a folder, on `..`, or the list is empty. The
    /// content pane always shows this selection.
    SelectionChanged {
        secret: Option<String>,
        source: SelectionSource,
    },

    // ═══════════════════════════════════════════════════════════════
    // FOCUS ACTIONS
    // ═══════════════════════════════════════════════════════════════
    /// Focus specific pane
    FocusPane { pane: FocusedPane },

    // ═══════════════════════════════════════════════════════════════
    // SCREEN TRANSITION ACTIONS
    // ═══════════════════════════════════════════════════════════════
    /// Switch to policy screen
    ShowPolicyScreen,

    /// Switch to logs screen
    ShowLogScreen,

    /// Go back to previous screen
    ScreenBack,

    // ═══════════════════════════════════════════════════════════════
    // OVERLAY ACTIONS
    // ═══════════════════════════════════════════════════════════════
    /// Show help overlay
    ShowHelp,

    /// Hide help overlay
    HideHelp,

    /// Toggle help overlay
    ToggleHelp,

    /// Show delete confirmation dialog
    ShowDeleteConfirmation { item_name: String },

    /// Show create secret input
    ShowCreateSecretInput,

    // ═══════════════════════════════════════════════════════════════
    // SECRET ACTIONS
    // ═══════════════════════════════════════════════════════════════
    /// A secret was written: read it again, if the cursor is still on it.
    /// If the cursor has left, the content pane keeps following the cursor.
    ReloadSecret { path: String },

    /// Go into the secret under the cursor: focus the content pane, loading
    /// the secret first if it is not on screen yet
    OpenSecret { path: String },

    /// A secret could not be loaded (async completion)
    SecretLoadFailed {
        /// Number `Root` gave the load, as for `SecretLoaded`
        request: u64,
        path: String,
        message: String,
    },

    /// Secret loaded successfully (async completion)
    SecretLoaded {
        /// Number `Root` gave the load. Two loads of one path can be on
        /// their way at once (the preview and the read after a save); only
        /// the later one may be shown, whichever arrives last.
        request: u64,
        path: String,
        data: SecretData,
        metadata: Option<SecretMetadata>,
    },

    /// Start editing current secret
    EditSecret,

    /// Create new secret at current path
    CreateSecret { name: String },

    /// Save current secret being edited
    SaveSecret {
        path: String,
        data: SecretData,
        /// Whether this creates the secret or adds a version to it
        mode: WriteMode,
    },

    /// Secret saved successfully
    SecretSaved { path: String },

    /// A secret could not be saved (async completion)
    SaveFailed { path: String, message: String },

    /// Delete a secret
    DeleteSecret { path: String },

    /// Secret deleted successfully
    SecretDeleted { path: String },

    /// A secret could not be deleted (async completion). `message` is the
    /// reason; the screen says which secret.
    DeleteFailed { path: String, message: String },

    /// View specific version of secret
    ViewSecretVersion { path: String, version: u64 },

    /// Secret version loaded successfully (async completion)
    SecretVersionLoaded {
        path: String,
        version: u64,
        data: SecretData,
    },

    /// Secret version loaded for editing (create new from old)
    VersionLoadedForEdit {
        path: String,
        version: u64,
        data: SecretData,
    },

    /// Create new version from historical version
    CreateFromVersion { path: String, version: u64 },

    /// A version of a secret could not be loaded (async completion)
    VersionLoadFailed {
        path: String,
        version: u64,
        /// What it had been asked for
        wanted_for: VersionUse,
        message: String,
    },

    // ═══════════════════════════════════════════════════════════════
    // EDIT VIEW ACTIONS
    // ═══════════════════════════════════════════════════════════════
    /// Esc in the edit view's field list: close the editor, after asking
    /// if it has unsaved changes
    CancelEdit,

    // ═══════════════════════════════════════════════════════════════
    // POLICY ACTIONS
    // ═══════════════════════════════════════════════════════════════
    /// Load policies (triggers async)
    LoadPolicies,

    /// Policies loaded (async completion)
    PoliciesLoaded { policies: Vec<String> },

    /// View selected policy content
    ViewPolicy { name: String },

    /// Policy content loaded
    PolicyLoaded { name: String, content: String },

    /// The policy list could not be loaded (async completion)
    PoliciesLoadFailed { message: String },

    /// A policy could not be loaded (async completion)
    PolicyLoadFailed { name: String, message: String },

    /// Scroll policy content
    ScrollPolicy { direction: ScrollDirection },

    // ═══════════════════════════════════════════════════════════════
    // LOG ACTIONS
    // ═══════════════════════════════════════════════════════════════
    /// Scroll logs
    ScrollLogs { direction: ScrollDirection },

    /// Toggle auto-scroll
    ToggleAutoScroll,

    /// Clear logs
    ClearLogs,

    // ═══════════════════════════════════════════════════════════════
    // CLIPBOARD ACTIONS
    // ═══════════════════════════════════════════════════════════════
    /// Copy text to clipboard
    CopyToClipboard { text: String },

    /// Nothing was copied to the clipboard
    CopyFailed { message: String },

    // ═══════════════════════════════════════════════════════════════
    // STATUS/ERROR ACTIONS
    // ═══════════════════════════════════════════════════════════════
    /// Set status message
    SetStatus { message: String },

    /// Set loading state
    SetLoading { loading: bool },

    // ═══════════════════════════════════════════════════════════════
    // COMPOSITE/BATCH ACTIONS
    // ═══════════════════════════════════════════════════════════════
    /// Execute multiple actions in sequence
    Batch { actions: Vec<Action> },
}

/// Scroll direction for list and content views
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollDirection {
    /// Scroll up one line
    Up,
    /// Scroll down one line
    Down,
    /// Scroll up one page
    PageUp,
    /// Scroll down one page
    PageDown,
    /// Scroll to top
    Top,
    /// Scroll to bottom
    Bottom,
}

/// What a version of a secret was asked for
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionUse {
    /// To be shown in the secret pane
    View,
    /// To be opened in the editor, as a copy to restore
    Edit,
}

/// What moved the navigation cursor
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionSource {
    /// A cursor key. The first step after a pause loads at once; further
    /// steps in quick succession wait until the cursor rests.
    Cursor,
    /// The list changed under the cursor (reload, filter, navigation). The
    /// load always waits until the cursor rests.
    ListChange,
}

/// Pane focus identifiers
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FocusedPane {
    /// Navigation pane (left side)
    Navigation,
    /// Content pane (right side)
    Content,
}

/// Secret metadata from Vault
#[derive(Debug, Clone)]
pub struct SecretMetadata {
    /// When the secret was created
    pub created_time: String,
    /// When the secret was last updated
    pub updated_time: String,
    /// Current version number
    pub current_version: u64,
    /// List of all versions
    pub versions: Vec<VersionInfo>,
}

/// Information about a specific secret version
#[derive(Debug, Clone)]
pub struct VersionInfo {
    /// Version number
    pub version: u64,
    /// Creation timestamp
    pub created_time: String,
    /// Whether this version is deleted
    pub deleted: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scroll_direction() {
        let dir = ScrollDirection::Up;
        assert_eq!(dir, ScrollDirection::Up);
        assert_ne!(dir, ScrollDirection::Down);
    }

    #[test]
    fn test_focused_pane() {
        let pane = FocusedPane::Navigation;
        assert_eq!(pane, FocusedPane::Navigation);
        assert_ne!(pane, FocusedPane::Content);
    }
}
