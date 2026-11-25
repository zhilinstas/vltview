# VltView v0.3.0 release plan

Written 2026-11-25, after a full review of v0.2.4 and a toolchain/dependency
modernization. A minor bump fits: new minimum Rust, a TLS trust-store change,
and behaviour fixes.

"Reproduced" means observed against a live dev Vault. "From code" means found
by reading and not yet triggered.

## Step 1: land the modernization

Status: done locally, not committed, GitHub workflows not yet run.

- [x] `mise.toml` pins Rust 1.100.0 (rustfmt, clippy) and cargo-audit; CI and
      the release workflow install from it via `jdx/mise-action@v5`
- [x] Edition 2024, `rust-version = "1.100"`
- [x] All dependencies at latest; `cargo audit` 22 vulnerabilities to 0
- [x] `mise run ci` green locally (fmt, clippy `-D warnings`, 52 tests, audit)
- [x] Release build verified for glibc and static musl
- [ ] Commit as its own PR, separate from the fixes below
- [ ] Confirm CI is green on GitHub for Ubuntu and macOS (first real run of
      the mise-based workflows; macOS builds are unverified)

Things to know about the upgrade:

- reqwest 0.13 verifies certificates against the OS trust store instead of
  bundled Mozilla roots, so private CAs installed on the machine are trusted.
- tui-textarea was replaced by ratatui-textarea (the ratatui org's fork),
  serde_yaml by serde_norway, chrono by jiff, dirs and once_cell by std.
- The clippy gate passes `-A dead_code` for now (see step 4).

## Step 2: release blockers

Status: all nine fixed locally, not committed. Each has a regression test
(39 new tests, 91 total). The tests were confirmed to fail with the fix
reverted, except B3, whose test uses the input that crashed the app live.
B1 to B6, B8 and the save path of B9 were also re-tested against a live dev
Vault. B7 was tested only with a fake clipboard, to avoid overwriting the real
one.

- [x] **B1 Saving deletes non-string fields** (reproduced). Opening the editor
      on `{"host":"db","port":5432,"tls":true}` and saving unchanged wrote back
      only `host`. Fix: values are `serde_json::Value` end to end
      (`vault::SecretData`). The editor shows non-strings as JSON and saves
      them with their type.
- [x] **B2 Failed writes show "Saved"** (from code). `KvEngine::put` never
      checked the HTTP status. Fix: every KV response goes through
      `ensure_success`; a rejected write is an error and the edit buffer stays
      open.
- [x] **B3 Crash on non-ASCII values** (reproduced). `mask_value` sliced by
      byte. Fix: it works in characters.
- [x] **B4 Typing in the filter fires global shortcuts** (reproduced). Fix:
      while the filter is open, the navigation pane gets every key.
- [x] **B5 `--help` prints the token** (reproduced). Fix: `hide_env_values`.
- [x] **B6 Path sanitizer is bypassable** (reproduced). Fix: URLs are built
      with `sanitize::vault_url`, which percent-encodes each segment. The
      sanitizer now drops only whole `.` and `..` segments, so names such as
      `v1..2` are no longer mangled.
- [x] **B7 Clipboard auto-clear is lost on quit** (from code). Fix: the
      manager clears on drop if the clipboard still holds what it copied.
- [x] **B8 Errors look like empty folders** (reproduced). Fix: `list` returns
      an empty list only for 404; other failures are errors.
- [x] **B9 Stale async results overwrite the current view** (from code). Fix:
      only the latest requested secret is shown, a loaded secret never closes
      an edit, the edit closes on a confirmed save, and version results carry
      their path.

Behaviour changes to put in the changelog:

- Starting at a path the token cannot list now exits with
  `Failed to list initial path` instead of showing an empty list.
- Non-string values appear in the detail and edit views as JSON (`5432`,
  `true`, `["a","b"]`). Editing one into invalid JSON is refused; putting the
  text in double quotes saves it as a string.
- Previewing another secret no longer closes an open editor.

## Step 3: robustness

- [ ] Terminal is left in raw mode if startup fails; build `App` before
      touching the terminal, or use a guard
- [ ] No panic hook: panic messages are lost in the alternate screen
- [ ] `--insecure` warning is printed after the alternate screen is entered,
      so it is never visible
- [ ] Bracketed paste is off: pasted text runs as commands (`d` then `y`
      confirms a permanent delete)
- [ ] History back/forward ping-pongs between two paths; forward never works
- [ ] Refresh/navigate race can leave one directory's items under another path
- [ ] Edit view accepts newline, empty and duplicate keys (Ctrl+S on a new
      secret with an unnamed field still sends `{"": ""}`)
- [ ] Unsaved edits are discarded without confirmation (`has_changes` is never
      read)
- [ ] Policy screen: errors land on the main screen, viewport height is
      hard-coded to 30, the list does not scroll, `y` (copy) is a no-op
- [ ] Log screen: first scroll jumps to the top; same hard-coded viewport
- [ ] Detail view cannot be closed (`ScreenBack` is never forwarded)
- [ ] Long field/version lists have no scrolling
- [ ] Masking shows the first and last character and the exact length; the
      edit view shows every value in clear
- [ ] False "token provided via command-line argument" warning when the token
      comes from `VAULT_TOKEN`
- [ ] Audit log entries drop their structured fields (no path, no action), and
      "Authenticated to Vault successfully" is logged before any request
- [ ] A malformed `~/.vltview.yaml` is silently ignored; a config file must
      contain both `vault_addr` and `vault_token` to parse at all
- [ ] Token `Zeroize` is nominal: the token is cloned into three
      `AuthManager`s and stays in `Config`

## Step 4: cleanup

- [ ] Wire up or delete the 20 dead-code items, then drop `-A dead_code` from
      the clippy task in `mise.toml`
  - Emitted but never handled: `SecretSaved`, `SecretDeleted`, `CopyPolicy`,
    `SelectNext`, `SelectPrevious`, `ScreenBack` from the detail view
  - Handled but never emitted: `ShowMainScreen`, `HideCreateSecretInput`,
    `StartEditKey`, `ClearStatus`, `ClearError`, the page/top/bottom scroll
    directions
  - Never constructed: `Tick`, `Render`, `Resize`, `Suspend`, `Resume`,
    `SelectConfirm`, `FocusNext`, `FocusPrevious`, `ConfirmationAccepted`,
    `ConfirmationRejected`, `ClearFilter`, `CopyComplete`, `TextInput`, `Noop`
  - Scaffolding: `FocusState`, the `CacheManager` stub, `is_loading`,
    `OperationId`, unused `VaultClient` getters, `redact_secret_value`
- [ ] Reconcile CLAUDE.md and README with the code: KV v1 support, caching,
      audit logging, `SecretService`/`NavigationController` responsibilities,
      help overlay bindings (`h`, `c`)
- [ ] README documents `insecure_skip_verify`; the real config key is
      `insecure`
- [ ] Dev environment: the docker-compose seed step fails (`vault-init.sh`
      needs bash, the Vault image has none); `docker-build.sh` points at
      `/tmp/Dockerfile.compat`; `make run-with-docker` passes `/secret/myapp`,
      which resolves to `secret/secret/myapp`
- [ ] Tests: extend coverage of key routing and the HTTP client (step 2
      added the first of both); nothing covers the policy client yet

## Step 5: release engineering

- [ ] Replace the archived `actions/create-release@v1` and
      `actions/upload-release-asset@v1` with `gh release`
- [ ] Pin GitHub Actions by commit SHA
- [ ] Tag `v0.3.0-rc.1` to exercise the release workflow (musl and both macOS
      targets) before the final tag
- [ ] Bump the version, write the changelog, tag `v0.3.0`

## Open decision

Configurable mounts and KV v1 support. The mount is hard-coded to `secret/`
and only KV v2 works, though the docs claim v1 support. Recommendation: defer
both past 0.3.0 and correct the docs instead.
