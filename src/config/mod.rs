use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

use crate::vault::Token;

/// Application configuration
///
/// Also the shape of the config file. Every key is optional there: what the
/// file leaves out comes from the environment, the command line or the
/// defaults.
///
/// Not `Clone`: the token in it has one owner. `VaultClient::new` takes the
/// configuration and moves the token out, so no configuration with a token
/// in it exists once there is a client.
#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Vault server address
    pub vault_addr: String,

    /// Vault authentication token (never shown by `Debug`, wiped on drop)
    pub vault_token: Token,

    /// Vault namespace (Enterprise feature)
    pub vault_namespace: Option<String>,

    /// Skip TLS certificate verification (insecure)
    pub insecure: bool,

    /// Request timeout in seconds
    pub timeout_secs: u64,

    /// Enable secret caching
    pub cache_enabled: bool,

    /// Cache TTL in seconds
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

/// What the process environment contributes to the configuration.
///
/// Read once in `main` and passed to `Config::load`, so the loader never
/// looks at the real environment or the real home directory itself.
#[derive(Default)]
pub struct Environment {
    /// `VAULT_ADDR`
    pub vault_addr: Option<String>,

    /// `VAULT_TOKEN`
    pub vault_token: Option<Token>,

    /// `VAULT_NAMESPACE`
    pub vault_namespace: Option<String>,

    /// `VAULT_SKIP_VERIFY`
    pub vault_skip_verify: Option<String>,

    /// Home directory, where `.vltview.yaml` and `.vault-token` are looked for
    pub home: Option<PathBuf>,
}

impl Environment {
    /// Read the variables of this process
    pub fn from_process() -> Self {
        Self {
            vault_addr: std::env::var("VAULT_ADDR").ok(),
            vault_token: std::env::var("VAULT_TOKEN").ok().map(Token::from),
            vault_namespace: std::env::var("VAULT_NAMESPACE").ok(),
            vault_skip_verify: std::env::var("VAULT_SKIP_VERIFY").ok(),
            home: std::env::home_dir(),
        }
    }
}

impl Config {
    /// Load configuration from multiple sources with priority
    /// Priority: CLI args > Environment > Config file > Token cache
    pub fn load(
        config_path: Option<&Path>,
        env: Environment,
        vault_addr: Option<String>,
        vault_token: Option<Token>,
        vault_namespace: Option<String>,
        insecure: bool,
    ) -> Result<Self> {
        // Start with the config file. One named with `--config` must exist;
        // the default one is optional, but if it is there it has to parse
        let default_path = env.home.as_deref().map(|home| home.join(".vltview.yaml"));
        let mut config = match (config_path, default_path) {
            (Some(path), _) => Self::load_from_file(path)?,
            (None, Some(path)) if path.exists() => Self::load_from_file(&path)?,
            _ => Self::default_config(),
        };

        // Override with environment variables
        if let Some(addr) = env.vault_addr {
            config.vault_addr = addr;
        }

        if let Some(token) = env.vault_token {
            config.vault_token = token;
        } else if config.vault_token.is_empty() {
            // Try to load from token cache if not set
            if let Ok(cached_token) = Self::load_token_cache(env.home.as_deref()) {
                config.vault_token = cached_token;
            }
        }

        if let Some(ns) = env.vault_namespace {
            config.vault_namespace = Some(ns);
        }

        if let Some(skip_verify) = env.vault_skip_verify
            && (skip_verify == "1" || skip_verify.to_lowercase() == "true")
        {
            config.insecure = true;
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
        // The file may hold the token, so the text read from it is wiped
        let contents = Zeroizing::new(
            fs::read_to_string(path)
                .with_context(|| format!("Failed to read config file {}", path.display()))?,
        );

        Self::parse(&contents)
            .with_context(|| format!("Failed to parse config file {}", path.display()))
    }

    /// Parse the contents of a config file
    fn parse(contents: &str) -> Result<Self> {
        serde_norway::from_str(contents).map_err(|error| {
            // The parser quotes a value it did not expect. For a file that
            // is one bare value, that value may well be a pasted token
            match serde_norway::from_str::<serde_norway::Value>(contents) {
                Ok(document) if !document.is_mapping() => {
                    anyhow!("expected `key: value` lines, such as `vault_addr: https://...`")
                }
                _ => error.into(),
            }
        })
    }

    /// Load Vault token from cache (~/.vault-token)
    fn load_token_cache(home: Option<&Path>) -> Result<Token> {
        let home = home.ok_or_else(|| anyhow!("Could not determine home directory"))?;

        let token_path = home.join(".vault-token");

        if token_path.exists() {
            let contents = Zeroizing::new(
                fs::read_to_string(&token_path).context("Failed to read vault token cache")?,
            );
            let token = Token::from(contents.trim().to_string());

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
            vault_token: Token::default(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A home directory of its own for one test, removed afterwards
    struct TestHome(PathBuf);

    impl TestHome {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let dir = std::env::temp_dir().join(format!(
                "vltview-config-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        /// A home directory whose `.vltview.yaml` has these contents
        fn with_config(contents: &str) -> Self {
            let home = Self::new();
            fs::write(home.config_path(), contents).unwrap();
            home
        }

        fn config_path(&self) -> PathBuf {
            self.0.join(".vltview.yaml")
        }

        /// The environment of a process with this home and no `VAULT_*` variables
        fn env(&self) -> Environment {
            Environment {
                home: Some(self.0.clone()),
                ..Environment::default()
            }
        }
    }

    impl Drop for TestHome {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Load with nothing on the command line
    fn load(env: Environment) -> Result<Config> {
        Config::load(None, env, None, None, None, false)
    }

    /// The whole error as the user sees it, causes included
    fn message(result: Result<Config>) -> String {
        format!("{:#}", result.expect_err("loading should fail"))
    }

    #[test]
    fn malformed_default_file_stops_the_start() {
        // Regression: a `~/.vltview.yaml` that did not parse was ignored
        let home = TestHome::with_config("vault_addr: http://localhost:8200\n  insecure: true\n");

        let message = message(load(home.env()));

        let path = home.config_path();
        assert!(
            message.contains(&path.display().to_string()),
            "the error should name the file: {message}"
        );
        assert!(
            message.contains("line 2"),
            "the error should say what is wrong: {message}"
        );
    }

    #[test]
    fn file_with_only_the_address_takes_the_token_from_the_environment() {
        // Regression: a file had to hold both, or it did not parse at all
        let home = TestHome::with_config("vault_addr: https://file.example\n");
        let env = Environment {
            vault_token: Some("env-token".to_string().into()),
            ..home.env()
        };

        let config = load(env).unwrap();

        assert_eq!(config.vault_addr, "https://file.example");
        assert_eq!(config.vault_token.expose(), "env-token");
        config.validate().unwrap();
    }

    #[test]
    fn file_with_only_the_token_takes_the_address_from_the_command_line() {
        let home = TestHome::with_config("vault_token: file-token\n");

        let config = Config::load(
            None,
            home.env(),
            Some("https://cli.example".to_string()),
            None,
            None,
            false,
        )
        .unwrap();

        assert_eq!(config.vault_addr, "https://cli.example");
        assert_eq!(config.vault_token.expose(), "file-token");
        config.validate().unwrap();
    }

    #[test]
    fn command_line_beats_environment_beats_file_beats_defaults() {
        let home = TestHome::with_config(
            "vault_addr: https://file.example\nvault_token: file-token\nvault_namespace: file-ns\n",
        );
        let env = || Environment {
            vault_addr: Some("https://env.example".to_string()),
            vault_token: Some("env-token".to_string().into()),
            ..home.env()
        };

        let config = Config::load(
            None,
            env(),
            Some("https://cli.example".to_string()),
            None,
            None,
            false,
        )
        .unwrap();

        // Each value comes from the strongest source that has it
        assert_eq!(config.vault_addr, "https://cli.example");
        assert_eq!(config.vault_token.expose(), "env-token");
        assert_eq!(config.vault_namespace.as_deref(), Some("file-ns"));
        assert_eq!(config.timeout_secs, 30);
        assert!(!config.insecure);

        // The command line also beats the environment for the token
        let config = Config::load(
            None,
            env(),
            None,
            Some("cli-token".to_string().into()),
            None,
            false,
        );
        assert_eq!(config.unwrap().vault_token.expose(), "cli-token");
    }

    #[test]
    fn insecure_can_be_set_by_file_environment_or_command_line() {
        let from_file = TestHome::with_config("insecure: true\n");
        assert!(load(from_file.env()).unwrap().insecure);

        let home = TestHome::new();
        let from_env = Environment {
            vault_skip_verify: Some("true".to_string()),
            ..home.env()
        };
        assert!(load(from_env).unwrap().insecure);

        let from_flag = Config::load(None, home.env(), None, None, None, true);
        assert!(from_flag.unwrap().insecure);

        assert!(!load(home.env()).unwrap().insecure);
    }

    #[test]
    fn missing_default_file_is_not_an_error() {
        let home = TestHome::new();

        let config = load(home.env()).unwrap();

        assert_eq!(config.vault_addr, "");
        assert_eq!(config.timeout_secs, 30);

        // Nor is having no home directory at all
        load(Environment::default()).unwrap();
    }

    #[test]
    fn token_cache_is_used_when_nothing_else_has_a_token() {
        let home = TestHome::with_config("vault_addr: https://file.example\n");
        fs::write(home.0.join(".vault-token"), "cached-token\n").unwrap();

        assert_eq!(
            load(home.env()).unwrap().vault_token.expose(),
            "cached-token"
        );

        let env = Environment {
            vault_token: Some("env-token".to_string().into()),
            ..home.env()
        };
        assert_eq!(load(env).unwrap().vault_token.expose(), "env-token");
    }

    #[test]
    fn missing_file_named_on_the_command_line_is_an_error() {
        let home = TestHome::new();
        let path = home.0.join("nowhere.yaml");

        let result = Config::load(Some(&path), home.env(), None, None, None, false);

        assert!(message(result).contains("nowhere.yaml"));
    }

    #[test]
    fn unknown_key_is_reported() {
        // Regression: the README documented `insecure_skip_verify`, which is
        // not a key. It was accepted and did nothing
        let home =
            TestHome::with_config("vault_addr: https://file.example\ninsecure_skip_verify: true\n");

        let message = message(load(home.env()));

        assert!(
            message.contains("insecure_skip_verify"),
            "the error should name the key: {message}"
        );
        assert!(
            message.contains(&home.config_path().display().to_string()),
            "the error should name the file: {message}"
        );
    }

    #[test]
    fn errors_about_the_file_never_show_the_token() {
        let files = [
            // Not YAML, after the token
            "vault_token: hvs.SECRET\n  vault_addr: https://file.example\n",
            // Unknown key next to the token
            "vault_token: hvs.SECRET\nvault_adr: https://file.example\n",
            // Another value of the wrong type
            "vault_token: hvs.SECRET\ntimeout_secs: soon\n",
            // The token itself of the wrong type
            "vault_token: [hvs.SECRET]\n",
            // Only the token, as in `~/.vault-token`
            "hvs.SECRET\n",
            "vault_token: {hvs.SECRET: hvs.SECRET}\n",
        ];

        for file in files {
            let home = TestHome::with_config(file);

            let message = message(load(home.env()));

            assert!(
                !message.contains("hvs.SECRET"),
                "{file:?} leaked the token: {message}"
            );
        }
    }

    #[test]
    fn file_without_any_key_is_accepted() {
        for file in ["", "# vault_addr: https://file.example\n", "---\n"] {
            let home = TestHome::with_config(file);

            let config = load(home.env()).unwrap();

            assert_eq!(config.vault_addr, "", "{file:?}");
        }
    }

    #[test]
    fn debug_output_never_shows_the_token() {
        let home = TestHome::with_config("vault_token: hvs.SECRET\n");

        let config = load(home.env()).unwrap();

        let debug = format!("{config:?} {config:#?}");
        assert!(!debug.contains("hvs.SECRET"), "{debug}");
        assert!(debug.contains("***REDACTED***"), "{debug}");
    }

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.timeout_secs, 30);
        assert!(config.cache_enabled);
        assert_eq!(config.cache_ttl, 300);
    }

    #[test]
    fn test_validate_missing_addr() {
        let config = Config::default();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_missing_token() {
        let config = Config {
            vault_addr: "https://vault.example.com".to_string(),
            ..Config::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_success() {
        let config = Config {
            vault_addr: "https://vault.example.com".to_string(),
            vault_token: "test-token".to_string().into(),
            ..Config::default()
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_base_url() {
        let config = Config {
            vault_addr: "https://vault.example.com/".to_string(),
            ..Config::default()
        };
        assert_eq!(config.base_url(), "https://vault.example.com");
    }
}
