/// Component implementations for VltView
///
/// This module contains all the UI components that implement the Component trait.
/// Components are organized into:
/// - Overlays: Modal overlays like Help, Confirmation dialogs
/// - Screens: Full-screen views like Log, Policy, Main
/// - Common: Reusable components used across multiple screens
pub mod detail_view;
pub mod edit_view;
pub mod help_overlay;
pub mod log_screen;
pub mod main_screen;
pub mod navigation_pane;
pub mod policy_screen;

pub use detail_view::{DetailViewComponent, VersionInfo};
pub use edit_view::EditViewComponent;
pub use help_overlay::HelpOverlay;
pub use log_screen::LogScreen;
pub use main_screen::MainScreen;
pub use navigation_pane::NavigationPane;
pub use policy_screen::PolicyScreen;
