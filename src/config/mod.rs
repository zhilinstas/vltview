use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// Application configuration
#[derive(Clone, Serialize, Deserialize)]
pub struct Config {
    /// Vault server address
    pub vault_addr: String,

    /// Vault authentication token
    pub vault_token: String,

    /// Vault namespace (Enterprise feature)
    #[serde(default)]
    pub vault_namespace: Option<String>,

    /// Skip TLS certificate verification (insecure)
    #[serde(default)]
    pub insecure: bool,

    /// Request timeout in seconds
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,

    /// Enable secret caching
    #[serde(default = "default_cache_enabled")]
    pub cache_enabled: bool,

    /// Cache TTL in seconds
    #[serde(default = "default_cache_ttl")]
    pub cache_ttl: u64,
}

fn default_timeout() -> u64 {
    30
}

fn default_cache_enabled() -> bool {
    true
}

fn default_cache_ttl() -> u64 {
    300 // 5 minutes
}

impl Config {
    /// Load configuration from multiple sources with priority
    /// Priority: CLI args > Environment > Config file > Token cache
    pub fn load(
        config_path: Option<&Path>,
        vault_addr: Option<String>,
        vault_token: Option<String>,
        vault_namespace: Option<String>,
        insecure: bool,
    ) -> Result<Self> {
        // Start with defaults or config file
        let mut config = if let Some(path) = config_path {
            Self::load_from_file(path)?
        } else {
            Self::load_default_file().unwrap_or_else(|_| Self::default_config())
        };

        // Override with environment variables
        if let Ok(addr) = std::env::var("VAULT_ADDR") {
            config.vault_addr = addr;
        }

        if let Ok(token) = std::env::var("VAULT_TOKEN") {
            config.vault_token = token;
        } else if config.vault_token.is_empty() {
            // Try to load from token cache if not set
            if let Ok(cached_token) = Self::load_token_cache() {
                config.vault_token = cached_token;
            }
        }

        if let Ok(ns) = std::env::var("VAULT_NAMESPACE") {
            config.vault_namespace = Some(ns);
        }

        if let Ok(skip_verify) = std::env::var("VAULT_SKIP_VERIFY") {
            if skip_verify == "1" || skip_verify.to_lowercase() == "true" {
                config.insecure = true;
            }
        }

        // Override with CLI arguments (highest priority)
        if let Some(addr) = vault_addr {
            config.vault_addr = addr;
        }

        if let Some(token) = vault_token {
            config.vault_token = token;
        }

        if let Some(ns) = vault_namespace {
            config.vault_namespace = Some(ns);
        }

        if insecure {
            config.insecure = true;
        }

        Ok(config)
    }

    /// Load configuration from YAML file
    fn load_from_file(path: &Path) -> Result<Self> {
        let contents = fs::read_to_string(path)
            .context(format!("Failed to read config file: {}", path.display()))?;

        let config: Self =
            serde_yaml::from_str(&contents).context("Failed to parse config file")?;

        Ok(config)
    }

    /// Try to load configuration from default location (~/.vltview.yaml)
    fn load_default_file() -> Result<Self> {
        let home = dirs::home_dir().ok_or_else(|| anyhow!("Could not determine home directory"))?;

        let config_path = home.join(".vltview.yaml");

        if config_path.exists() {
            Self::load_from_file(&config_path)
        } else {
            Err(anyhow!("Default config file not found"))
        }
    }

    /// Load Vault token from cache (~/.vault-token)
    fn load_token_cache() -> Result<String> {
        let home = dirs::home_dir().ok_or_else(|| anyhow!("Could not determine home directory"))?;

        let token_path = home.join(".vault-token");

        if token_path.exists() {
            let token = fs::read_to_string(&token_path)
                .context("Failed to read vault token cache")?
                .trim()
                .to_string();

            if !token.is_empty() {
                tracing::debug!("Loaded token from cache: ~/.vault-token");
                Ok(token)
            } else {
                Err(anyhow!("Token cache is empty"))
            }
        } else {
            Err(anyhow!("Token cache not found"))
        }
    }

    /// Create default configuration (requires env vars or will fail validation)
    fn default_config() -> Self {
        Self {
            vault_addr: String::new(),
            vault_token: String::new(),
            vault_namespace: None,
            insecure: false,
            timeout_secs: default_timeout(),
            cache_enabled: default_cache_enabled(),
            cache_ttl: default_cache_ttl(),
        }
    }

    /// Validate that required configuration is present
    pub fn validate(&self) -> Result<()> {
        if self.vault_addr.is_empty() {
            return Err(anyhow!(
                "Vault address is required. Set VAULT_ADDR environment variable or use --vault-addr flag"
            ));
        }

        if self.vault_token.is_empty() {
            return Err(anyhow!(
                "Vault token is required. Set VAULT_TOKEN environment variable, use --vault-token flag, or ensure ~/.vault-token exists"
            ));
        }

        // Validate URL format
        url::Url::parse(&self.vault_addr).context("Invalid Vault address URL")?;

        if self.insecure {
            tracing::warn!("TLS certificate verification is disabled (insecure mode)");
        }

        Ok(())
    }

    /// Get the base URL for Vault API
    pub fn base_url(&self) -> String {
        self.vault_addr.trim_end_matches('/').to_string()
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::default_config()
    }
}

// Custom Debug implementation to avoid leaking tokens in logs
impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("vault_addr", &self.vault_addr)
            .field("vault_token", &"***REDACTED***")
            .field("vault_namespace", &self.vault_namespace)
            .field("insecure", &self.insecure)
            .field("timeout_secs", &self.timeout_secs)
            .field("cache_enabled", &self.cache_enabled)
            .field("cache_ttl", &self.cache_ttl)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.timeout_secs, 30);
        assert_eq!(config.cache_enabled, true);
        assert_eq!(config.cache_ttl, 300);
    }

    #[test]
    fn test_validate_missing_addr() {
        let config = Config::default();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_missing_token() {
        let mut config = Config::default();
        config.vault_addr = "https://vault.example.com".to_string();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_success() {
        let mut config = Config::default();
        config.vault_addr = "https://vault.example.com".to_string();
        config.vault_token = "test-token".to_string();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_base_url() {
        let mut config = Config::default();
        config.vault_addr = "https://vault.example.com/".to_string();
        assert_eq!(config.base_url(), "https://vault.example.com");
    }
}
