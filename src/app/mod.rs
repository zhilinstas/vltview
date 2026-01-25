use anyhow::Result;

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

    /// Configuration
    #[allow(dead_code)]
    config: Config,

    /// Navigation controller
    navigation: NavigationController,

    /// Secret service
    secret_service: SecretService,

    /// Policy service
    policy_service: PolicyService,

    /// Filter engine
    #[allow(dead_code)]
    filter_engine: FilterEngine,
}

impl App {
    /// Create a new application instance
    pub fn new(config: Config, initial_path: String) -> Result<Self> {
        // Initialize state
        let state = Arc::new(AppState::new(initial_path));

        // Initialize Vault client
        let vault_client = Arc::new(VaultClient::new(config.clone())?);

        // Initialize services
        let navigation = NavigationController::new(Arc::clone(&state), Arc::clone(&vault_client));
        let secret_service = SecretService::new(Arc::clone(&vault_client), Arc::clone(&state));
        let policy_service = PolicyService::new(Arc::clone(&vault_client));
        let filter_engine = FilterEngine::new();

        Ok(Self {
            vault_client,
            state,
            config,
            navigation,
            secret_service,
            policy_service,
            filter_engine,
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

        // Load initial path
        self.navigation.refresh().await?;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_creation() {
        // This would need proper test setup with mock config
        // Placeholder for now
        assert!(true);
    }
}
