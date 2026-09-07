# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

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
