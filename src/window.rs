//! Outer terminal window tint.
//!
//! Herdr draws inside a terminal emulator; the window chrome (title bar) belongs
//! to that emulator, not to herdr, so it can only be coloured through whatever
//! the emulator itself exposes. Apple Terminal ignores OSC colour sequences for
//! the window but is scriptable: the tab hosting a herdr client is found by its
//! tty and its `background color` set, which macOS also uses for the title bar.
//!
//! The first colour seen on a tab is saved so `clear` (and a switch to the base
//! theme) can put it back exactly.

use std::collections::BTreeMap;
use std::process::Command;

type Res<T> = Result<T, String>;

#[derive(Default)]
pub struct WindowState {
    /// tty → AppleScript colour triple ("r,g,b", 16-bit) seen before the first tint.
    pub original: BTreeMap<String, String>,
    /// tty → hex colour currently applied by this plugin.
    pub current: BTreeMap<String, String>,
}

/// `ps -axo tty=,command=` output, or None when ps is unavailable.
pub fn read_ps() -> Option<String> {
    let out = Command::new("ps")
        .args(["-axo", "tty=,command="])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Terminal devices with an attached herdr client (not the server, which has
/// no controlling terminal).
pub fn client_ttys(ps: &str) -> Vec<String> {
    let mut ttys = Vec::new();
    for line in ps.lines() {
        let mut parts = line.split_whitespace();
        let (Some(tty), Some(cmd)) = (parts.next(), parts.next()) else {
            continue;
        };
        if tty == "??" || tty == "?" {
            continue;
        }
        let base = cmd.rsplit('/').next().unwrap_or(cmd);
        if base != "herdr" {
            continue;
        }
        if parts.next() == Some("server") {
            continue;
        }
        let dev = format!("/dev/{tty}");
        if !ttys.contains(&dev) {
            ttys.push(dev);
        }
    }
    ttys
}

pub fn terminal_app_running(ps: &str) -> bool {
    ps.lines()
        .any(|l| l.contains("/Terminal.app/Contents/MacOS/Terminal"))
}

/// "#rgb" or "#rrggbb" → the 16-bit triple AppleScript's Terminal dictionary uses.
pub fn hex_to_triple(hex: &str) -> Option<String> {
    let h = hex.strip_prefix('#')?;
    let chan = |s: &str| u8::from_str_radix(s, 16).ok().map(|v| u16::from(v) * 257);
    let (r, g, b) = match h.len() {
        3 => {
            let d = |i: usize| {
                let c = &h[i..i + 1];
                chan(&format!("{c}{c}"))
            };
            (d(0)?, d(1)?, d(2)?)
        }
        6 => (chan(&h[0..2])?, chan(&h[2..4])?, chan(&h[4..6])?),
        _ => return None,
    };
    Some(format!("{r},{g},{b}"))
}

/// AppleScript that locates the Terminal tab on `tty` and runs `body` on it as
/// `t`, returning "missing" when no tab has that tty.
fn tab_script(tty: &str, body: &str) -> String {
    format!(
        "tell application \"Terminal\"\n\
         repeat with w in windows\n\
         repeat with t in tabs of w\n\
         if tty of t is \"{tty}\" then\n\
         {body}\n\
         end if\n\
         end repeat\n\
         end repeat\n\
         end tell\n\
         return \"missing\""
    )
}

fn osascript(script: &str) -> Res<String> {
    let out = Command::new("osascript")
        .args(["-e", script])
        .output()
        .map_err(|e| format!("osascript: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_owned());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// Current background of the tab on `tty` as "r,g,b", or None when Terminal
/// has no tab on that tty (the client runs in another emulator).
fn terminal_get(tty: &str) -> Res<Option<String>> {
    let body = "set c to background color of t\n\
                return (item 1 of c as string) & \",\" & (item 2 of c as string) & \",\" & (item 3 of c as string)";
    let v = osascript(&tab_script(tty, body))?;
    Ok((v != "missing").then_some(v))
}

fn terminal_set(tty: &str, triple: &str) -> Res<bool> {
    let body = format!("set background color of t to {{{triple}}}\nreturn \"ok\"");
    Ok(osascript(&tab_script(tty, &body))? == "ok")
}

fn supported() -> bool {
    cfg!(target_os = "macos")
}

/// Tint every Terminal tab hosting a herdr client with `target` (hex), or
/// restore the saved colour when `target` is None. Returns report lines.
pub fn apply(st: &mut WindowState, target: Option<&str>, dry_run: bool) -> Res<Vec<String>> {
    let mut report = Vec::new();
    if !supported() {
        return Ok(report);
    }
    let Some(ps) = read_ps() else {
        return Ok(report);
    };
    if !terminal_app_running(&ps) {
        return Ok(report);
    }
    let ttys = client_ttys(&ps);

    // Tabs whose client went away keep whatever colour they have; forget them
    // after a best-effort restore so state never grows.
    let stale: Vec<String> = st
        .original
        .keys()
        .filter(|t| !ttys.contains(t))
        .cloned()
        .collect();
    for tty in stale {
        if !dry_run {
            if let Some(orig) = st.original.remove(&tty) {
                let _ = terminal_set(&tty, &orig);
            }
            st.current.remove(&tty);
        }
    }

    for tty in &ttys {
        match target {
            Some(hex) => {
                if st.current.get(tty).is_some_and(|c| c == hex) {
                    continue;
                }
                let Some(triple) = hex_to_triple(hex) else {
                    return Err(format!("window colour {hex:?} is not #rgb/#rrggbb"));
                };
                if dry_run {
                    report.push(format!("window {tty}: would tint {hex}"));
                    continue;
                }
                if !st.original.contains_key(tty) {
                    match terminal_get(tty)? {
                        Some(orig) => {
                            st.original.insert(tty.clone(), orig);
                        }
                        None => continue,
                    }
                }
                if terminal_set(tty, &triple)? {
                    st.current.insert(tty.clone(), hex.to_owned());
                    report.push(format!("window {tty}: {hex}"));
                }
            }
            None => {
                let Some(orig) = st.original.get(tty).cloned() else {
                    continue;
                };
                if dry_run {
                    report.push(format!("window {tty}: would restore"));
                    continue;
                }
                terminal_set(tty, &orig)?;
                st.original.remove(tty);
                st.current.remove(tty);
                report.push(format!("window {tty}: restored"));
            }
        }
    }
    Ok(report)
}

/// Put every tinted tab back to the colour it had before the first tint.
pub fn restore_all(st: &mut WindowState) -> Res<Vec<String>> {
    let mut report = Vec::new();
    if !supported() {
        return Ok(report);
    }
    let originals: Vec<(String, String)> = st
        .original
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    for (tty, orig) in originals {
        match terminal_set(&tty, &orig) {
            Ok(true) => report.push(format!("window {tty}: restored")),
            Ok(false) => report.push(format!("window {tty}: tab gone, nothing to restore")),
            Err(e) => report.push(format!("window {tty}: restore failed: {e}")),
        }
    }
    st.original.clear();
    st.current.clear();
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PS: &str = "\
??       /System/Applications/Utilities/Terminal.app/Contents/MacOS/Terminal
??       /opt/homebrew/bin/herdr server
ttys001  herdr
ttys004  /opt/homebrew/bin/herdr attach
ttys009  herdr-space-colors status
ttys001  herdr
";

    #[test]
    fn client_ttys_skips_server_and_unrelated_binaries() {
        assert_eq!(client_ttys(PS), vec!["/dev/ttys001", "/dev/ttys004"]);
    }

    #[test]
    fn detects_terminal_app() {
        assert!(terminal_app_running(PS));
        assert!(!terminal_app_running("ttys001  herdr\n"));
    }

    #[test]
    fn hex_to_triple_handles_short_and_long_forms() {
        assert_eq!(hex_to_triple("#1e1f29").as_deref(), Some("7710,7967,10537"));
        assert_eq!(hex_to_triple("#fff").as_deref(), Some("65535,65535,65535"));
        assert_eq!(hex_to_triple("1e1f29"), None);
        assert_eq!(hex_to_triple("#12345"), None);
        assert_eq!(hex_to_triple("#gg0000"), None);
    }

    #[test]
    fn tab_script_targets_exactly_one_tty() {
        let s = tab_script("/dev/ttys001", "return \"ok\"");
        assert!(s.contains("if tty of t is \"/dev/ttys001\" then"));
        assert!(s.ends_with("return \"missing\""));
    }
}
