# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

VltView is a terminal UI (TUI) application for browsing and managing HashiCorp Vault secrets. Built with Ratatui for the UI framework and Tokio for async operations, it follows a clean three-layer architecture with component-based UI.

What it works with: the KV version 2 secrets engine mounted at `secret/` (the mount is fixed in `vault/kv.rs`; KV v1 and other mounts are not supported), ACL policies (read only), and token authentication. Nothing is cached: every view is a fresh read from Vault.

## Build & Development Commands

### Toolchain

Tool versions are pinned in `mise.toml` (Rust 1.100.0 with rustfmt and clippy, cargo-audit). CI and the release workflow install from the same file via `jdx/mise-action`, so bump versions there and keep `rust-version` in `Cargo.toml` in sync. The crate uses edition 2024.

```bash
mise install            # Install the pinned toolchain (also: make setup)
mise run ci             # What CI runs: fmt-check, clippy -D warnings, test, audit (also: make ci)
```

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
make run-with-docker    # make docker-up, then run in the sample folder /myapp (token: root)
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
make audit         # Check Cargo.lock against the RustSec advisory database
make pre-commit    # Full pre-commit check (fmt, clippy, test, build)
```

### Docker Environment
```bash
make docker-up        # Start Vault dev server (token: root), seed the sample data, wait for the seed step
make docker-down      # Stop and remove the containers (the data is in memory and goes with them)
make docker-logs      # Follow the logs of both containers
make docker-logs-init # Output of the seed step
make docker-clean     # docker-down, plus volumes
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
- **Root** (`root.rs`): Top-level component managing screen transitions, async completions, and global state. It decides which results are still wanted (the secret under the cursor, the latest folder listing).
- **Key table** (`keymap.rs`): The hints for every context. Footers and the help overlay are generated from it, and a test presses every key in it.

**Component hierarchy:**
- `Root` (screen manager)
  - `MainScreen` (split view: navigation + content)
    - `NavigationPane` (left: tree browser with filter)
    - `DetailViewComponent` (right: view secret data/versions)
    - `EditViewComponent` (right: edit secret with TextArea)
  - `PolicyScreen` (view/search policies)
  - `LogScreen` (application logs)
  - `HelpOverlay` (modal help display)

**Critical pattern:** Components never directly call async operations. They emit actions (e.g., `Action::OpenSecret`), Root spawns async tasks via AsyncManager, which returns completion actions (e.g., `Action::SecretLoaded`, or `Action::SecretLoadFailed`) that components handle in `update()`.

### 2. App Layer (`src/app/`)

Business logic layer. Service-oriented design with Arc/RwLock for shared state.

**Key services:**
- **NavigationController** (`navigation.rs`): Lists folders. It only reads; what is on screen changes in `AppState`
- **SecretService** (`secret.rs`): Reads a secret with its version history, reads one version, writes and deletes. The UI goes through it rather than calling the Vault client. The module also holds the rules for showing, masking and saving values (`display_value`, `masked`, `edited_value`)
- **PolicyService** (`policy.rs`): List and retrieve Vault policies
- **FilterEngine** (`filter.rs`): Fuzzy search/filtering for UI lists

**State management** (`src/state/`):
- `AppState` uses interior mutability (RwLock) for concurrent access
- Stores the folder on screen (path and items) and the history of folders visited. There is no cache: every view is a fresh read from Vault
- Thread-safe via Arc wrapping

### 3. Vault Client Layer (`src/vault/`)

HTTP client for Vault API using reqwest + Tokio.

**Modules:**
- `kv.rs`: KV v2 operations on the mount `secret/` (list, read, read a version, write, delete with all versions, read metadata)
- `policy.rs`: Policy API (list, read policies)
- `auth.rs`: `Token` (wiped on drop, not `Clone`) and `AuthManager`, its one owner
- `request.rs`: What every request shares: `authorize` (token and namespace headers) and `ensure_success` (any other status becomes a sanitized error)
- `response.rs`: Types of Vault's answers
- `sanitize.rs`: Path and namespace sanitization, URL building (`vault_url`), error sanitization
- `audit_log.rs`: One log entry per operation (action, path, count), never values

**Security features:**
- **Rate limiting**: Semaphore-based (MAX_CONCURRENT_REQUESTS = 10) applied to all KV and Policy API operations
- **Path sanitization**: `sanitize_vault_path()` prevents path traversal attacks
- **Namespace sanitization**: `sanitize_namespace()` prevents header injection (Enterprise)
- **Error sanitization**: `sanitize_vault_error()` prevents information disclosure
- **Audit logging**: Each operation is logged via the `audit_log` module with its action, the full path and a count, and shown on the log screen. This is the session's own log inside the app; it is not written to a file and it is not Vault's audit log
- **Data redaction**: `redact_secret_data()` shows keys but hides values in logs

## Key Patterns

### Action Flow
```rust
// Component emits action in handle_key()
fn handle_key(&mut self, key: KeyEvent, ctx: &ComponentContext) -> Result<Option<Action>> {
    match key.code {
        KeyCode::Enter => Ok(Some(Action::OpenSecret { path: "...".into() })),
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

### Selection and Preview

The navigation cursor is the selection, and the content pane shows it.

- `NavigationPane` emits `Action::SelectionChanged` whenever the cursor rests on a different item, whatever moved it (cursor keys, reload, filter, changing folder). Do not load a secret from a key handler; change the selection and let this action do it.
- `Root::selection_changed` loads the secret. The first cursor step after a pause loads at once; while the cursor keeps moving, only the secret it stops on is loaded (`PREVIEW_DEBOUNCE`). Every load is a read in Vault's audit log, so scrolling must not read what it passes.
- `MainScreen` never leaves the previous secret on screen: a selected secret that has not arrived is a "Loading" placeholder.
- A reload keeps the cursor on the same name (`NavigationPane::reload`).

### Component Communication
- Components emit actions (return `Some(Action)` from `handle_key()` or `update()`)
- Root processes actions sequentially
- Parent components pass actions to children via `update(action, ctx)`
- No direct component-to-component communication

### State Access
- Read-only access via `&ComponentContext` (contains `&App`)
- Changes go through actions. Requests to Vault are made by the App layer services, in the background; what is on screen (`AppState`) is changed only by `Root`, when a result it still wants arrives
- UI state (selection, scroll) lives in components
- Business state (the folder on screen, the history) lives in AppState

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
│   ├── keymap.rs           # Key hints per context (footers and help)
│   ├── log_buffer.rs       # Global log storage for UI
│   └── components/         # UI components
│       ├── main_screen.rs  # Main split-view screen
│       ├── navigation_pane.rs  # Left: tree browser
│       ├── detail_view.rs  # Right: view secret
│       ├── edit_view.rs    # Right: edit secret
│       ├── policy_screen.rs # Policy viewer
│       ├── log_screen.rs   # Log viewer
│       ├── help_overlay.rs # Help modal
│       └── popup.rs        # Shared helpers: centered popups, truncate, wrap
├── vault/                  # Vault API client
│   ├── mod.rs              # VaultClient
│   ├── kv.rs               # KV engine API
│   ├── policy.rs           # Policy API
│   ├── auth.rs             # Token and its one owner
│   ├── request.rs          # Headers and status check shared by all requests
│   ├── response.rs         # Types of Vault's answers
│   ├── sanitize.rs         # Path, namespace and error sanitization, URLs
│   └── audit_log.rs        # What is logged about each operation
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

### Adding or Changing a Key Binding

1. Handle the key in the component's `handle_key()`
2. Add or change the entry in `src/tui/keymap.rs` for that context. Footers and the help overlay are generated from it, so a key that is not there is not shown anywhere
3. Footer labels are short; hints that do not fit the terminal width are dropped by priority, never cut
4. A second key for the same thing gets its own entry with footer priority 0 ("The same as c"), not a mention in another entry's help text: `every_hinted_key_does_something_where_it_is_hinted` in `root.rs` presses each key of each entry in the place it is hinted for, and can only press what the table spells out. A new context needs a way in there (`ways_into`)
5. Do not write a key hint as a string anywhere else, in a dialog either

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

// Write: only `Root` does, when the result of a request it still wants arrives
app.state().show_folder(path, items);
```

The path on screen and its items change together (`show_folder`, `reload_folder`). A folder listing runs in the background and only reads; `Root` numbers each request and applies the result of the latest one only, so a slow listing cannot put one folder's items under another folder's path.

## Configuration

Configuration priority: CLI args > env vars > config file > defaults

Config file location: `~/.vltview.yaml`. Every key is optional. A file that
does not parse, or has a key `Config` does not know, stops the start with an
error naming the file; only a missing default file is not an error.

`Config::load` does not read the process environment or the home directory
itself: `main` passes them in as `config::Environment`, and tests pass their
own.

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
- A failed request comes back as an action that says what failed (`SecretLoadFailed`, `SaveFailed`, `DeleteFailed`, `FolderListFailed`, `VersionLoadFailed`, `PolicyLoadFailed`, ...), with a message that names the path or policy. There is no catch-all error action
- An error is shown where its subject is: a failed load in the content pane while that secret is selected, a failed save inside the editor it came from, a policy error on the policy screen. Anything else (a folder that would not open, a failed delete or copy) is a message over the main screen (`MainScreen::notices`); while an editor is open, such a message waits and is shown when the editor is closed. A failed read of something the user has left is dropped
- Components display errors in UI, don't panic

### Terminal Lifecycle
- `Tui::new` takes the terminal last (`take_terminal`), after everything that can fail. An error before that, or from `Tui::run`, is printed by `main` on a normal terminal
- `restore_terminal` gives it back, once: when the `Tui` is dropped and from the panic hook. A panic on the UI thread restores the terminal and then prints its message and location; a panic in a background task is written to the application log and the UI goes on
- Nothing printed to stderr is visible while the UI has the screen. Startup warnings are printed in `main` before the terminal is taken, which leaves them on the terminal after exit; one that matters during the session is also shown in the UI (`show_insecure_notice`)
- The "token on the command line" warning is decided by `token_on_command_line` from where clap got the value. `cli.vault_token` is also set from `VAULT_TOKEN`

### Focus Management
- Which pane has focus is state of the screen that owns the panes (`MainScreen::focused_pane`); it tells each pane through `RenderContext::is_focused`
- Components check `RenderContext::is_focused` for styling
- Tab key cycles focus between panes

### Viewport & Scrolling
- Components manage their own scroll state
- A list that can outgrow its pane must keep the cursor on screen, and has a render test that proves it. Lists of one-row items (navigation, fields, versions, editor, policies) are rendered with a `ListState` kept in the component, which does that
- The log screen is the exception: an entry can wrap over several rows, so it lays its rows out itself and scrolls by row
- Paging needs the height the list had when it was last drawn; components store it in `draw()`

### Layout
- The main screen has one header line and one footer line; each pane is a single bordered block with its state on the border. Do not add nested boxes: at 80x24 every row counts. The one box inside a pane is the text box of the field being typed in the editor: it is there only while typing, and it has to stand out from the list above it
- Popups have a fixed size (`popup::centered`), not a percentage of the screen, so their text is all on screen on a small terminal. A question or a message over the main screen is drawn with `popup::dialog`, which adds the keys that answer it from the key table
- Text that carries a warning or instructions is wrapped or dropped whole, never cut off mid-sentence. Check new UI at 80x24 (the render tests draw into a `TestBackend` of that size)

### KV Version Handling
- Only KV v2 is supported, at the mount `secret/` (`KV_MOUNT` in `kv.rs`). KV v1 and configurable mounts are not implemented
- Paths in the UI are relative to the mount: `/myapp` is `secret/myapp` in Vault. The client adds `data/` or `metadata/` (`secret/data/myapp`); a path that starts with `secret/` would be doubled
- A new secret is written with check-and-set 0; an edit is an unconditional write of a new version
- Delete removes the metadata, that is every version; there is no soft delete
- DetailView shows the version history and can load any version

### Logging
Uses `tracing` crate. Events are captured by `LogCaptureLayer` (`src/tui/log_buffer.rs`) with their message and their structured fields, and stored in the global `LogBuffer`. The log screen shows both, so a field is as visible as the message: the rule about tokens and secret values covers fields too.

```rust
tracing::info!("User action completed");
tracing::warn!(attempt = 2, "Failed to load metadata");
tracing::error!("Operation failed: {}", error);
```

- Audit entries (`src/vault/audit_log.rs`) say what happened to what in fields: `action`, then `path` or `policy`, then counts. They have no message; the log screen shows `secret.list path=/myapp count=3`
- "Authenticated" (`auth.success`) is logged by `AuthAudit::accepted()` once Vault has answered a request with success, never when the client is created
- Tests read the log through `log_buffer::capture()`, which gives each test a buffer of its own. Do not use `tracing::subscriber::set_default` in tests: with tests running in parallel it loses events

## Testing with Vault Dev Server

The Docker dev environment works from a fresh clone; nothing has to be run by hand after it:

```bash
make docker-up          # Vault at http://localhost:8200, token root, with the sample data
make run-with-docker    # the same, then: cargo run -- /myapp
```

`docker-compose.yml` runs two services:
1. `vault` (container `vltview-vault`): HashiCorp Vault in dev mode. Everything is in memory, so removing the container (`make docker-down`) discards the data
2. `vault-init` (container `vltview-vault-init`): runs `vault-init.sh` and exits. `make docker-up` waits for it and fails if its exit status is not 0. Compose runs it again on every `make docker-up`; the script does nothing when the sample data is already there, so edits made in a running Vault are kept. For a fresh start: `make docker-down && make docker-up`

`vault-init.sh` creates:
- Sample secrets under `secret/`: `myapp`, `prod`, `staging`, `dev`, `test` (with multi-line values), and the nested `company` and `services`
- Five policies: `admin-policy`, `readonly-policy`, `dev-policy`, `prod-readonly-policy`, `myapp-policy`

Rules for this environment:
- `vault-init.sh` runs inside the Vault image, which has no bash, only busybox `sh`. Keep the script POSIX: check with `sh -n vault-init.sh` and `shellcheck -s sh vault-init.sh`, then run `make docker-up`
- The path given to the app is inside the KV mount: `/myapp` is `secret/myapp` in Vault. `/secret/myapp` would be `secret/secret/myapp`, which does not exist
- The container names are fixed, so only one checkout can run the environment at a time. Stop it with `make docker-down` in the checkout that started it
- `./docker-build.sh` builds a release binary in Ubuntu 20.04 from `Dockerfile.compat` (for older glibc) and writes it to `target/vltview`

## Async Operation Pattern

The AsyncManager bridges async Vault API calls with the synchronous TUI event loop:

```rust
// In Root (see `spawn_load_secret`)
let secret_service = app.secret_service().clone();
let failed_path = path.clone();

self.async_manager.spawn(
    async move {
        let secret = secret_service.get(&path).await?;
        Ok(Action::SecretLoaded { path, data: secret.data, metadata })
    },
    move |e| Action::SecretLoadFailed {
        message: format!("Could not load {failed_path}: {e}"),
        path: failed_path.clone(),
    },
);
```

Key points:
- Clone the service before moving it into the future; the future only talks to Vault and never touches `AppState` or the UI
- `spawn` takes the future and a function that turns its error into an action; it sends whichever action results
- Both the success and the failure say what they are for, so `Root` can drop them if the user has moved on
- Main loop polls `async_manager.poll_completions()` to receive actions

## Release Process

Pushing a tag that starts with `v` runs `.github/workflows/release.yml`:

```bash
# Create and push a version tag
git tag v0.1.0
git push origin v0.1.0
```

1. Four `build` jobs (Linux glibc and musl, macOS Intel and ARM) each build a tarball and its SHA256 checksum and hand them on as a workflow artifact. They have a read-only token.
2. When all four have succeeded, the `release` job checks the eight files against their checksums, then creates the GitHub release and uploads them with `gh release create`. It is the only job with `contents: write`. If a build fails, no release is created.

A tag with a hyphen (`v0.3.0-rc.1`) becomes a pre-release, so it is never shown as the latest release. Release notes are the ones GitHub generates; edit them on the release page.

Files on the release, each with a `.sha256` next to it (check with `sha256sum -c <file>.sha256`):
- `vltview-linux-amd64.tar.gz` (glibc)
- `vltview-linux-amd64-musl.tar.gz` (static)
- `vltview-macos-amd64.tar.gz` (Intel)
- `vltview-macos-arm64.tar.gz` (Apple Silicon)

### Workflow Rules

- Every action in `.github/workflows/` is pinned to a full commit SHA with the version in a trailing comment (`uses: actions/checkout@<sha> # v7.0.1`). Look the SHA up, do not type it from memory: `gh api repos/<owner>/<repo>/git/ref/tags/<tag>`; if that returns an annotated tag (`"type": "tag"`), follow it with `gh api repos/<owner>/<repo>/git/tags/<sha>`
- Workflows start from `permissions: contents: read`; a job that needs more says so itself
- Do not use `actions/create-release` or `actions/upload-release-asset` (both archived); use the `gh` CLI with `GH_TOKEN: ${{ github.token }}`
- Check workflow changes with `mise x actionlint@latest shellcheck@latest -- actionlint`

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
Every operation is logged with what was done to what, and never with a value:
```rust
// secret.write path=/team/db fields=3
audit_log::log_secret_write(&path, data.len());

// Field names only, for debug output
let redacted = audit_log::redact_secret_data(&data); // every value is "***REDACTED***"
```
"Authenticated" is logged by `AuthAudit::accepted()` after the first request Vault accepts, not when the client is built. A test (`no_entry_contains_a_secret_value_or_the_token`) reads the whole log after a write and a read; keep it passing.

### Clipboard Auto-Clear
Clipboard manager automatically clears copied secrets after 30 seconds:
- Logs copy operation: `audit_log::log_clipboard_copy(size, timeout)`
- Verifies content unchanged before clearing (prevents clearing user's own copies)
- Logs auto-clear events: `audit_log::log_security_event("clipboard.auto_cleared", ...)`

### When Adding New Features
- **Never log tokens or secret values**, not even in part. `audit_log::redact_secret_data()` keeps the field names and drops every value. The one thing logged about a value is the size in bytes of what was copied to the clipboard (`log_clipboard_copy`); do not add to that
- **The token has one owner.** `vault::Token` is not `Clone` and is wiped on drop. It moves from where it was read into `Config`, and from there into the `AuthManager` when `VaultClient::new` takes the configuration; the client, `KvEngine` and `PolicyApi` share that `Arc<AuthManager>`. Do not copy `Token::expose()` into a `String`, do not make either type `Clone`, and do not keep a `Config` around. What this does not cover (copies inside reqwest, the environment, the argument list) is listed under "Security Notes" in the README; keep that list true
- **Apply rate limiting** to new Vault API operations
- **Sanitize all user input** going to Vault API
- **Build request URLs with `sanitize::vault_url()`**, never `format!`. It percent-encodes each path segment, so a name containing `%2e%2e`, `?` or `#` cannot change which endpoint is called
- **Check the HTTP status of every response** (`ensure_success` in `request.rs`). A write that Vault rejects must reach the UI as an error
- **Create with `WriteMode::Create`**, which writes with check-and-set 0: Vault refuses it if anything exists at the path. Checking the name against the folder on screen is a convenience, not the guard; it cannot see a nested name or a secret created by someone else since the folder was listed
- **Keep secret values as `vault::SecretData`** (`HashMap<String, serde_json::Value>`), not strings. Converting to `String` anywhere on the load path deletes non-string fields on the next save. Use `app::secret::display_value()` to show a value and `edited_value()` to turn edited text back into a value
- **Async completions must say what they are for** (path, version, policy name, request number), failures included, and be ignored when the user has moved on. See the `SecretLoaded` guard and `take_listing` in `Root`, `DetailViewComponent::version_arrived`, and `PolicyScreen::awaited`
- **Pasted text is data**: it arrives as `Event::Paste` and goes through `Component::handle_paste`, which only text inputs act on. Never feed it to `handle_key`
- Use TLS verification by default (`--insecure` flag for dev only, warns user)
