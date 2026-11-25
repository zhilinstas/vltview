use anyhow::{Context, Result};

pub mod filter;
pub mod navigation;
pub mod policy;
pub mod secret;

pub use filter::FilterEngine;
pub use navigation::NavigationController;
pub use policy::PolicyService;
pub use secret::SecretService;

use crate::config::Config;
use crate::state::AppState;
use crate::vault::VaultClient;

use std::sync::Arc;

/// Application core that orchestrates all business logic
pub struct App {
    /// Vault client for API operations
    vault_client: Arc<VaultClient>,

    /// Application state
    state: Arc<AppState>,

    /// Navigation controller
    navigation: NavigationController,

    /// Secret service
    secret_service: SecretService,

    /// Policy service
    policy_service: PolicyService,
}

impl App {
    /// Create a new application instance
    pub fn new(config: Config, initial_path: String) -> Result<Self> {
        // Initialize state
        let state = Arc::new(AppState::new(initial_path));

        // Initialize Vault client
        let vault_client = Arc::new(VaultClient::new(config)?);

        // Initialize services
        let navigation = NavigationController::new(Arc::clone(&vault_client));
        let secret_service = SecretService::new(Arc::clone(&vault_client), Arc::clone(&state));
        let policy_service = PolicyService::new(Arc::clone(&vault_client));

        Ok(Self {
            vault_client,
            state,
            navigation,
            secret_service,
            policy_service,
        })
    }

    /// Get reference to navigation controller
    pub fn navigation(&self) -> &NavigationController {
        &self.navigation
    }

    /// Get reference to secret service
    pub fn secret_service(&self) -> &SecretService {
        &self.secret_service
    }

    /// Get reference to policy service
    pub fn policy_service(&self) -> &PolicyService {
        &self.policy_service
    }

    /// Get reference to application state
    pub fn state(&self) -> &Arc<AppState> {
        &self.state
    }

    /// Get reference to vault client
    pub fn vault_client(&self) -> &Arc<VaultClient> {
        &self.vault_client
    }

    /// Initialize the application (load initial data)
    pub async fn initialize(&mut self) -> Result<()> {
        tracing::info!("Initializing application");

        // Load initial path. A failure here is fatal, so say which path
        // could not be listed (a token without list access is a common cause).
        let path = self.state.current_path();
        let items = self
            .navigation
            .list(&path)
            .await
            .with_context(|| format!("Failed to list initial path '{path}'"))?;
        self.state.reload_folder(&path, items);

        Ok(())
    }

    /// Shutdown the application gracefully
    pub async fn shutdown(&mut self) -> Result<()> {
        tracing::info!("Shutting down application");

        // Clear cache if needed
        self.state.clear_cache();

        Ok(())
    }
}
