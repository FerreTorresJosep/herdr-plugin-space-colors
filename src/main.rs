//! herdr-space-colors — Peacock-style per-workspace colours for Herdr.
//!
//! Herdr's theme is global to a session and its socket API has no per-workspace
//! colour method. Exactly one workspace is focused at a time, though, so a
//! global theme that follows focus reads as per-workspace colour. On every
//! `workspace.focused` event this binary rewrites only the `[theme.custom]`
//! keys it manages in herdr's config.toml and asks the running server to
//! reload, which applies the `theme` section live.
//!
//! Safety contract, because this edits a file the user hand-maintains:
//!   * format-preserving edits via toml_edit — comments and ordering survive
//!   * only tokens from the `theme.custom` allowlist, only strict hex colours
//!   * a one-time backup next to config.toml before the first write
//!   * `herdr config check` after every write; failure restores the old bytes
//!   * `clear` removes exactly the keys this plugin wrote and nothing else

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use serde_json::Value;
use toml_edit::{value, DocumentMut, Item};

const PLUGIN_ID: &str = "ferretorres.space-colors";
const TAG: &str = "space-colors";
const DEFAULT_CONFIG: &str = include_str!("../config.example.toml");
const BACKUP_SUFFIX: &str = ".space-colors.bak";

/// `theme.custom.*` tokens accepted by herdr 0.8.2. `herdr config check` does
/// not reject unknown tokens or malformed colours, so this list is the guard.
const THEME_TOKENS: &[&str] = &[
    "accent", "panel_bg", "sidebar_bg", "active_row_bg", "selection_bg",
    "surface0", "surface1", "surface_dim", "overlay0", "overlay1",
    "text", "subtext0", "mauve", "green", "yellow", "red", "blue", "teal", "peach",
];

type Res<T> = Result<T, String>;
type Palette = BTreeMap<String, String>;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let cmd = args.iter().find(|a| !a.starts_with("--")).map(String::as_str).unwrap_or("apply");
    let dry_run = args.iter().any(|a| a == "--dry-run");

    let result = match cmd {
        "apply" => cmd_apply(dry_run),
        "clear" => cmd_clear(dry_run),
        "status" => cmd_status(),
        "validate" => cmd_validate(),
        "help" | "-h" => { print!("{}", usage()); Ok(()) }
        other => Err(format!("unknown command `{other}`\n\n{}", usage())),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => { eprintln!("[{TAG}] {e}"); ExitCode::FAILURE }
    }
}

fn usage() -> String {
    format!(
"herdr-space-colors — the Herdr theme follows the focused workspace

Usage:
  herdr-space-colors [apply] [--dry-run]   colour the focused workspace (default)
  herdr-space-colors clear   [--dry-run]   remove every key this plugin wrote
  herdr-space-colors status                show which palette each workspace gets
  herdr-space-colors validate              check the plugin config and exit

Plugin config: $HERDR_PLUGIN_CONFIG_DIR/config.toml  (seeded on first run)
Herdr config:  $HERDR_CONFIG_PATH, else `herdr_config` in the plugin config,
               else ~/.config/herdr/config.toml
Backup:        <herdr config>{BACKUP_SUFFIX}, written once before the first edit
")
}

// --- environment ---------------------------------------------------------------

struct Ctx {
    herdr: String,
    config_dir: PathBuf,
    state_dir: PathBuf,
}

fn home() -> PathBuf {
    env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

fn ctx() -> Ctx {
    let herdr = env::var("HERDR_BIN_PATH").unwrap_or_else(|_| "herdr".into());
    // Outside a herdr-launched command (plain CLI use) the plugin dirs are not
    // injected; mirror where herdr keeps them so both paths agree.
    let plugins = home().join(".config/herdr/plugins");
    let config_dir = env::var_os("HERDR_PLUGIN_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| plugins.join("config").join(PLUGIN_ID));
    // herdr keeps plugin state under the XDG state dir, not beside the config.
    let state_root = env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".local/state"));
    let state_dir = env::var_os("HERDR_PLUGIN_STATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| state_root.join("herdr/plugins").join(PLUGIN_ID));
    Ctx { herdr, config_dir, state_dir }
}

fn herdr_config_path(cfg: &PluginConfig) -> PathBuf {
    if let Some(p) = env::var_os("HERDR_CONFIG_PATH") {
        return PathBuf::from(p);
    }
    if let Some(p) = &cfg.herdr_config {
        return expand_home(p);
    }
    home().join(".config/herdr/config.toml")
}

fn expand_home(p: &str) -> PathBuf {
    match p.strip_prefix("~/") {
        Some(rest) => home().join(rest),
        None if p == "~" => home(),
        None => PathBuf::from(p),
    }
}

// --- plugin config --------------------------------------------------------------

#[derive(Debug)]
struct Rule {
    label: Option<String>,
    path: Option<String>,
    palette: String,
}

#[derive(Debug)]
struct PluginConfig {
    auto: bool,
    herdr_config: Option<String>,
    palettes: BTreeMap<String, Palette>,
    rules: Vec<Rule>,
}

fn load_plugin_config(ctx: &Ctx) -> Res<PluginConfig> {
    let path = ctx.config_dir.join("config.toml");
    if !path.exists() {
        fs::create_dir_all(&ctx.config_dir)
            .map_err(|e| format!("create {}: {e}", ctx.config_dir.display()))?;
        fs::write(&path, DEFAULT_CONFIG).map_err(|e| format!("seed {}: {e}", path.display()))?;
        eprintln!("[{TAG}] wrote default config to {}", path.display());
    }
    let text = fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    parse_plugin_config(&text).map_err(|e| format!("{}: {e}", path.display()))
}

fn parse_plugin_config(text: &str) -> Res<PluginConfig> {
    let doc: DocumentMut = text.parse().map_err(|e| format!("parse error: {e}"))?;

    let auto = doc.get("auto").and_then(Item::as_bool).unwrap_or(true);
    let herdr_config = doc.get("herdr_config").and_then(Item::as_str).map(str::to_owned);

    let mut palettes = BTreeMap::new();
    if let Some(table) = doc.get("palettes").and_then(Item::as_table_like) {
        for (name, item) in table.iter() {
            let entries = item
                .as_table_like()
                .ok_or_else(|| format!("palettes.{name} must be a table of token = \"#hex\""))?;
            let mut palette = Palette::new();
            for (token, v) in entries.iter() {
                let colour = v
                    .as_str()
                    .ok_or_else(|| format!("palettes.{name}.{token} must be a string"))?;
                palette.insert(token.to_owned(), colour.to_owned());
            }
            palettes.insert(name.to_owned(), palette);
        }
    }

    let mut rules = Vec::new();
    if let Some(list) = doc.get("workspaces").and_then(Item::as_array_of_tables) {
        for (i, t) in list.iter().enumerate() {
            let label = t.get("label").and_then(Item::as_str).map(str::to_owned);
            let path = t.get("path").and_then(Item::as_str).map(str::to_owned);
            if label.is_none() && path.is_none() {
                return Err(format!("workspaces[{i}] needs `label` or `path`"));
            }
            let palette = t.get("palette").and_then(Item::as_str)
                .ok_or_else(|| format!("workspaces[{i}] needs `palette`"))?;
            rules.push(Rule { label, path, palette: palette.to_owned() });
        }
    }

    let cfg = PluginConfig { auto, herdr_config, palettes, rules };
    validate_plugin_config(&cfg)?;
    Ok(cfg)
}

fn validate_plugin_config(cfg: &PluginConfig) -> Res<()> {
    if cfg.palettes.is_empty() {
        return Err("no palettes defined".into());
    }
    for (name, palette) in &cfg.palettes {
        if palette.is_empty() {
            return Err(format!("palettes.{name} is empty"));
        }
        for (token, colour) in palette {
            if !THEME_TOKENS.contains(&token.as_str()) {
                return Err(format!(
                    "palettes.{name}.{token}: not a theme.custom token (allowed: {})",
                    THEME_TOKENS.join(", ")
                ));
            }
            if !is_hex_colour(colour) {
                return Err(format!(
                    "palettes.{name}.{token} = {colour:?}: colours must be #rgb or #rrggbb"
                ));
            }
        }
    }
    for rule in &cfg.rules {
        if rule.label.as_deref().map_or(true, |s| s.trim().is_empty())
            && rule.path.as_deref().map_or(true, |s| s.trim().is_empty())
        {
            return Err("workspaces[] needs a non-empty `label` or `path`".into());
        }
        if !cfg.palettes.contains_key(&rule.palette) {
            return Err(format!(
                "workspaces[].palette = {:?} is not a defined palette",
                rule.palette
            ));
        }
    }
    Ok(())
}

fn is_hex_colour(s: &str) -> bool {
    let Some(hex) = s.strip_prefix('#') else { return false };
    matches!(hex.len(), 3 | 6) && hex.chars().all(|c| c.is_ascii_hexdigit())
}

// --- state ----------------------------------------------------------------------

#[derive(Default)]
struct State {
    managed: Vec<String>,
    last_workspace: Option<String>,
    last_palette: Option<String>,
}

fn state_path(ctx: &Ctx) -> PathBuf {
    ctx.state_dir.join("state.json")
}

fn load_state(ctx: &Ctx) -> State {
    let Ok(text) = fs::read_to_string(state_path(ctx)) else { return State::default() };
    let Ok(v) = serde_json::from_str::<Value>(&text) else { return State::default() };
    State {
        managed: v["managed"].as_array().map(|a| {
            a.iter().filter_map(Value::as_str).map(str::to_owned).collect()
        }).unwrap_or_default(),
        last_workspace: v["last_workspace"].as_str().map(str::to_owned),
        last_palette: v["last_palette"].as_str().map(str::to_owned),
    }
}

fn save_state(ctx: &Ctx, st: &State) -> Res<()> {
    fs::create_dir_all(&ctx.state_dir).map_err(|e| format!("create {}: {e}", ctx.state_dir.display()))?;
    let v = serde_json::json!({
        "managed": st.managed,
        "last_workspace": st.last_workspace,
        "last_palette": st.last_palette,
    });
    fs::write(state_path(ctx), serde_json::to_string_pretty(&v).unwrap())
        .map_err(|e| format!("write state: {e}"))
}

// --- herdr ----------------------------------------------------------------------

struct Workspace {
    id: String,
    label: String,
    cwd: String,
    focused: bool,
}

fn herdr_json(ctx: &Ctx, args: &[&str]) -> Res<Value> {
    let out = Command::new(&ctx.herdr).args(args).output()
        .map_err(|e| format!("run {} {}: {e}", ctx.herdr, args.join(" ")))?;
    if !out.status.success() {
        return Err(format!(
            "`herdr {}` failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let v: Value = serde_json::from_slice(&out.stdout)
        .map_err(|e| format!("`herdr {}`: bad JSON: {e}", args.join(" ")))?;
    Ok(v.get("result").cloned().unwrap_or(v))
}

fn herdr_run(ctx: &Ctx, args: &[&str]) -> Res<()> {
    let out = Command::new(&ctx.herdr).args(args).output()
        .map_err(|e| format!("run {} {}: {e}", ctx.herdr, args.join(" ")))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "`herdr {}` failed: {}{}",
            args.join(" "),
            String::from_utf8_lossy(&out.stdout).trim(),
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// Event hooks get `data.workspace_id`; actions get `HERDR_WORKSPACE_ID`; plain
/// CLI use falls back to whatever is focused.
fn resolve_workspace_id(ctx: &Ctx) -> Res<String> {
    if let Ok(raw) = env::var("HERDR_PLUGIN_EVENT_JSON") {
        if let Ok(ev) = serde_json::from_str::<Value>(&raw) {
            let data = ev.get("data").unwrap_or(&ev);
            for candidate in [
                data.get("workspace_id"),
                data.get("workspace").and_then(|w| w.get("workspace_id")),
                ev.get("workspace_id"),
            ] {
                if let Some(id) = candidate.and_then(Value::as_str) {
                    return Ok(id.to_owned());
                }
            }
        }
    }
    if let Ok(id) = env::var("HERDR_WORKSPACE_ID") {
        if !id.is_empty() {
            return Ok(id);
        }
    }
    all_workspaces(ctx)?
        .into_iter()
        .find(|w| w.focused)
        .map(|w| w.id)
        .ok_or_else(|| "no focused workspace".into())
}

fn all_workspaces(ctx: &Ctx) -> Res<Vec<Workspace>> {
    let list = herdr_json(ctx, &["workspace", "list"])?;
    let mut out = Vec::new();
    for w in list["workspaces"].as_array().into_iter().flatten() {
        let id = w["workspace_id"].as_str().unwrap_or_default().to_owned();
        if id.is_empty() {
            continue;
        }
        out.push(Workspace {
            cwd: workspace_cwd(ctx, &id).unwrap_or_default(),
            label: w["label"].as_str().unwrap_or_default().to_owned(),
            focused: w["focused"].as_bool().unwrap_or(false),
            id,
        });
    }
    Ok(out)
}

fn workspace_info(ctx: &Ctx, id: &str) -> Res<Workspace> {
    let w = herdr_json(ctx, &["workspace", "get", id])?;
    let w = w.get("workspace").unwrap_or(&w);
    Ok(Workspace {
        id: id.to_owned(),
        label: w["label"].as_str().unwrap_or_default().to_owned(),
        cwd: workspace_cwd(ctx, id).unwrap_or_default(),
        focused: w["focused"].as_bool().unwrap_or(false),
    })
}

/// `workspace get` carries no cwd; the panes do. The first pane's cwd is the
/// workspace's project directory for every layout the manager plugin builds.
fn workspace_cwd(ctx: &Ctx, id: &str) -> Res<String> {
    let panes = herdr_json(ctx, &["pane", "list", "--workspace", id])?;
    Ok(panes["panes"]
        .as_array()
        .and_then(|a| a.first())
        .and_then(|p| p["cwd"].as_str())
        .unwrap_or_default()
        .to_owned())
}

// --- palette selection ----------------------------------------------------------

enum Pick<'a> {
    Explicit(&'a str),
    Auto(&'a str),
    None,
}

fn choose_palette<'a>(cfg: &'a PluginConfig, ws: &Workspace) -> Pick<'a> {
    // Explicit rules: an exact label match, or the longest cwd prefix match.
    let mut best: Option<(usize, &Rule)> = None;
    for rule in &cfg.rules {
        let score = if rule.label.as_deref() == Some(ws.label.as_str()) {
            Some(usize::MAX)
        } else if let Some(p) = &rule.path {
            let m = expand_home(p);
            let m = m.to_string_lossy();
            let m = m.trim_end_matches('/');
            (!ws.cwd.is_empty() && (ws.cwd == m || ws.cwd.starts_with(&format!("{m}/"))))
                .then_some(m.len())
        } else {
            None
        };
        if let Some(s) = score {
            if best.map_or(true, |(b, _)| s > b) {
                best = Some((s, rule));
            }
        }
    }
    if let Some((_, rule)) = best {
        return Pick::Explicit(&rule.palette);
    }
    if !cfg.auto {
        return Pick::None;
    }
    let names: Vec<&str> = cfg.palettes.keys().map(String::as_str).collect();
    let key = if ws.cwd.is_empty() { &ws.label } else { &ws.cwd };
    Pick::Auto(names[(fnv1a(key) % names.len() as u64) as usize])
}

fn fnv1a(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3))
}

// --- config editing (pure) ------------------------------------------------------

/// Build `[theme]` / `[theme.custom]` explicitly. Index auto-vivification in
/// toml_edit creates *inline* tables (`theme = {}`), which would render the
/// managed keys on one line and leave an empty inline table behind on clear.
fn custom_table(doc: &mut DocumentMut) -> Res<&mut dyn toml_edit::TableLike> {
    if doc.get("theme").is_none() {
        let mut t = toml_edit::Table::new();
        t.set_implicit(true);
        doc.insert("theme", Item::Table(t));
    }
    let theme = doc
        .get_mut("theme")
        .and_then(Item::as_table_like_mut)
        .ok_or("theme exists but is not a table")?;
    if theme.get("custom").is_none() {
        theme.insert("custom", Item::Table(toml_edit::Table::new()));
    }
    theme
        .get_mut("custom")
        .and_then(Item::as_table_like_mut)
        .ok_or_else(|| "theme.custom exists but is not a table".into())
}

fn plan_apply(original: &str, palette: &Palette, previously_managed: &[String]) -> Res<String> {
    let mut doc: DocumentMut = original.parse().map_err(|e| format!("herdr config parse error: {e}"))?;
    {
        let table = custom_table(&mut doc)?;
        for key in previously_managed {
            if !palette.contains_key(key) {
                table.remove(key);
            }
        }
        for (token, colour) in palette {
            table.insert(token, value(colour));
        }
    }
    tidy_theme(&mut doc);
    Ok(doc.to_string())
}

fn plan_clear(original: &str, managed: &[String]) -> Res<String> {
    let mut doc: DocumentMut = original.parse().map_err(|e| format!("herdr config parse error: {e}"))?;
    if let Some(table) = doc
        .get_mut("theme")
        .and_then(Item::as_table_like_mut)
        .and_then(|t| t.get_mut("custom"))
        .and_then(Item::as_table_like_mut)
    {
        for key in managed {
            table.remove(key);
        }
    }
    tidy_theme(&mut doc);
    Ok(doc.to_string())
}

/// Drop `[theme.custom]` / `[theme]` when the plugin emptied them (standard or
/// inline), and keep `[theme]` implicit when it only holds sub-tables so no
/// bare header is emitted.
fn tidy_theme(doc: &mut DocumentMut) {
    let theme_empty = {
        let Some(theme) = doc.get_mut("theme") else { return };
        if let Some(t) = theme.as_table_like_mut() {
            let custom_empty = t
                .get("custom")
                .and_then(Item::as_table_like)
                .map_or(false, |c| c.is_empty());
            if custom_empty {
                t.remove("custom");
            }
        }
        theme.as_table_like().map_or(false, |t| t.is_empty())
    };
    if theme_empty {
        doc.remove("theme");
        return;
    }
    if let Some(t) = doc.get_mut("theme").and_then(Item::as_table_mut) {
        if t.iter().all(|(_, v)| v.is_table()) {
            t.set_implicit(true);
        }
    }
}

// --- commit ---------------------------------------------------------------------

fn commit(ctx: &Ctx, path: &Path, original: &str, updated: &str, dry_run: bool, what: &str) -> Res<bool> {
    if updated == original {
        println!("[{TAG}] {what}: already in place, nothing to do");
        return Ok(false);
    }
    if dry_run {
        println!("[{TAG}] {what}: dry run, would change {}:", path.display());
        print!("{}", diff(original, updated));
        return Ok(false);
    }

    ensure_backup(path, original)?;
    write_atomic(path, updated)?;

    if let Err(e) = herdr_run(ctx, &["config", "check"]) {
        write_atomic(path, original)?;
        return Err(format!("{what}: `herdr config check` rejected the result, restored the previous config.\n{e}"));
    }
    herdr_run(ctx, &["server", "reload-config"])
        .map_err(|e| format!("{what}: config written and valid, but the reload failed (is the server running?)\n{e}"))?;
    println!("[{TAG}] {what}: applied");
    Ok(true)
}

fn ensure_backup(path: &Path, original: &str) -> Res<()> {
    let backup = PathBuf::from(format!("{}{BACKUP_SUFFIX}", path.display()));
    if backup.exists() {
        return Ok(());
    }
    fs::write(&backup, original).map_err(|e| format!("write backup {}: {e}", backup.display()))?;
    eprintln!("[{TAG}] first edit: backed up the original to {}", backup.display());
    Ok(())
}

fn write_atomic(path: &Path, text: &str) -> Res<()> {
    let tmp = PathBuf::from(format!("{}.{TAG}.tmp", path.display()));
    fs::write(&tmp, text).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    fs::rename(&tmp, path).map_err(|e| format!("rename into {}: {e}", path.display()))
}

fn diff(old: &str, new: &str) -> String {
    let old: Vec<&str> = old.lines().collect();
    let new: Vec<&str> = new.lines().collect();
    let mut out = String::new();
    for l in &old {
        if !new.contains(l) {
            out.push_str(&format!("- {l}\n"));
        }
    }
    for l in &new {
        if !old.contains(l) {
            out.push_str(&format!("+ {l}\n"));
        }
    }
    out
}

// --- commands -------------------------------------------------------------------

fn cmd_apply(dry_run: bool) -> Res<()> {
    let ctx = ctx();
    let cfg = load_plugin_config(&ctx)?;
    let mut st = load_state(&ctx);

    let id = resolve_workspace_id(&ctx)?;
    let ws = workspace_info(&ctx, &id)?;

    let (palette_name, how) = match choose_palette(&cfg, &ws) {
        Pick::Explicit(n) => (Some(n), "explicit rule"),
        Pick::Auto(n) => (Some(n), "auto"),
        Pick::None => (None, "no rule, auto off"),
    };

    // Same workspace, same palette, config untouched since: skip the file read.
    if !dry_run
        && st.last_workspace.as_deref() == Some(&ws.id)
        && st.last_palette.as_deref() == palette_name
    {
        println!("[{TAG}] {} ({}): unchanged", ws.label, palette_name.unwrap_or("none"));
        return Ok(());
    }

    let path = herdr_config_path(&cfg);
    let original = fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;

    let (updated, what) = match palette_name {
        Some(name) => (
            plan_apply(&original, &cfg.palettes[name], &st.managed)?,
            format!("{} → {name} ({how})", ws.label),
        ),
        None => (
            plan_clear(&original, &st.managed)?,
            format!("{} → base theme ({how})", ws.label),
        ),
    };

    commit(&ctx, &path, &original, &updated, dry_run, &what)?;

    if !dry_run {
        st.managed = palette_name
            .map(|n| cfg.palettes[n].keys().cloned().collect())
            .unwrap_or_default();
        st.last_workspace = Some(ws.id);
        st.last_palette = palette_name.map(str::to_owned);
        save_state(&ctx, &st)?;
    }
    Ok(())
}

fn cmd_clear(dry_run: bool) -> Res<()> {
    let ctx = ctx();
    let cfg = load_plugin_config(&ctx)?;
    let mut st = load_state(&ctx);

    let path = herdr_config_path(&cfg);
    let original = fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let updated = plan_clear(&original, &st.managed)?;

    commit(&ctx, &path, &original, &updated, dry_run, "clear")?;

    if !dry_run {
        st = State::default();
        save_state(&ctx, &st)?;
    }
    Ok(())
}

fn cmd_status() -> Res<()> {
    let ctx = ctx();
    let cfg = load_plugin_config(&ctx)?;
    let st = load_state(&ctx);

    println!("herdr config: {}", herdr_config_path(&cfg).display());
    println!("state:        {}", state_path(&ctx).display());
    println!("managed keys: {}", if st.managed.is_empty() { "none".into() } else { st.managed.join(", ") });
    println!();
    println!("{:<4} {:<3} {:<18} {:<10} {:<14} CWD", "ID", "", "LABEL", "PALETTE", "VIA");
    for ws in all_workspaces(&ctx)? {
        let (palette, via) = match choose_palette(&cfg, &ws) {
            Pick::Explicit(n) => (n, "explicit"),
            Pick::Auto(n) => (n, "auto"),
            Pick::None => ("-", "none"),
        };
        println!(
            "{:<4} {:<3} {:<18} {:<10} {:<14} {}",
            ws.id,
            if ws.focused { "*" } else { "" },
            ws.label,
            palette,
            via,
            ws.cwd.replacen(&home().to_string_lossy().to_string(), "~", 1),
        );
    }
    Ok(())
}

fn cmd_validate() -> Res<()> {
    let ctx = ctx();
    let cfg = load_plugin_config(&ctx)?;
    println!("config: {}", ctx.config_dir.join("config.toml").display());
    println!("herdr config: {}", herdr_config_path(&cfg).display());
    println!("auto: {}", cfg.auto);
    println!("palettes ({}):", cfg.palettes.len());
    for (name, p) in &cfg.palettes {
        let pairs: Vec<String> = p.iter().map(|(k, v)| format!("{k}={v}")).collect();
        println!("  {name}: {}", pairs.join(" "));
    }
    println!("rules ({}):", cfg.rules.len());
    for r in &cfg.rules {
        let what = match (&r.label, &r.path) {
            (Some(l), Some(p)) => format!("label {l:?} or path {p}"),
            (Some(l), None) => format!("label {l:?}"),
            (None, Some(p)) => format!("path {p}"),
            (None, None) => "(invalid)".into(),
        };
        println!("  {what} → {}", r.palette);
    }
    println!("config is valid.");
    Ok(())
}

// --- tests ----------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn palette(pairs: &[(&str, &str)]) -> Palette {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    const USER_CONFIG: &str = "\
# my herdr config
onboarding = false

[session]
resume_agents_on_restore = true   # keep

[[keys.command]]
key = \"prefix+space\"
type = \"plugin_action\"
command = \"herdr-layout-cycle.cycle-layout\"
";

    #[test]
    fn apply_adds_keys_and_preserves_everything_else() {
        let out = plan_apply(USER_CONFIG, &palette(&[("accent", "#f38ba8"), ("sidebar_bg", "#2e1f26")]), &[]).unwrap();
        assert!(out.starts_with("# my herdr config\n"), "leading comment lost");
        assert!(out.contains("resume_agents_on_restore = true   # keep"), "inline comment lost");
        assert!(out.contains("command = \"herdr-layout-cycle.cycle-layout\""));
        assert!(out.contains("[theme.custom]"));
        assert!(out.contains("accent = \"#f38ba8\""));
        assert!(out.contains("sidebar_bg = \"#2e1f26\""));
        assert!(!out.contains("\n[theme]\n"), "bare [theme] header must stay implicit");
    }

    #[test]
    fn apply_is_idempotent() {
        let p = palette(&[("accent", "#f38ba8")]);
        let once = plan_apply(USER_CONFIG, &p, &[]).unwrap();
        let twice = plan_apply(&once, &p, &["accent".into()]).unwrap();
        assert_eq!(once, twice);
    }

    #[test]
    fn switching_palette_removes_stale_managed_keys_only() {
        let first = plan_apply(USER_CONFIG, &palette(&[("accent", "#f38ba8"), ("sidebar_bg", "#2e1f26")]), &[]).unwrap();
        let managed = vec!["accent".to_string(), "sidebar_bg".to_string()];
        let second = plan_apply(&first, &palette(&[("accent", "#89b4fa")]), &managed).unwrap();
        assert!(second.contains("accent = \"#89b4fa\""));
        assert!(!second.contains("sidebar_bg"), "stale managed key must be removed");
    }

    #[test]
    fn user_owned_custom_keys_survive_apply_and_clear() {
        let base = format!("{USER_CONFIG}\n[theme]\nname = \"catppuccin\"\n\n[theme.custom]\ntext = \"#ffffff\"\n");
        let applied = plan_apply(&base, &palette(&[("accent", "#f38ba8")]), &[]).unwrap();
        assert!(applied.contains("text = \"#ffffff\""));
        assert!(applied.contains("name = \"catppuccin\""));
        let cleared = plan_clear(&applied, &["accent".into()]).unwrap();
        assert!(cleared.contains("text = \"#ffffff\""), "user key removed by clear");
        assert!(!cleared.contains("accent"));
        assert!(cleared.contains("[theme]\nname = \"catppuccin\""), "explicit [theme] with a value must stay");
    }

    #[test]
    fn clear_restores_original_bytes_when_plugin_created_the_tables() {
        let p = palette(&[("accent", "#f38ba8"), ("sidebar_bg", "#2e1f26")]);
        let applied = plan_apply(USER_CONFIG, &p, &[]).unwrap();
        let managed: Vec<String> = p.keys().cloned().collect();
        let cleared = plan_clear(&applied, &managed).unwrap();
        assert_eq!(cleared, USER_CONFIG);
    }

    #[test]
    fn inline_custom_table_is_handled() {
        let base = "[theme]\ncustom = { text = \"#ffffff\" }\n";
        let applied = plan_apply(base, &palette(&[("accent", "#f38ba8")]), &[]).unwrap();
        assert!(applied.contains("accent = \"#f38ba8\""));
        assert!(applied.contains("text = \"#ffffff\""));
        let cleared = plan_clear(&applied, &["accent".into()]).unwrap();
        assert!(cleared.contains("text = \"#ffffff\""));
        assert!(!cleared.contains("accent"));
    }

    #[test]
    fn hex_validation_is_strict() {
        for ok in ["#fff", "#F38BA8", "#000000"] {
            assert!(is_hex_colour(ok), "{ok}");
        }
        for bad in ["fff", "#ff", "#ffff", "#ggg", "red", "rgb(1,2,3)", "#f38ba8 "] {
            assert!(!is_hex_colour(bad), "{bad}");
        }
    }

    #[test]
    fn plugin_config_rejects_unknown_token_and_bad_colour_and_dangling_rule() {
        let err = parse_plugin_config("[palettes.x]\nnope = \"#fff\"\n").unwrap_err();
        assert!(err.contains("not a theme.custom token"), "{err}");
        let err = parse_plugin_config("[palettes.x]\naccent = \"red\"\n").unwrap_err();
        assert!(err.contains("#rgb or #rrggbb"), "{err}");
        let err = parse_plugin_config("[palettes.x]\naccent = \"#fff\"\n[[workspaces]]\nlabel = \"a\"\npalette = \"missing\"\n").unwrap_err();
        assert!(err.contains("not a defined palette"), "{err}");
        assert!(parse_plugin_config(DEFAULT_CONFIG).is_ok(), "shipped default config must validate");
    }

    #[test]
    fn selection_precedence_label_then_longest_cwd_prefix_then_auto() {
        let cfg = parse_plugin_config(
            "[palettes.a]\naccent = \"#111\"\n[palettes.b]\naccent = \"#222\"\n[palettes.c]\naccent = \"#333\"\n\
             [[workspaces]]\npath = \"/p\"\npalette = \"a\"\n\
             [[workspaces]]\npath = \"/p/deep\"\npalette = \"b\"\n\
             [[workspaces]]\nlabel = \"Exact\"\npalette = \"c\"\n",
        ).unwrap();
        let ws = |label: &str, cwd: &str| Workspace { id: "w".into(), label: label.into(), cwd: cwd.into(), focused: false };
        assert!(matches!(choose_palette(&cfg, &ws("Exact", "/p/deep/x")), Pick::Explicit("c")), "label wins");
        assert!(matches!(choose_palette(&cfg, &ws("x", "/p/deep/x")), Pick::Explicit("b")), "longest prefix wins");
        assert!(matches!(choose_palette(&cfg, &ws("x", "/p/other")), Pick::Explicit("a")));
        assert!(matches!(choose_palette(&cfg, &ws("x", "/plain")), Pick::Auto(_)), "no false prefix match on /p vs /plain");
        assert!(matches!(choose_palette(&cfg, &ws("x", "/elsewhere")), Pick::Auto(_)));
    }

    #[test]
    fn auto_hash_is_stable_and_uses_cwd_over_label() {
        let cfg = parse_plugin_config("[palettes.a]\naccent = \"#111\"\n[palettes.b]\naccent = \"#222\"\n").unwrap();
        let ws = |label: &str, cwd: &str| Workspace { id: "w".into(), label: label.into(), cwd: cwd.into(), focused: false };
        let Pick::Auto(x) = choose_palette(&cfg, &ws("one", "/same")) else { panic!() };
        let Pick::Auto(y) = choose_palette(&cfg, &ws("two", "/same")) else { panic!() };
        assert_eq!(x, y, "same cwd → same colour regardless of label");
        assert_eq!(fnv1a("/Users/x/project"), fnv1a("/Users/x/project"));
        assert_ne!(fnv1a("a"), fnv1a("b"));
    }

    #[test]
    fn auto_off_yields_none() {
        let cfg = parse_plugin_config("auto = false\n[palettes.a]\naccent = \"#111\"\n").unwrap();
        let ws = Workspace { id: "w".into(), label: "x".into(), cwd: "/x".into(), focused: false };
        assert!(matches!(choose_palette(&cfg, &ws), Pick::None));
    }
}
