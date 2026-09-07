# herdr-plugin-space-colors

Peacock-style per-workspace colours for [Herdr](https://herdr.dev). The theme
follows the focused workspace, so you can tell at a glance which project you're
in — the same idea as the VS Code Peacock extension, for your agent terminals.

Each workspace gets a palette: an accent plus tinted sidebar and active-row
surfaces. Explicit rules pin a colour to a project; everything else is coloured
automatically from its directory, so the same project gets the same colour on
every machine with no setup.

## How it works

Herdr's theme is global to a session, and its socket API has no per-workspace
colour call. But exactly one workspace is focused at a time. On every
`workspace.focused` event the plugin writes that workspace's palette into the
`[theme.custom]` table of herdr's `config.toml` and runs
`herdr server reload-config`, which herdr applies live — no restart. A global
theme that follows focus reads as per-workspace colour.

That means the plugin edits a file you maintain by hand. See [Safety](#safety)
for exactly what it will and won't touch.

## Install

```bash
herdr plugin install FerreTorresJosep/herdr-plugin-space-colors
```

The install step compiles a small Rust binary once (`cargo` must be on your
PATH — [rustup.rs](https://rustup.rs)). After that there are no runtime
dependencies. Switch workspaces and the colours follow.

To pin a release: `herdr plugin install FerreTorresJosep/herdr-plugin-space-colors --ref v0.1.0`.

For local development:

```bash
git clone https://github.com/FerreTorresJosep/herdr-plugin-space-colors
herdr plugin link ./herdr-plugin-space-colors
```

## Configuration

The plugin seeds its config on first run. Find it with:

```bash
herdr plugin config-dir ferretorres.space-colors
```

```toml
# Colour workspaces that match no rule by hashing their directory onto the
# palettes. Same project → same colour, everywhere.
auto = true

# Pin a colour. Match by exact workspace `label`, or by `path` — a directory
# prefix, so every worktree of a project shares its colour (longest path wins).
[[workspaces]]
path = "~/projects/api"
palette = "blue"

[[workspaces]]
label = "Website"
palette = "green"

# Palettes are sets of theme.custom tokens. Values must be #rgb or #rrggbb.
[palettes.blue]
accent = "#89b4fa"
sidebar_bg = "#1f2535"
active_row_bg = "#283248"
```

Eight palettes tuned for the default catppuccin theme ship in
[`config.example.toml`](./config.example.toml): red, peach, yellow, green,
teal, blue, mauve, pink. Edit them freely or add your own.

Allowed tokens (herdr 0.8.2): `accent`, `panel_bg`, `sidebar_bg`,
`active_row_bg`, `selection_bg`, `surface0`, `surface1`, `surface_dim`,
`overlay0`, `overlay1`, `text`, `subtext0`, `mauve`, `green`, `yellow`, `red`,
`blue`, `teal`, `peach`. Anything else is refused before it reaches your config.

`herdr_config = "..."` overrides the path to herdr's `config.toml`;
`$HERDR_CONFIG_PATH` always wins over both.

## Commands

Registered as plugin actions:

```bash
herdr plugin action invoke ferretorres.space-colors.apply    # colour the focused workspace
herdr plugin action invoke ferretorres.space-colors.clear    # remove every key the plugin wrote
herdr plugin action invoke ferretorres.space-colors.status   # what each workspace gets, and why
```

Or run the binary directly from the plugin directory for `--dry-run` and
`validate`:

```bash
sh bin/herdr-space-colors status
sh bin/herdr-space-colors apply --dry-run     # prints the diff, writes nothing
sh bin/herdr-space-colors clear --dry-run
sh bin/herdr-space-colors validate            # checks the plugin config and exits
```

`apply` also runs as a startup hook, so the colour is right after a server
restart before the first switch.

## Safety

This plugin writes to your herdr `config.toml`. The contract:

- **Format-preserving edits.** Uses `toml_edit`; your comments, key order and
  formatting survive. Nothing is re-emitted from a parsed model.
- **Only its own keys.** It writes the tokens named in the active palette under
  `[theme.custom]`, and only ever removes keys it wrote. Your own
  `theme.custom` entries and every other line are untouched.
- **Strict validation first.** Tokens must be on the allowlist and colours must
  be `#rgb`/`#rrggbb`. `herdr config check` accepts malformed colours, so the
  plugin does this check itself.
- **One-time backup.** Before its first edit it copies the original to
  `config.toml.space-colors.bak` beside it, and never overwrites that file.
- **Verify, then roll back.** After every write it runs `herdr config check`;
  if that fails, the previous bytes are restored before anything else happens.
- **Atomic writes.** Temp file plus rename, so a crash mid-write cannot leave a
  half-written config.
- **`clear` gets you back.** Removes exactly the managed keys; if the plugin
  created `[theme.custom]` and `[theme]`, it removes those too, leaving the file
  byte-identical to before.

## Uninstall

```bash
herdr plugin action invoke ferretorres.space-colors.clear
herdr plugin uninstall ferretorres.space-colors
```

Run `clear` first so the theme returns to your base config. The backup file is
left in place for you to delete.

## Limitations

- Colours apply to herdr's own chrome — sidebar, panels, accent — not to what
  runs inside panes.
- One theme at a time: the focused workspace decides. Unfocused workspaces in
  the sidebar are not individually coloured; that needs a change in herdr
  itself.
- On first write, `[theme.custom]` is appended at the end of `config.toml`.
- Built and tested on macOS with herdr 0.8.2. Linux is declared and expected
  to work; reports welcome.

## Requirements

- Herdr ≥ 0.8.2 (`reload-config` applies the theme section live from this
  version; earlier versions untested)
- `cargo` at install time

## License

MIT — see [LICENSE](./LICENSE).
