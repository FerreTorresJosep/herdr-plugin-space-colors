#!/bin/sh
# End-to-end run against a COPY of the live herdr config. Needs a running herdr
# server for the read-only workspace/pane lookups; `reload-config` reloads the
# live server's own (unchanged) config, which is harmless.
set -eu
root=$(cd "$(dirname "$0")/.." && pwd)
bin="$root/target/release/herdr-space-colors"
[ -x "$bin" ] || cargo build --release --quiet --manifest-path "$root/Cargo.toml"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
# A synthetic herdr config: deterministic, and free of anything this plugin
# may already have written to the live one (which would break the identity check).
cat > "$tmp/config.toml" <<'TOML'
# e2e fixture
onboarding = false

[session]
resume_agents_on_restore = true   # keep

[ui]
agent_panel_sort = "priority"

[[keys.command]]
key = "prefix+space"
type = "plugin_action"
command = "herdr-layout-cycle.cycle-layout"
TOML
cp "$tmp/config.toml" "$tmp/orig.toml"
export HERDR_CONFIG_PATH="$tmp/config.toml" HERDR_PLUGIN_CONFIG_DIR="$tmp/cfg" HERDR_PLUGIN_STATE_DIR="$tmp/state"
# The fixture must not recolour the developer's real terminal window.
mkdir -p "$tmp/cfg"
sed 's/^tint = true$/tint = false/' "$root/config.example.toml" > "$tmp/cfg/config.toml"

step() { printf '\n== %s\n' "$*"; }
step validate;           "$bin" validate >/dev/null
step status;             "$bin" status >/dev/null
step "dry-run apply";    "$bin" apply --dry-run >/dev/null
step apply;              "$bin" apply >/dev/null
grep -q '^\[theme.custom\]' "$tmp/config.toml"
grep -q '^\[ui.sidebar.agents\]' "$tmp/config.toml"
grep -q '^\[ui.sidebar.spaces\]' "$tmp/config.toml"
herdr config check >/dev/null
[ -f "$tmp/config.toml.space-colors.bak" ]
step "second apply is a no-op"; "$bin" apply | grep -q "already in place"
step clear;              "$bin" clear >/dev/null
cmp "$tmp/orig.toml" "$tmp/config.toml"
printf '\nOK: apply/clear round-trip left the config byte-identical\n'
