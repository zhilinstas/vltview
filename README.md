# ⚙️ VltView

Terminal UI application for browsing and managing HashiCorp Vault secrets.

## Features

- Browse the KV version 2 secrets engine mounted at `secret/`, folder by
  folder; the secret under the cursor is shown next to the list
- Values are masked until you reveal them, one at a time or all at once
- Create, edit and delete secrets; values keep their JSON type (numbers,
  booleans, lists) through an edit
- Version history: view any version, or restore one as a new version
- Fuzzy filter within the current folder
- Copy a path or a value to the clipboard; what was copied is cleared again
  after 30 seconds
- Read ACL policies
- A log of what this session did (which secret was read, written or
  deleted), inside the app
- Vault Enterprise namespaces

Not supported: KV version 1, mounts other than `secret/`, authentication
other than a token, and editing policies. Nothing is cached; every view is a
fresh read from Vault, and each read appears in Vault's own audit log.

## Screenshots

### Navigation View
![Navigation View](screenshots/1.png)
*Browse Vault KV secrets with tree navigation*

### Secret Details
![Secret Details](screenshots/2.png)
*View secret keys and values with masked display*

### Edit Secret
![Edit Secret](screenshots/3.png)
*Edit secrets with multi-line support*

### Version History
![Version History](screenshots/4.png)
*Browse and restore previous secret versions (KV v2)*

### Help Overlay
![Help Overlay](screenshots/5.png)
*Quick access to all keyboard shortcuts*

### Application Logs
![Application Logs](screenshots/6.png)
*Real-time log monitoring for debugging*

## Architecture

Three layers, each talking only to the one below it:

```
┌─────────────────────────────────────────┐
│            TUI layer (src/tui)          │
│  Root: screens, key routing, which      │
│        results are still wanted         │
│  ├─ Main screen: list + secret/editor   │
│  ├─ Policy screen                       │
│  ├─ Log screen                          │
│  └─ Help overlay                        │
└─────────────────────────────────────────┘
              ↕ actions
┌─────────────────────────────────────────┐
│          App layer (src/app)            │
│  ├─ NavigationController: list folders  │
│  ├─ SecretService: read, write, delete  │
│  ├─ PolicyService: list, read           │
│  └─ FilterEngine: fuzzy filter          │
└─────────────────────────────────────────┘
              ↕ async calls
┌─────────────────────────────────────────┐
│       Vault client (src/vault)          │
│  ├─ Token authentication                │
│  ├─ KV v2 on the mount secret/          │
│  └─ ACL policies (read)                 │
└─────────────────────────────────────────┘
```

### Key Modules

- **`tui`**: Terminal UI with Ratatui: components that emit actions, and a
  root that processes them and starts the requests
- **`app`**: Services between the UI and the Vault client
- **`vault`**: HTTP client for the Vault API (reqwest + tokio), with rate
  limiting, path sanitization and the session log
- **`state`**: The folder on screen and the history of folders visited
- **`config`**: Configuration from file, environment and command line
- **`clipboard`**: Clipboard access with auto-clear

## Installation

### From Source

Requires Rust 1.100 or newer. The exact toolchain is pinned in `mise.toml`;
with [mise](https://mise.jdx.dev) installed, `mise install` sets it up.

```bash
mise install            # optional: pinned Rust toolchain
cargo build --release
sudo cp target/release/vltview /usr/local/bin/
```

### Using Cargo

```bash
cargo install --path .
```

## Usage

### Basic Usage

```bash
# Connect using environment variables
export VAULT_ADDR=http://localhost:8200
export VAULT_TOKEN=your-token
vltview

# Or use CLI flags
vltview --vault-addr http://localhost:8200 --vault-token your-token

# Start in a folder. Paths are relative to the KV mount `secret/`, which
# vltview adds itself: this opens secret/myapp
vltview /myapp

# With config file
vltview --config ~/.vltview.yaml
```

### Configuration File

`~/.vltview.yaml` is read if it exists; `--config FILE` names another file.
Every key is optional:

```yaml
vault_addr: http://localhost:8200
vault_token: your-token
vault_namespace: admin  # Vault Enterprise only
insecure: false         # true skips TLS certificate verification, like --insecure
timeout_secs: 30        # request timeout
```

| Key | Environment | Command line | Default |
|-----|-------------|--------------|---------|
| `vault_addr` | `VAULT_ADDR` | `--vault-addr` | none, required |
| `vault_token` | `VAULT_TOKEN` | `--vault-token` | contents of `~/.vault-token` |
| `vault_namespace` | `VAULT_NAMESPACE` | `--vault-namespace` | none |
| `insecure` | `VAULT_SKIP_VERIFY` (`1` or `true`) | `--insecure` | `false` |
| `timeout_secs` | | | `30` |

A value on the command line wins over the environment, which wins over the
file. The file may hold only some of the values, for example the address,
with the token coming from `VAULT_TOKEN`.

A file that is not valid YAML, or that has a key not listed here, stops the
start with an error naming the file and the problem. `cache_enabled` and
`cache_ttl` are accepted for compatibility and have no effect.

### Key Bindings

The footer always shows the keys that work where you are, and `?` lists all
of them. The main ones:

| Where | Key | Action |
|-------|-----|--------|
| Main screen | `Tab`, `h`/`l`, `←`/`→` | Switch between the list and the secret |
| | `r` | Reload the current folder |
| | `p` / `~` | Policies / application logs |
| | `?` | Help |
| | `q` | Quit (asks if an edit has unsaved changes) |
| | `Ctrl+C` | Quit from anywhere, at once |
| List | `↑/↓` or `j/k`, `PgUp/PgDn`, `g/G`, `Home/End` | Move; the secret under the cursor is shown |
| | `Enter` | Open the folder, or go into the secret |
| | `Backspace` | Parent folder |
| | `b` / `f` | Back / forward through the folders visited |
| | `/` | Filter the folder (`Esc` clears) |
| | `n` / `d` | New secret / delete secret |
| | `y` | Copy the path |
| Secret | `Space` / `v` | Reveal the selected value / every value |
| | `Enter` | Show the whole value |
| | `c` | Copy the selected value |
| | `e` | Edit |
| | `l` | Version history |
| | `Esc` | Back to the list |
| Version history | `Enter` | Show the selected version |
| | `n` | Edit a copy of it; saving makes it the current version |
| Editor | `Enter` | Edit the value; in a field, done |
| | `Alt+Enter`, `Ctrl+J` | Line break in a value |
| | `Tab` | In a field: switch between key and value |
| | `r` / `a` / `d` | Rename key / add field / delete field |
| | `Space` / `v` | Reveal the selected value / every value |
| | `Ctrl+S` | Save the secret |
| | `Esc` | Discard the field being typed, or close the editor (asks if there are unsaved changes) |

Moving the cursor onto a secret reads it from Vault once the cursor rests
there, so scrolling through a folder does not read every secret on the way.

## Security Notes

### The token in memory

VltView holds the Vault token in one place while it runs and overwrites that
memory when it exits, also when the start fails. The token is never written
to the application log or shown in debug output.

That is a limit on how many long-lived copies there are, not a promise that
the token cannot be found in the memory of the process:

- The HTTP library copies the token into the headers of every request it
  sends, and the TLS layer below it copies the request again. Those copies
  are freed after the request and are not overwritten.
- Wherever the token came from still has it. `VAULT_TOKEN` stays in the
  environment of the process and `--vault-token` in its argument list for as
  long as it runs (the argument list is also visible to other users in
  `ps`). The text of the config file and of `~/.vault-token` is overwritten
  after reading, but the argument and YAML parsers make short-lived copies
  of their own that are not.
- Nothing is overwritten if the process is killed by a signal, and memory
  that the system swapped out or wrote to a core dump is out of reach.

## Development

### Toolchain

Tool versions (Rust, rustfmt, clippy, cargo-audit) are pinned in `mise.toml`
and used by CI and the release workflow.

```bash
mise install     # install the pinned toolchain
mise run ci      # format check, clippy, tests, dependency audit
```

### Run Tests

```bash
cargo test
```

### Run with Debug Logging

```bash
vltview --debug
```

### Docker Development Environment

Needs Docker with Compose and `make`. From a fresh clone:

```bash
make docker-up          # start a dev Vault and fill it with sample data
make run-with-docker    # the same, then run vltview in the sample folder /myapp
make docker-down        # stop and remove the containers
```

`make docker-up` starts Vault in dev mode at `http://localhost:8200` (token
`root`) and then runs `vault-init.sh` in a second container, which writes the
sample secrets (`myapp`, `prod`, `staging`, `test`, `dev`, `company`,
`services`) and five policies. The target waits for that step and fails if it
fails; `make docker-logs-init` shows its output.

The dev server keeps everything in memory. `make docker-down` discards the
data, and the next `make docker-up` writes the sample data again. While Vault
is running, `make docker-up` leaves what is in it alone.

To run against it yourself:

```bash
export VAULT_ADDR=http://localhost:8200
export VAULT_TOKEN=root
cargo run -- /myapp
```

The path is relative to the KV mount `secret/`, which vltview adds itself:
`/myapp` is `secret/myapp` in Vault.

`./docker-build.sh` builds a release binary inside Ubuntu 20.04
(`Dockerfile.compat`) for machines with an older glibc and writes it to
`target/vltview`.

## License

MIT

## Contributing

Contributions welcome! Please open an issue or PR.
