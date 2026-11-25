# Changelog

## 0.3.0

A release of fixes and of changes to how the screen works. Read "Changed
behaviour" before upgrading: several keys do something different from 0.2.4.

### Before you upgrade

- **Building needs Rust 1.100 or newer.** The toolchain is pinned in
  `mise.toml`.
- **Certificates are verified against the operating system's trust store**,
  no longer against a bundled copy of Mozilla's roots. A private CA installed
  on the machine is now trusted. A setup that relied on the bundled roots,
  with no system certificates installed, will fail to connect.
- **A config file with a mistake stops the start.** `~/.vltview.yaml` that is
  not valid YAML, or that has a key vltview does not know, is an error naming
  the file and the problem; before, the whole file was silently ignored. The
  setting that skips TLS verification is `insecure` (older versions of the
  README called it `insecure_skip_verify`; rename or delete that key).
- **Starting at a path the token cannot list is an error**
  (`Failed to list initial path`), not an empty list.

### Fixed: data loss and wrong data

- Saving a secret kept only its string fields. Opening the editor on
  `{"host":"db","port":5432,"tls":true}` and saving wrote back only `host`.
  Values now keep their JSON type from load to save.
- A write that Vault rejected was reported as "Saved". Every response is now
  checked.
- Creating a secret under a name that existed wrote a new version over it.
  A create is now refused by Vault itself if anything exists at the path.
- A refresh or a navigation that finished late could put one folder's items
  under another folder's path, where Enter or `d` acted on the wrong secret.
- A secret, a version or a policy whose answer arrived after you had moved on
  could replace what you were looking at.
- A deleted secret stayed on screen, where it could be edited and saved back.
- The editor sent a field with no key under the empty name, and of two
  fields with the same key one silently replaced the other. Both are refused
  with a message.
- Unsaved edits were dropped by a single Esc, or by `q` from the list.
- What was typed after Ctrl+S, while the save was on its way, was dropped
  when the save completed. The editor now takes no changes until the answer
  is there.
- A read of a secret that was started before a save could arrive after the
  read that followed the save, and put the old content back on screen.

### Fixed: security

- `--help` printed the token when `VAULT_TOKEN` was set.
- A masked value showed its first and last character and had one dot per
  character. Every masked value is now the same eight dots. The editor did
  not mask at all; it now masks its list like the secret view.
- Path sanitization could be bypassed with encoded characters. Request URLs
  are now built one encoded segment at a time.
- Text pasted into the terminal ran as commands (`d`, `y` in a paste deleted
  a secret). A paste now goes into the text input that has focus and is
  dropped anywhere else.
- The clipboard was not cleared if you quit before the 30 seconds were up.
- With `--insecure`, the warning was printed where it could not be seen. It
  is now printed before the UI starts and stays in the header.
- The token was copied into several places and stayed in the configuration.
  It now has one owner that overwrites it on exit. The README says what this
  does and does not cover.
- The session log named operations without saying which secret, and said
  "Authenticated" before any request had been made. Each entry now has its
  action and path, never a value, and "authenticated" is logged after the
  first request Vault accepts.

### Fixed: other

- A value with a non-ASCII character crashed the app when it was masked.
- A permission error on a folder looked like an empty folder.
- Typing `q`, `p` or `r` into the filter quit, opened policies or refreshed.
- History back went back and forth between two folders; forward did nothing.
- A failed start left the terminal in raw mode, and a panic lost its message.
- "Token provided via command-line argument" was printed when the token came
  from `VAULT_TOKEN`.
- A config file had to contain both the address and the token to be read at
  all. It may now hold any subset of the keys.
- Long warnings and errors were cut off at the edge of a pane.
- The Docker dev environment did not seed its sample data.

### Changed behaviour

- **The screen is laid out differently**: one header line with the Vault
  address, one footer line with the keys that work right now, and the secret
  in a single pane. It is designed to be usable at 80x24.
- **The secret pane always shows what the cursor is on**, whatever moved the
  cursor (keys, the filter, a reload, opening a folder), and it never keeps
  the previous secret on screen while the next one loads. The secret is read
  from Vault once the cursor rests on it. 0.2.4 read on every cursor step,
  so scrolling through a folder now puts far fewer reads into Vault's audit
  log.
- **Enter** on a secret moves into it. In the editor, Enter in a field
  finishes the field; a line break in a value is Alt+Enter or Ctrl+J.
- **Ctrl+S** saves the secret from anywhere in the editor, in one step.
- **Esc** in the secret pane returns to the list. In the editor it asks
  before closing if there are unsaved changes, and so does `q`. Ctrl+C still
  quits at once.
- **Deleting** a secret, or a field, is confirmed with `y` only; Enter no
  longer confirms. The dialog says how many versions will be destroyed.
- `q`, `p` and `~` do nothing while the editor has focus, and `r` there
  renames the selected key instead of refreshing the list.
- Non-string values are shown and edited as JSON (`5432`, `true`,
  `["a","b"]`). Editing one into invalid JSON is refused; put the text in
  double quotes to save it as a string.
- Previewing another secret no longer closes an open editor.
- Errors are shown where they belong: a failed save inside the editor, a
  failed load in the secret's pane, anything else in a message over the
  screen that names what failed.
- The log screen has one line per operation with its path, and no module
  column.

### New

- Value viewer: Enter on a field shows the whole value, wrapped, with
  scrolling.
- Reveal one value with Space, all with `v`, in the secret view and in the
  editor.
- PgUp/PgDn and `g`/`G` in every list. The navigation list shows where the
  cursor is (`3/12`) and lists folders first.
- The help overlay lists every key and scrolls.
- A "Connecting" screen while the first folder is listed.

### For contributors

- Tool versions are pinned in `mise.toml`; `mise run ci` is what CI runs
  (format, clippy with warnings as errors and no exemptions, tests, audit).
- All dependencies were updated; `cargo audit` went from 22 advisories to 0.
- The release workflow creates the release with the `gh` CLI, and every
  GitHub Action is pinned by commit.
- The suite went from 52 tests to more than 260, including one that presses
  every key shown in a footer or in the help.
