use crate::vault::SecretData;

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
    /// Tick event for periodic updates (e.g., log refresh)
    Tick,

    /// Request a render cycle
    Render,

    /// Terminal resized
    Resize { width: u16, height: u16 },

    /// Quit the application
    Quit,

    /// Suspend terminal (Ctrl+Z support)
    Suspend,

    /// Resume from suspension
    Resume,

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

    /// Move selection up in a list
    SelectPrevious,

    /// Move selection down in a list
    SelectNext,

    /// Confirm/activate current selection
    SelectConfirm,

    // ═══════════════════════════════════════════════════════════════
    // FOCUS ACTIONS
    // ═══════════════════════════════════════════════════════════════
    /// Switch focus to next pane
    FocusNext,

    /// Switch focus to previous pane
    FocusPrevious,

    /// Focus specific pane
    FocusPane { pane: FocusedPane },

    // ═══════════════════════════════════════════════════════════════
    // SCREEN TRANSITION ACTIONS
    // ═══════════════════════════════════════════════════════════════
    /// Switch to main screen
    ShowMainScreen,

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

    /// Hide confirmation dialog
    HideConfirmation,

    /// User confirmed dialog action
    ConfirmationAccepted,

    /// User rejected dialog action
    ConfirmationRejected,

    /// Show create secret input
    ShowCreateSecretInput,

    /// Hide create secret input
    HideCreateSecretInput,

    // ═══════════════════════════════════════════════════════════════
    // SECRET ACTIONS
    // ═══════════════════════════════════════════════════════════════
    /// Load and view a secret (triggers async operation)
    ViewSecret { path: String, should_focus: bool },

    /// Secret loaded successfully (async completion)
    SecretLoaded {
        path: String,
        data: SecretData,
        metadata: Option<SecretMetadata>,
    },

    /// Start editing current secret
    EditSecret,

    /// Create new secret at current path
    CreateSecret { name: String },

    /// Save current secret being edited
    SaveSecret { path: String, data: SecretData },

    /// Secret saved successfully
    SecretSaved { path: String },

    /// Delete a secret
    DeleteSecret { path: String },

    /// Secret deleted successfully
    SecretDeleted { path: String },

    /// View specific version of secret
    ViewSecretVersion { path: String, version: u64 },

    /// Secret version loaded successfully (async completion)
    SecretVersionLoaded {
        path: String,
        version: u64,
        data: SecretData,
    },

    /// Secret version loaded for editing (create new from old)
    VersionLoadedForEdit { path: String, data: SecretData },

    /// Create new version from historical version
    CreateFromVersion { path: String, version: u64 },

    // ═══════════════════════════════════════════════════════════════
    // DETAIL VIEW ACTIONS
    // ═══════════════════════════════════════════════════════════════
    /// Toggle value visibility in detail view
    ToggleValueVisibility,

    /// Switch between data and version modes
    ToggleDetailMode,

    /// Copy selected value to clipboard
    CopySelectedValue,

    // ═══════════════════════════════════════════════════════════════
    // EDIT VIEW ACTIONS
    // ═══════════════════════════════════════════════════════════════
    /// Start editing field value
    StartEditValue,

    /// Start editing field key
    StartEditKey,

    /// Confirm current field edit
    ConfirmEdit,

    /// Cancel current edit
    CancelEdit,

    /// Toggle between key/value editing
    ToggleEditTarget,

    /// Add new field
    AddField,

    /// Delete selected field
    DeleteField,

    // ═══════════════════════════════════════════════════════════════
    // FILTER ACTIONS
    // ═══════════════════════════════════════════════════════════════
    /// Start filter mode
    StartFilter,

    /// Update filter text
    UpdateFilter { text: String },

    /// Confirm filter and exit filter mode
    ConfirmFilter,

    /// Cancel filter (clear and exit mode)
    CancelFilter,

    /// Clear filter while keeping mode
    ClearFilter,

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

    /// Copy policy content
    CopyPolicy,

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

    /// Copy operation completed
    CopyComplete { success: bool, message: String },

    // ═══════════════════════════════════════════════════════════════
    // STATUS/ERROR ACTIONS
    // ═══════════════════════════════════════════════════════════════
    /// Set status message
    SetStatus { message: String },

    /// Clear status message
    ClearStatus,

    /// Set error message
    SetError { message: String },

    /// Clear error message
    ClearError,

    /// Set loading state
    SetLoading { loading: bool },

    // ═══════════════════════════════════════════════════════════════
    // INPUT FORWARDING (for TextArea)
    // ═══════════════════════════════════════════════════════════════
    /// Forward key to TextArea component
    TextInput { input: ratatui_textarea::Input },

    // ═══════════════════════════════════════════════════════════════
    // COMPOSITE/BATCH ACTIONS
    // ═══════════════════════════════════════════════════════════════
    /// Execute multiple actions in sequence
    Batch { actions: Vec<Action> },

    /// No operation (useful for event handling that needs no action)
    Noop,
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

    #[test]
    fn test_action_noop() {
        let action = Action::Noop;
        matches!(action, Action::Noop);
    }
}
