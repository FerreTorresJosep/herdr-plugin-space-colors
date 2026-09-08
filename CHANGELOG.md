# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [0.3.2] - 2026-09-08

### Added

- Prebuilt release binaries for macOS (arm64, x86_64) and Linux (x86_64,
  arm64). When `cargo` is absent the shim downloads the binary matching the
  host and the manifest version and verifies its SHA-256 before use, so
  installing the plugin no longer requires a Rust toolchain. With `cargo`
  present the plugin still builds from source, so `herdr plugin link` on a
  checkout reflects local edits.

## [0.3.1] - 2026-09-07

### Fixed

- Window tint is scoped to the herdr session the hook runs under. With two
  sessions open (for example `herdr` and `herdr --session demo`), a focus
  change in one no longer recolours the other session's window. The session
  of each tinted window is recorded in state so stale-window cleanup only
  touches the caller's own windows.

## [0.3.0] - 2026-09-07

### Added

- Window tint: the terminal window hosting the herdr client, title bar
  included, takes the focused workspace's colour. Apple Terminal via
  AppleScript on the tab whose tty runs the client; the first colour seen is
  saved in state and restored on `clear`, on a switch to the base theme, or
  when the client disappears. `[window] tint` toggles it; palettes accept an
  optional `window_bg` (defaults to `pane_bg`, then `sidebar_bg`).
- `status` reports the tinted windows.

## [0.2.0] - 2026-09-07

### Added

- Sidebar colouring: one coloured `$sc_<palette>` token per palette in
  `ui.sidebar.agents.rows` / `ui.sidebar.spaces.rows` (written only when the
  user has not set their own rows), with exactly the matching token reported
  on every pane and Space. `sweep` refreshes tags; it also runs on
  `workspace.created` and `pane.agent_status_changed`.
- Per-agent colour: `[[agents]]` rules keyed by agent session id;
  `set-agent` / `unset-agent` commands.
- Pane tint: `osc` prints an OSC 11 sequence for the calling pane; `shell-hook`
  prints or installs a guarded `~/.zshrc` hook; `install-cli` symlinks the
  tool into `~/.local/bin`.
- Commands with arguments: `set`, `unset`, `focus`, `palettes`.
- Light-theme support: `[palettes.<name>.light]` variants, chosen from
  `theme.name`; shipped palettes carry catppuccin-latte values.
- A write lock in the state dir, and a toast when a write is rolled back or a
  reload fails.

### Changed

- `status` lists every pane with its agent, workspace, palette and why.

## [0.1.1] - 2026-09-07

### Fixed

- `apply` always reconciles against `config.toml` instead of trusting cached
  state, so a `[theme.custom]` block removed by hand or by another tool is
  restored on the next event rather than after the next workspace switch.

## [0.1.0] - 2026-09-07

### Added

- Theme follows the focused workspace via a `workspace.focused` hook and
  `herdr server reload-config`.
- Explicit rules by workspace `label` or directory `path` (prefix, longest
  wins); automatic palette by directory hash for everything else.
- Eight catppuccin-tuned palettes.
- `apply`, `clear`, `status` plugin actions; `validate` and `--dry-run` from
  the CLI; `apply` as a startup hook.
- Safety contract: format-preserving edits, token allowlist and strict hex
  validation, one-time backup, `herdr config check` with rollback, atomic
  writes, `clear` restores the original bytes.
