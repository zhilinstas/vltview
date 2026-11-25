use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
use tracing::Level;
use tracing_subscriber::{Layer, layer::SubscriberExt, util::SubscriberInitExt};

mod app;
mod clipboard;
mod config;
mod state;
mod tui;
mod vault;

use config::Config;
use tui::Tui;

/// VltView - Terminal UI for HashiCorp Vault
#[derive(Parser, Debug)]
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
    #[arg(long, env = "VAULT_TOKEN")]
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
    let cli = Cli::parse();

    // Warn if token is provided via CLI argument (security issue)
    if cli.vault_token.is_some() {
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
        cli.vault_addr,
        cli.vault_token,
        cli.vault_namespace,
        cli.insecure,
    )?;

    tracing::debug!("Configuration loaded: {:?}", config);

    // Validate configuration
    config.validate()?;

    // Initialize and run the TUI application
    let mut tui = Tui::new(config, cli.initial_path)?;
    tui.run().await?;

    tracing::info!("VltView exited successfully");
    Ok(())
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
