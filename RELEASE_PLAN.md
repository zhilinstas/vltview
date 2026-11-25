# VltView v0.3.0 release plan

Written 2026-11-25, after a full review of v0.2.4 and a toolchain/dependency
modernization. A minor bump fits: new minimum Rust, a TLS trust-store change,
and behaviour fixes.

"Reproduced" means observed against a live dev Vault. "From code" means found
by reading and not yet triggered.

## Step 1: land the modernization

Status: committed on the branch `v0.3.0-groundwork`, the first of three
commits in one pull request.

- [x] `mise.toml` pins Rust 1.100.0 (rustfmt, clippy) and cargo-audit; CI and
      the release workflow install from it via `jdx/mise-action@v5`
- [x] Edition 2024, `rust-version = "1.100"`
- [x] All dependencies at latest; `cargo audit` 22 vulnerabilities to 0
- [x] `mise run ci` green locally (fmt, clippy `-D warnings`, 52 tests, audit)
- [x] Release build verified for glibc and static musl
- [x] Commit separately from the fixes below. Decided: one pull request with
      one commit per step (modernization, blockers, UX) instead of one pull
      request per step. Each commit passes `mise run ci` on its own
- [x] CI is green on GitHub for Ubuntu and macOS: the first run of the
      mise-based workflows passed test, build and audit on pull request #1
      with no changes needed. The release workflow (musl and the macOS
      targets) is still unverified; see step 6

Things to know about the upgrade:

- reqwest 0.13 verifies certificates against the OS trust store instead of
  bundled Mozilla roots, so private CAs installed on the machine are trusted.
- tui-textarea was replaced by ratatui-textarea (the ratatui org's fork),
  serde_yaml by serde_norway, chrono by jiff, dirs and once_cell by std.
- The clippy gate passes `-A dead_code` for now (see step 5).

## Step 2: release blockers

Status: all nine fixed and committed (second commit of the pull request).
Each has a regression test
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
- [x] Bracketed paste is off: pasted text runs as commands (`d` then `y`
      confirms a permanent delete). Fixed with step 4: a paste goes into the
      text input that has focus and is dropped anywhere else
- [x] History back/forward ping-pongs between two paths; forward never works
- [x] Refresh/navigate race can leave one directory's items under another path
- [x] Edit view accepts empty and duplicate keys (Ctrl+S on a new secret
      with an unnamed field still sends `{"": ""}`). Newlines in keys were
      fixed with step 4. Fixed: a value without a key and two fields with
      one key are refused in the editor, an empty row is left out, and a
      new secret with no named field is not sent
- [x] Unsaved edits are discarded without confirmation (`has_changes` is never
      read). Fixed: Esc in the editor, and `q` with an edit open, ask first
      when there are unsaved changes; Ctrl+C still quits at once
- [x] Policy screen: errors land on the main screen, viewport height is
      hard-coded to 30, the list does not scroll, `y` (copy) is a no-op.
      Fixed with step 4; list scrolling has no test
- [x] Log screen: first scroll jumps to the top; same hard-coded viewport.
      Fixed with step 4
- [x] Detail view cannot be closed (`ScreenBack` is never forwarded).
      Settled with step 4: Esc returns to the list, and the pane shows
      whatever the cursor is on
- [x] Long field/version lists have no scrolling. Fixed with step 4
- [x] Masking shows the first and last character and the exact length; the
      edit view shows every value in clear. Fixed: every masked value is the
      same eight dots, and the editor masks its list too, with the same
      reveal keys (Space, `v`)
- [ ] False "token provided via command-line argument" warning when the token
      comes from `VAULT_TOKEN`
- [ ] Audit log entries drop their structured fields (no path, no action), and
      "Authenticated to Vault successfully" is logged before any request
- [ ] A malformed `~/.vltview.yaml` is silently ignored; a config file must
      contain both `vault_addr` and `vault_token` to parse at all
- [ ] Token `Zeroize` is nominal: the token is cloned into three
      `AuthManager`s and stays in `Config`

### From the review of the pull request

The three commits were reviewed against CLAUDE.md and against this plan
before the pull request was opened.

Fixed in the pull request (fourth commit), each with a test:

- [x] The error from a failed load stayed in the pane after the cursor moved
      on, over folders and in place of the next secret's "Loading"
- [x] Starting a new secret threw away the secret under the cursor: after
      Esc the pane was empty with the cursor still on it, and Enter waited
      for a load that was never requested
- [x] Enter confirmed deleting a field although the dialog offered only `y`,
      and the footer went on showing the editor's keys while it was open
- [x] The policy list URL was still built with `format!`

Open:

- [x] A late policy result is applied whatever the user has moved to, and
      opens the policy even after Esc
- [x] Version results are checked by path but not by version number, so two
      requests for one secret resolve to whichever arrives last (from code)
- [x] Errors do not say what they are for: an error lands on whichever screen
      or editor is open when it arrives
- [x] After a save the saved secret is loaded and shown even if the cursor
      has left it
- [x] Creating `a/b` when `a/b` exists still writes a new version: the name
      check only looks at the folder on screen (from code). Fixed: a create
      is written with check-and-set 0, so Vault refuses it; tried against the
      dev Vault with `myapp/api`, which stayed at version 1
- [ ] Keys that work but are not in the key table: the help overlay's own
      keys, `q` on the connecting screen, Esc to dismiss an error, Home/End
      in the list, and PgUp/PgDn/g/G, `?` and `h` in the editor
- [x] Warnings can still be cut: the restore note beyond two lines, the name
      prompt's error, and policy errors, which are not wrapped at all.
      Fixed: each is wrapped at spaces and shown whole, or replaced by a
      shorter wording where there is no room
- [ ] CLAUDE.md asks for `ListState` and for no nested boxes; the navigation
      pane and the log screen scroll by hand and the editor draws a box
      inside the pane. Follow the rule or say what it really requires
- [ ] Duplication to remove with step 5: the policy client repeats the
      request code that `kv.rs` extracted; the edit view collects its fields
      in two places and `save` uses the one that keeps empty keys; the TUI
      calls the Vault client directly in two places instead of going through
      `SecretService`

## Step 4: UX

Status: U1 to U19 done and committed (third commit of the pull request).
The suite went from 91 to 164 tests, and `mise run ci` is green. Also driven in a real 80x24 terminal
(tmux) against the dev Vault: preview, filter, value viewer, history, create,
edit, save, restore from an old version, delete, policies, logs and the
startup screen. A burst of ten cursor steps made 2 reads (before, from code:
one per step). Not checked live: KV v1, a namespace in the header, and a
secret under the cursor at startup (the dev Vault's root holds only folders).

The findings were from code (a read of `src/tui`); row and column counts
were computed from the layout constraints.

Changes to put in the changelog:

- The screen is laid out differently: one header line and one footer line,
  and the secret is a single block. The screenshots in the README are out of
  date.
- Enter in a field now finishes the field; a line break in a value is
  Alt+Enter or Ctrl+J. Ctrl+S saves the secret from anywhere in the editor.
- Deleting a secret is confirmed with `y` only.
- Esc in the secret pane returns to the list; the pane always shows what the
  cursor is on.
- `q`, `p` and `~` do nothing while the editor has focus, and `r` renames
  the selected key instead of refreshing the list.

Fixed on the way, because the same code was being rewritten (ticked in
step 3): bracketed paste, list scrolling, the policy and log screen items,
and closing the detail view.

### Selection and preview (0.3.0)

Decision: keep preview-on-navigation. The cursor is the selection and the
content pane shows what is selected; Enter is not needed to see a secret.
Today this is half implemented, which is what the items below fix.

- [x] **U1 Preview follows the selection, not the keypress.** Preview fires
      only on `j`/`k` (`navigation_pane.rs:311`). Fire it on every selection
      change: entering a folder, refresh, save, delete, filter. Clear the pane
      when the cursor is on a folder or `..`.
- [x] **U2 Keep the selection across reloads.** Every reload resets the cursor
      to row 0 (`navigation_pane.rs:395`) while the pane keeps the old secret,
      so the two can name different secrets. Reselect by name; after going up,
      land on the folder just left; after a delete, land on the neighbour.
- [x] **U3 Debounce preview by about 200ms.** Each cursor step makes two
      requests (data and metadata) with no debounce, so holding `j` reads
      every secret in the folder and puts each one in Vault's audit log.
- [x] **U4 Enter goes into the secret.** It only moves focus today
      (`navigation_pane.rs:163`). It should move focus and, if the debounced
      load has not fired yet, load at once. The empty state says "Press Enter
      on a secret", which currently loads nothing.
- [x] **U5 Clear the pane on delete.** The pane keeps showing the deleted
      secret, which can then be edited and saved back (`SecretDeleted` is
      unhandled, see step 5).

Not planned: an `auto_preview: false` config option for environments where
every read of a production secret is reviewed. Add it only on request; the
pane would show "Press Enter to load" in that mode.

### Destructive actions and feedback (0.3.0)

- [x] **U6 Delete dialog.** Delete destroys all versions and metadata
      (`kv.rs:151`) and Enter confirms it. The dialog is 30% of the screen
      height (`main_screen.rs:383`), so at 80x24 the path and the warning do
      not fit. Use a fixed-size dialog, confirm with `y` only, and show the
      version count.
- [x] **U7 Loading indicator.** `is_loading` is set and never drawn, so the
      previous secret stays on screen while the next one loads (the request
      timeout is 30s). The policy screen shows "No policies found" while
      loading.
- [x] **U8 Startup feedback.** The screen is blank until the first list
      returns (`tui/mod.rs:74`). Draw a "Connecting to <addr>" frame first.

### Layout and orientation

- [x] **U9 Layout assumes a large terminal.** At 80x24, 15 rows are headers,
      footers and borders, leaving 7 rows for fields (with no scrolling, see
      step 3). Move headers into border titles and use one borderless footer
      line. The path is shown twice.
- [x] **U10 Footers and metadata are clipped at 80 columns.** The edit footer
      is about 70 characters in a 50-column pane, so "Ctrl+s: Save secret" is
      cut off; the detail footer loses "Values MASKED"; the metadata line
      loses "Modified".
- [x] **U11 Help overlay does not fit.** 30 lines in about 13 rows at 80x24,
      with no scrolling.
- [x] **U12 Focus is hard to see.** The detail and edit views ignore
      `is_focused`, and both panes show a cyan selection at once
      (`screenshots/2.png`). Only the `[NAV]`/`[CONTENT]` tag tells them
      apart.
- [x] **U13 Hints come from one keymap table.** The main footer always shows
      `n`/`d`, even when the content pane has focus and they do nothing. The
      help overlay omits Tab, `b`/`f` and every edit and version key (its
      wrong `h` and `c` entries are in step 5). Generate footers and help
      from the same table, per context.
- [x] **U14 Header shows no Vault address or namespace**, so nothing says
      which cluster is being edited.

### Flow friction

- [x] **U15 Filter.**
  - After typing, the cursor sits on `..`, so Enter, Enter goes up a folder;
    put it on the first match
  - The filter persists into subfolders; clear it on navigation
  - The hint says "/ to edit" but `/` clears the text
    (`navigation_pane.rs:363`)
  - "No secrets at this path" is shown when the filter hides everything
- [x] **U16 Reading values.**
  - Long and multi-line values cannot be read without entering edit mode;
    add a value viewer with wrap and scroll
  - The key column is fixed at 20 characters
  - Reveal is all-or-nothing and resets on every selection; add a per-field
    peek
- [x] **U17 Editing.**
  - Saving takes Ctrl+S twice (confirm field, then save secret)
  - `q`, `p` and `r` stay live in the edit view, so `q` quits with unsaved
    changes (related: the discard confirmation in step 3)
  - Creating a secret under an existing name silently writes a new version
  - The name prompt has no cursor movement
  - A validation error replaces the whole content pane, hiding the edit
- [x] **U18 Old versions.** The only cue is a missing "(current)" label, and
      `e` then save is a silent rollback. Show a "viewing v3 of 7" banner and
      say so before saving.
- [x] **U19 Navigation list.**
  - Folders and secrets look identical apart from a trailing `/`; list
    folders first and style them
  - No PgUp/PgDn, `g`/`G` or position indicator
  - Timestamps are UTC with no label

How it was done, where that is not obvious from the item:

- U3: the first step after a pause loads at once and later steps wait for
  the cursor to rest, so a single step is not slowed down. A selection moved
  by a reload or by the filter always waits.
- U7: "busy" is whether any request is in flight (`AsyncManager`), shown as a
  spinner in the header. A selected secret that has not arrived is a
  "Loading" placeholder, never the previous secret.
- U13: `src/tui/keymap.rs` holds the hints for every context. The key
  dispatch is still in each component's `handle_key`, so a changed binding
  has to be changed in both places.
- U16: masking itself was changed later, with step 3.
- U17: `q` no longer quits from inside the editor. The discard confirmation
  came later, with step 3.

### Feature candidates (not scheduled)

- Recursive search or a go-to-path prompt
- Version diff
- Copy the whole secret as JSON or env
- Soft delete (latest version only) with undelete
- Move and rename

## Step 5: cleanup

- [ ] Wire up or delete the remaining dead-code items, then drop
      `-A dead_code` from the clippy task in `mise.toml`. Step 4 dealt with
      the emitted-but-unhandled actions, the scroll directions and
      `is_loading`; what is left:
  - Handled but never emitted: `ShowMainScreen`, `HideCreateSecretInput`,
    `ClearStatus`, `ClearError`
  - Never constructed: `Tick`, `Render`, `Resize`, `Suspend`, `Resume`,
    `SelectConfirm`, `FocusNext`, `FocusPrevious`, `ConfirmationAccepted`,
    `ConfirmationRejected`, `CopyComplete`, `TextInput`, `Noop`
  - Scaffolding: `FocusState`, the `CacheManager` stub, `OperationId`,
    unused `VaultClient` getters, `redact_secret_value`,
    `DetailViewComponent::mode`, `EditViewComponent::get_data`
- [ ] Reconcile CLAUDE.md and README with the code: KV v1 support, caching,
      audit logging, `SecretService`/`NavigationController` responsibilities.
      The key bindings are done (step 4); the README screenshots need
      retaking
- [ ] README documents `insecure_skip_verify`; the real config key is
      `insecure`
- [ ] Dev environment: the docker-compose seed step fails (`vault-init.sh`
      needs bash, the Vault image has none); `docker-build.sh` points at
      `/tmp/Dockerfile.compat`; `make run-with-docker` passes `/secret/myapp`,
      which resolves to `secret/secret/myapp`
- [ ] Tests: extend coverage of key routing and the HTTP client (step 2
      added the first of both); nothing covers the policy client yet

## Step 6: release engineering

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
