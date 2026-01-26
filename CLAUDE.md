# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

VltView is a terminal UI (TUI) application for browsing and managing HashiCorp Vault secrets. Built with Ratatui for the UI framework and Tokio for async operations, it follows a clean three-layer architecture with component-based UI.

## Build & Development Commands

### Building
```bash
make build              # Debug build
make build-release      # Optimized release build
make check              # Check compilation without building
```

### Running
```bash
make run                # Run with fake credentials (VAULT_ADDR=localhost:8200, VAULT_TOKEN=test-token)
make run-debug          # Run with debug logging
make run-with-docker    # Start docker-compose and run
make dev                # Development mode with cargo watch
```

### Testing
```bash
make test                           # Run all tests
make test-verbose                   # Run tests with output
make test-specific TEST=test_name   # Run specific test
```

### Code Quality
```bash
make fmt           # Format code with rustfmt
make clippy        # Run clippy linter
make lint          # Run both fmt-check and clippy
make pre-commit    # Full pre-commit check (fmt, clippy, test, build)
```

### Docker Environment
```bash
make docker-up        # Start Vault dev server (token: root)
make docker-down      # Stop Docker services
make docker-logs      # View Vault logs
make docker-clean     # Remove containers and volumes
```

## Architecture

### Three-Layer Design

VltView follows a strict separation of concerns across three layers:

```
TUI Layer (src/tui)
    ↕ Actions (event-driven)
App Layer (src/app)
    ↕ API calls (async)
Vault Client Layer (src/vault)
```

### 1. TUI Layer (`src/tui/`)

Component-based architecture using Ratatui. The Root component manages screens and routes actions.

**Key concepts:**
- **Actions** (`action.rs`): All state changes flow through the `Action` enum. Components emit actions, Root processes them.
- **Components** (`component.rs`): Trait-based system. Each component has `init()`, `handle_key()`, `update()`, `draw()`, and `captures_input()`.
- **AsyncManager** (`async_manager.rs`): Spawns async operations on Tokio runtime, returns completed actions via channels.
- **Root** (`root.rs`): Top-level component managing screen transitions, async completions, and global state.

**Component hierarchy:**
- `Root` (screen manager)
  - `MainScreen` (split view: navigation + content)
    - `NavigationPane` (left: tree browser with filter)
    - `DetailViewComponent` (right: view secret data/versions)
    - `EditViewComponent` (right: edit secret with TextArea)
  - `PolicyScreen` (view/search policies)
  - `LogScreen` (application logs)
  - `HelpOverlay` (modal help display)

**Critical pattern:** Components never directly call async operations. They emit actions (e.g., `Action::ViewSecret`), Root spawns async tasks via AsyncManager, which returns completion actions (e.g., `Action::SecretLoaded`) that components handle in `update()`.

### 2. App Layer (`src/app/`)

Business logic layer. Service-oriented design with Arc/RwLock for shared state.

**Key services:**
- **NavigationController** (`navigation.rs`): Manages path navigation, directory listing, history
- **SecretService** (`secret.rs`): CRUD operations for secrets, version management
- **PolicyService** (`policy.rs`): List and retrieve Vault policies
- **FilterEngine** (`filter.rs`): Fuzzy search/filtering for UI lists

**State management** (`src/state/`):
- `AppState` uses interior mutability (RwLock) for concurrent access
- Stores current path, selection, cache, navigation history
- Thread-safe via Arc wrapping

### 3. Vault Client Layer (`src/vault/`)

HTTP client for Vault API using reqwest + Tokio.

**Modules:**
- `kv.rs`: KV v1 and v2 secret engine operations (list, read, write, delete)
- `policy.rs`: Policy API (list, read policies)
- `auth.rs`: Authentication manager (token-based, extensible for AppRole/LDAP)
- `request.rs`: Request builders with authentication headers
- `response.rs`: Response parsing and error handling
- `sanitize.rs`: Path sanitization for Vault API

**Security features:**
- **Rate limiting**: Semaphore-based (MAX_CONCURRENT_REQUESTS = 10) applied to all KV and Policy API operations
- **Path sanitization**: `sanitize_vault_path()` prevents path traversal attacks
- **Namespace sanitization**: `sanitize_namespace()` prevents header injection (Enterprise)
- **Error sanitization**: `sanitize_vault_error()` prevents information disclosure
- **Audit logging**: All operations logged via `audit_log` module with redaction of sensitive data
- **Data redaction**: `redact_secret_data()` shows keys but hides values in logs

## Key Patterns

### Action Flow
```rust
// Component emits action in handle_key()
fn handle_key(&mut self, key: KeyEvent, ctx: &ComponentContext) -> Result<Option<Action>> {
    match key.code {
        KeyCode::Enter => Ok(Some(Action::ViewSecret { path: "...".into(), should_focus: true })),
        _ => Ok(None)
    }
}

// Root processes action (may spawn async op)
// AsyncManager spawns task, returns completion action
// Component receives completion in update()
fn update(&mut self, action: Action, ctx: &mut ComponentContext) -> Result<Option<Action>> {
    match action {
        Action::SecretLoaded { path, data, metadata } => {
            self.detail_view = Some(DetailViewComponent::new(data, path));
            Ok(None)
        }
        _ => Ok(None)
    }
}
```

### Component Communication
- Components emit actions (return `Some(Action)` from `handle_key()` or `update()`)
- Root processes actions sequentially
- Parent components pass actions to children via `update(action, ctx)`
- No direct component-to-component communication

### State Access
- Read-only access via `&ComponentContext` (contains `&App`)
- Mutations go through actions → App layer services
- UI state (selection, scroll) lives in components
- Business state (paths, cache) lives in AppState

## File Organization

```
src/
├── main.rs                 # Entry point, CLI args, logging setup
├── app/                    # Business logic layer
│   ├── mod.rs              # App struct orchestrating services
│   ├── navigation.rs       # Path navigation, directory listing
│   ├── secret.rs           # Secret CRUD operations
│   ├── policy.rs           # Policy operations
│   └── filter.rs           # Search/filter engine
├── tui/                    # Terminal UI layer
│   ├── mod.rs              # Tui struct, event loop
│   ├── root.rs             # Root component (screen manager)
│   ├── action.rs           # Action enum (all state changes)
│   ├── component.rs        # Component trait
│   ├── async_manager.rs    # Async operation spawner
│   ├── context.rs          # ComponentContext, RenderContext
│   ├── log_buffer.rs       # Global log storage for UI
│   └── components/         # UI components
│       ├── main_screen.rs  # Main split-view screen
│       ├── navigation_pane.rs  # Left: tree browser
│       ├── detail_view.rs  # Right: view secret
│       ├── edit_view.rs    # Right: edit secret
│       ├── policy_screen.rs # Policy viewer
│       ├── log_screen.rs   # Log viewer
│       └── help_overlay.rs # Help modal
├── vault/                  # Vault API client
│   ├── mod.rs              # VaultClient
│   ├── kv.rs               # KV engine API
│   ├── policy.rs           # Policy API
│   ├── auth.rs             # Authentication
│   ├── request.rs          # Request builders
│   └── response.rs         # Response parsing
├── state/                  # Application state
│   └── mod.rs              # AppState with RwLock
├── config/                 # Configuration
│   └── mod.rs              # Config from file/env/CLI
└── clipboard/              # Clipboard integration
    └── mod.rs              # Cross-platform clipboard
```

## Common Tasks

### Adding a New Action

1. Add variant to `Action` enum in `src/tui/action.rs`
2. Handle action in relevant component's `update()` method
3. If async operation needed, spawn in `Root::process_action()` via AsyncManager
4. Return completion action from async task

### Adding a New Component

1. Create component struct implementing `Component` trait
2. Add to parent component (usually Root or MainScreen)
3. Call `init()`, `handle_key()`, `update()`, `draw()` from parent
4. Route focus/events appropriately

### Adding Vault API Operation

1. Add method to appropriate API struct in `src/vault/` (KvEngine, PolicyApi, etc.)
2. Create service method in `src/app/` that calls Vault API
3. Add Action variant(s) for request and completion
4. Handle in component via async action flow

### Working with State

State is in `AppState` (Arc&lt;RwLock&gt;). Access patterns:
```rust
// Read
let path = ctx.app.state().current_path();

// Write (via service methods, not direct RwLock access)
ctx.app.navigation().navigate_to(new_path).await?;
```

## Configuration

Configuration priority: CLI args > env vars > config file > defaults

Config file location: `~/.vltview.yaml`

Environment variables:
- `VAULT_ADDR`: Vault server address
- `VAULT_TOKEN`: Vault token
- `VAULT_NAMESPACE`: Vault namespace (Enterprise)

## Important Implementation Notes

### Thread Safety
- `AppState` uses `RwLock` for concurrent reads, exclusive writes
- `VaultClient` is `Arc`-wrapped and shared across tasks
- Async operations run on Tokio runtime, results sent via channels

### Error Handling
- Use `anyhow::Result` for most operations
- Vault API errors are surfaced as `Action::SetError`
- Components display errors in UI, don't panic

### Focus Management
- `FocusState` tracks current focus (global, screen-level, pane-level)
- Components check `RenderContext::is_focused` for styling
- Tab key cycles focus between panes

### Viewport & Scrolling
- Components manage their own scroll state
- Adjust scroll in `draw()` before rendering to keep selection visible
- Use `Block::borders()` height to calculate viewport size

### KV Version Handling
- Supports both KV v1 (unversioned) and KV v2 (versioned)
- KV v2 paths have `/data/` injected by client (e.g., `secret/data/myapp`)
- DetailView shows version selector for KV v2 secrets

### Logging
Uses `tracing` crate. Logs are captured by `LogCaptureLayer` in `main.rs` and stored in global buffer for UI display.

```rust
tracing::info!("User action completed");
tracing::debug!("API response: {:?}", response);
tracing::error!("Operation failed: {}", error);
```

## Testing with Vault Dev Server

The project includes a complete Docker-based development environment:

```bash
# Start Vault dev server and initialize with test data
make docker-up
./vault-init.sh   # Optional: populate with sample secrets/policies

# The vault-init.sh script creates:
# - Sample secrets at various paths (secret/myapp/*, secret/test/*)
# - Test policies (admin, readonly, etc.)
# - Nested directory structures for testing navigation

# Vault dev server credentials:
# - Address: http://localhost:8200
# - Token: root
```

The docker-compose.yml runs:
1. `vault` service: HashiCorp Vault in dev mode
2. `vault-init` service: One-time initialization container

## Async Operation Pattern

The AsyncManager bridges async Vault API calls with the synchronous TUI event loop:

```rust
// In Root::process_action()
let app_clone = Arc::clone(&self.app);
let tx = self.async_manager.action_sender();

self.async_manager.spawn(async move {
    match app_clone.secret_service().get_secret(&path).await {
        Ok(data) => {
            tx.send(Action::SecretLoaded { path, data, metadata }).unwrap();
        }
        Err(e) => {
            tx.send(Action::SetError { message: e.to_string() }).unwrap();
        }
    }
});
```

Key points:
- Clone `Arc<App>` before moving into async closure
- Get `tx` sender before spawning
- Send completion action (success or error) via channel
- Main loop polls `async_manager.poll_completions()` to receive actions

## Release Process

GitHub Actions automatically build multi-platform binaries on git tags:

```bash
# Create and push a version tag
git tag v0.1.0
git push origin v0.1.0

# GitHub Actions will:
# 1. Build for Linux (glibc and musl), macOS (Intel and ARM)
# 2. Create GitHub release
# 3. Upload tarballs with SHA256 checksums
```

Binaries produced:
- `vltview-linux-amd64.tar.gz` (glibc)
- `vltview-linux-amd64-musl.tar.gz` (static)
- `vltview-macos-amd64.tar.gz` (Intel)
- `vltview-macos-arm64.tar.gz` (Apple Silicon)

## Security Features

This application implements multiple layers of security for handling Vault secrets:

### Rate Limiting
All Vault API calls use a semaphore to limit concurrent requests (max 10):
```rust
// Automatically applied in KvEngine and PolicyApi
let _permit = self.rate_limiter.acquire().await?;
// Make API call
```

### Input Sanitization
- **Path sanitization**: Removes `..`, `//`, validates no control characters
- **Namespace sanitization**: Prevents header injection attacks (`\n`, `\r`)
- **Error sanitization**: Redacts internal details from user-facing errors

### Audit Logging with Redaction
All sensitive operations are logged WITHOUT exposing secrets:
```rust
// Log secret write - shows key count, not values
audit_log::log_secret_write(&path, data.len());

// Redact data before logging
let redacted = audit_log::redact_secret_data(&data); // Shows keys: "***REDACTED***"
```

### Clipboard Auto-Clear
Clipboard manager automatically clears copied secrets after 30 seconds:
- Logs copy operation: `audit_log::log_clipboard_copy(size, timeout)`
- Verifies content unchanged before clearing (prevents clearing user's own copies)
- Logs auto-clear events: `audit_log::log_security_event("clipboard.auto_cleared", ...)`

### When Adding New Features
- **Never log tokens or secret values** - use `audit_log::redact_secret_value()` if needed
- **Apply rate limiting** to new Vault API operations
- **Sanitize all user input** going to Vault API
- Use TLS verification by default (`--insecure` flag for dev only, warns user)
