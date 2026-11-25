use anyhow::Result;
use clap::parser::ValueSource;
use clap::{ArgMatches, CommandFactory, FromArgMatches, Parser};
use std::path::PathBuf;
use tracing::Level;
use tracing_subscriber::{Layer, layer::SubscriberExt, util::SubscriberInitExt};

mod app;
mod clipboard;
mod config;
mod state;
mod tui;
mod vault;

use config::{Config, Environment};
use tui::Tui;
use vault::Token;

/// VltView - Terminal UI for HashiCorp Vault
#[derive(Parser)]
#[command(name = "vltview")]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Path to configuration file
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,

    /// Vault address (overrides config and env)
    #[arg(long, env = "VAULT_ADDR")]
    vault_addr: Option<String>,

    /// Vault token (overrides config and env)
    /// ⚠️  WARNING: Using CLI args for tokens exposes them in process lists!
    /// ⚠️  Prefer VAULT_TOKEN environment variable or config file for security.
    #[arg(long, env = "VAULT_TOKEN", hide_env_values = true)]
    vault_token: Option<String>,

    /// Vault namespace (for Enterprise)
    #[arg(long, env = "VAULT_NAMESPACE")]
    vault_namespace: Option<String>,

    /// Initial path to navigate to
    #[arg(value_name = "PATH", default_value = "/")]
    initial_path: String,

    /// Enable debug logging
    #[arg(short, long)]
    debug: bool,

    /// Skip TLS verification (insecure)
    #[arg(long)]
    insecure: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Parse CLI arguments
    let mut matches = Cli::command().get_matches();
    let warn_about_token = token_on_command_line(&matches);
    let cli = Cli::from_arg_matches_mut(&mut matches).unwrap_or_else(|error| error.exit());

    // Warn if token is provided via CLI argument (security issue)
    if warn_about_token {
        eprintln!("⚠️  WARNING: Vault token provided via command-line argument!");
        eprintln!("⚠️  This exposes your token in process listings (ps, top, etc.)");
        eprintln!("⚠️  For better security, use:");
        eprintln!("⚠️    - VAULT_TOKEN environment variable");
        eprintln!("⚠️    - Config file (~/.vltview.yaml)");
        eprintln!("⚠️    - Token cache (~/.vault-token)\n");
    }

    // Initialize logging
    init_logging(cli.debug)?;

    tracing::info!("Starting VltView v{}", env!("CARGO_PKG_VERSION"));

    // Load configuration
    let config = Config::load(
        cli.config.as_deref(),
        Environment::from_process(),
        cli.vault_addr,
        cli.vault_token.map(Token::from),
        cli.vault_namespace,
        cli.insecure,
    )?;

    tracing::debug!("Configuration loaded: {:?}", config);

    // Validate configuration
    config.validate()?;

    // Printed before the UI takes the screen, so it is what is left on the
    // terminal after exit. The UI shows its own notice while it runs
    if config.insecure {
        eprintln!("⚠️  WARNING: TLS CERTIFICATE VERIFICATION DISABLED ⚠️");
        eprintln!("⚠️  This is INSECURE and should ONLY be used for development!");
        eprintln!("⚠️  Your Vault credentials may be intercepted by attackers!");
        eprintln!("⚠️  NEVER use --insecure in production environments!\n");
    }

    // Initialize and run the TUI application
    let mut tui = Tui::new(config, cli.initial_path)?;
    tui.run().await?;

    tracing::info!("VltView exited successfully");
    Ok(())
}

/// Whether the token was typed as an argument, where the process list shows
/// it. A token from `VAULT_TOKEN` also ends up in the `--vault-token` value,
/// so having a value is not the test.
fn token_on_command_line(matches: &ArgMatches) -> bool {
    matches.value_source("vault_token") == Some(ValueSource::CommandLine)
}

/// Initialize logging based on debug flag
fn init_logging(debug: bool) -> Result<()> {
    let level = if debug { Level::DEBUG } else { Level::INFO };

    // Create the main subscriber with our custom log capture layer
    let subscriber = tracing_subscriber::registry()
        .with(LogCaptureLayer)
        .with(tracing_subscriber::filter::LevelFilter::from_level(level));

    subscriber.init();

    Ok(())
}

/// Custom tracing layer that captures logs for the UI
struct LogCaptureLayer;

impl<S> Layer<S> for LogCaptureLayer
where
    S: tracing::Subscriber,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let metadata = event.metadata();
        let level = metadata.level().to_string();
        let target = metadata.target().to_string();

        // Extract the message from the event
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);

        // Add to log buffer
        tui::log_buffer::add_log(&level, &target, visitor.message);
    }
}

/// Visitor to extract message from tracing event
#[derive(Default)]
struct MessageVisitor {
    message: String,
}

impl tracing::field::Visit for MessageVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{:?}", value);
            // Remove surrounding quotes if present
            if self.message.starts_with('"') && self.message.ends_with('"') {
                self.message = self.message[1..self.message.len() - 1].to_string();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse `args` with the token read from the variable `token_env` instead
    /// of `VAULT_TOKEN`, so that the result does not depend on whether this
    /// machine has a `VAULT_TOKEN`
    fn warns_about_token(args: &[&str], token_env: &'static str) -> bool {
        let matches = Cli::command()
            .mut_arg("vault_token", |arg| arg.env(token_env))
            .try_get_matches_from(args)
            .unwrap();
        token_on_command_line(&matches)
    }

    #[test]
    fn token_warning_only_for_a_token_typed_as_an_argument() {
        // `PATH` is always set: it stands in for a `VAULT_TOKEN` that is
        const SET: &str = "PATH";
        const UNSET: &str = "VLTVIEW_TEST_NO_SUCH_VARIABLE";

        // Regression: a token from the environment got the warning too
        assert!(!warns_about_token(&["vltview"], SET));
        // Token from the config file or `~/.vault-token`
        assert!(!warns_about_token(&["vltview"], UNSET));

        assert!(warns_about_token(&["vltview", "--vault-token", "t"], UNSET));
        assert!(warns_about_token(&["vltview", "--vault-token=t"], SET));
    }

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn help_never_prints_the_token_from_the_environment() {
        // Regression: `--help` printed "[env: VAULT_TOKEN=<the token>]"
        let command = Cli::command();
        let token_arg = command
            .get_arguments()
            .find(|arg| arg.get_id() == "vault_token")
            .expect("vault_token argument");

        assert!(token_arg.is_hide_env_values_set());
    }
}
