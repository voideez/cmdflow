//! Locating, parsing and accumulating shell history.

use std::collections::HashSet;
use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

fn env_dir(var: &str) -> Option<PathBuf> {
    env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// `$XDG_DATA_HOME` or `~/.local/share`.
pub fn data_home(home: &Path) -> PathBuf {
    env_dir("XDG_DATA_HOME").unwrap_or_else(|| home.join(".local/share"))
}

/// `$XDG_CONFIG_HOME` or `~/.config`.
pub fn config_home(home: &Path) -> PathBuf {
    env_dir("XDG_CONFIG_HOME").unwrap_or_else(|| home.join(".config"))
}

/// `$ZDOTDIR` or `~`.
pub fn zdotdir(home: &Path) -> PathBuf {
    env_dir("ZDOTDIR").unwrap_or_else(|| home.to_path_buf())
}

/// Directory where cmdflow keeps its own data.
pub fn cmdflow_dir(home: &Path) -> PathBuf {
    data_home(home).join("cmdflow")
}

pub fn fish_history_path(home: &Path) -> PathBuf {
    data_home(home).join("fish/fish_history")
}

/// Bash history: `HISTFILE` from `~/.bashrc`, otherwise `~/.bash_history`.
pub fn bash_history_path(home: &Path) -> PathBuf {
    fs::read_to_string(home.join(".bashrc"))
        .ok()
        .and_then(|rc| histfile_from_rc(&rc, home, home))
        .unwrap_or_else(|| home.join(".bash_history"))
}

/// Zsh history: `HISTFILE` from `.zshrc`, otherwise the first common location that exists.
/// zsh has no default history file, so `None` means zsh isn't saving history at all.
pub fn zsh_history_path(home: &Path) -> Option<PathBuf> {
    let zdot = zdotdir(home);
    if let Some(p) = fs::read_to_string(zdot.join(".zshrc"))
        .ok()
        .and_then(|rc| histfile_from_rc(&rc, home, &zdot))
    {
        return Some(p);
    }
    [
        zdot.join(".zsh_history"),
        home.join(".zsh_history"),
        zdot.join(".histfile"),
        home.join(".histfile"),
        zdot.join(".zhistory"),
    ]
    .into_iter()
    .find(|p| p.exists())
}

/// Finds the last `HISTFILE=...` assignment in a shell rc file and expands the usual
/// home-directory forms. Values with other expansions are ignored.
fn histfile_from_rc(rc: &str, home: &Path, zdot: &Path) -> Option<PathBuf> {
    let raw = rc
        .lines()
        .filter_map(|l| {
            let l = l.trim();
            let l = l.strip_prefix("export ").unwrap_or(l);
            l.strip_prefix("HISTFILE=")
        })
        .next_back()?;
    let value = raw
        .split_whitespace()
        .next()?
        .trim_matches(|c| c == '"' || c == '\'');

    let home_s = home.to_string_lossy();
    let zdot_s = zdot.to_string_lossy();
    let mut path = value.to_string();
    if let Some(rest) = path.strip_prefix('~') {
        path = format!("{home_s}{rest}");
    }
    for (pat, rep) in [
        ("${ZDOTDIR:-$HOME}", &*zdot_s),
        ("${ZDOTDIR:-~}", &*zdot_s),
        ("${ZDOTDIR}", &*zdot_s),
        ("$ZDOTDIR", &*zdot_s),
        ("${HOME}", &*home_s),
        ("$HOME", &*home_s),
    ] {
        path = path.replace(pat, rep);
    }
    if path.is_empty() || path.contains('$') {
        return None;
    }
    let path = PathBuf::from(path);
    Some(if path.is_absolute() { path } else { home.join(path) })
}

// ---------------------------------------------------------------------------
// Parsers
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq)]
pub struct FishEntry {
    pub cmd: String,
    pub when: u64,
}

/// Parses fish's YAML-like history file.
pub fn parse_fish(content: &str) -> Vec<FishEntry> {
    let mut out: Vec<FishEntry> = Vec::new();
    let mut has_when = false;
    for line in content.lines() {
        if let Some(cmd) = line.strip_prefix("- cmd: ") {
            out.push(FishEntry { cmd: unescape_fish(cmd), when: 0 });
            has_when = false;
        } else if let Some(w) = line.strip_prefix("  when: ") {
            if let (Some(last), false) = (out.last_mut(), has_when) {
                last.when = w.trim().parse().unwrap_or(0);
                has_when = true;
            }
        }
    }
    out
}

/// fish stores newlines as `\n` and backslashes as `\\`.
fn unescape_fish(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// Parses `~/.bash_history`. Lines starting with `#` are `HISTTIMEFORMAT` timestamps.
pub fn parse_bash(content: &str) -> Vec<String> {
    content
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_string)
        .collect()
}

/// Parses a zsh history file (plain or `EXTENDED_HISTORY`).
/// Only the first line of multi-line commands is kept: it is the one with the command name.
pub fn parse_zsh(bytes: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(&unmetafy(bytes)).into_owned();
    let mut out = Vec::new();
    let mut continuation = false;
    for line in text.lines() {
        let was_continuation = continuation;
        continuation = line.ends_with('\\');
        if was_continuation {
            continue;
        }
        let cmd = strip_zsh_extended(line)
            .trim_end_matches('\\')
            .trim();
        if !cmd.is_empty() {
            out.push(cmd.to_string());
        }
    }
    out
}

/// zsh "metafies" bytes 0x83..=0xFF: they are stored as 0x83 followed by `byte ^ 0x20`.
/// Without this, non-ASCII commands come out garbled.
fn unmetafy(bytes: &[u8]) -> Vec<u8> {
    const META: u8 = 0x83;
    let mut out = Vec::with_capacity(bytes.len());
    let mut it = bytes.iter();
    while let Some(&b) = it.next() {
        if b == META {
            if let Some(&n) = it.next() {
                out.push(n ^ 0x20);
            }
        } else {
            out.push(b);
        }
    }
    out
}

/// Strips the `: 1700000000:0;` prefix of `EXTENDED_HISTORY` lines.
fn strip_zsh_extended(line: &str) -> &str {
    if let Some(rest) = line.strip_prefix(": ") {
        if let Some(idx) = rest.find(';') {
            let meta = &rest[..idx];
            if !meta.is_empty() && meta.chars().all(|c| c.is_ascii_digit() || c == ':') {
                return &rest[idx + 1..];
            }
        }
    }
    line
}

// ---------------------------------------------------------------------------
// Command name extraction
// ---------------------------------------------------------------------------

/// Commands that only wrap the real command: `sudo pacman -Syu` counts as `pacman`.
const WRAPPERS: &[&str] = &["sudo", "doas", "run0", "env", "nohup", "time"];

/// Wrapper options that take a separate argument (`sudo -u root cmd`).
const WRAPPER_OPTS_WITH_ARG: &[&str] = &[
    "-u", "-g", "-C", "-D", "-p", "-r", "-t", "-U", "-T", "-S", "--user", "--group", "--chdir",
];

fn is_env_assignment(token: &str) -> bool {
    match token.split_once('=') {
        Some((name, _)) => {
            let mut chars = name.chars();
            matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
                && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        }
        None => false,
    }
}

/// Extracts the command name from a history line, e.g. `cargo` from `cargo build`.
///
/// Skips `VAR=value` prefixes and wrappers like `sudo`/`doas`/`env`, and stops at shell
/// operators so `make&&./run` counts as `make`. Returns `None` for lines that don't start
/// with a command name (`./script`, `(subshell)`, `# comment`).
pub fn command_key(line: &str) -> Option<String> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    let mut i = 0;
    let mut wrapper = None;
    while i < tokens.len() {
        let t = tokens[i];
        if is_env_assignment(t) {
            i += 1;
        } else if WRAPPERS.contains(&t) {
            wrapper.get_or_insert(t);
            i += 1;
            while i < tokens.len() && tokens[i].starts_with('-') {
                let takes_arg = WRAPPER_OPTS_WITH_ARG.contains(&tokens[i]);
                i += if takes_arg { 2 } else { 1 };
            }
        } else {
            break;
        }
    }

    // `sudo -i` with nothing after it still counts as `sudo`
    let token = tokens.get(i).copied().or(wrapper)?;
    let key = token.split(|c| ";|&()<>`".contains(c)).next()?;
    if !key.chars().next()?.is_alphanumeric() {
        return None;
    }
    Some(key.to_string())
}

// ---------------------------------------------------------------------------
// Fish accumulation
// ---------------------------------------------------------------------------
//
// fish keeps only the latest copy of each command in fish_history, so repeats vanish.
// To count them anyway, cmdflow:
//   1. remembers every (timestamp, command) pair it ever saw in `fish_seen.tsv`, so entries
//      that fish later deduplicates are still counted;
//   2. after `--fix-history`, reads `fish_live.log`, written by a fish_postexec hook,
//      which records every command. History entries newer than the hook's install
//      time (`fish_live.since`) are then skipped to avoid double counting.

pub const FISH_SEEN: &str = "fish_seen.tsv";
pub const FISH_LIVE: &str = "fish_live.log";
pub const FISH_SINCE: &str = "fish_live.since";

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn flatten(cmd: &str) -> String {
    cmd.replace(['\n', '\t'], " ")
}

/// Returns every fish command cmdflow knows about and updates the persistent cache.
pub fn collect_fish(history: &Path, data_dir: &Path) -> Vec<String> {
    let since: Option<u64> = fs::read_to_string(data_dir.join(FISH_SINCE))
        .ok()
        .and_then(|s| s.trim().parse().ok());

    // Previously seen entries
    let seen_path = data_dir.join(FISH_SEEN);
    let mut seen: HashSet<(u64, String)> = HashSet::new();
    let mut commands: Vec<String> = Vec::new();
    if let Ok(content) = fs::read_to_string(&seen_path) {
        for line in content.lines() {
            if let Some((when, cmd)) = line.split_once('\t') {
                if let Ok(when) = when.parse() {
                    if seen.insert((when, cmd.to_string())) {
                        commands.push(cmd.to_string());
                    }
                }
            }
        }
    }

    // New entries from fish_history
    let mut new_lines = String::new();
    if let Ok(content) = fs::read_to_string(history) {
        for entry in parse_fish(&content) {
            if since.is_some_and(|s| entry.when >= s) {
                continue; // covered by the live log
            }
            let cmd = flatten(&entry.cmd);
            if seen.insert((entry.when, cmd.clone())) {
                new_lines.push_str(&format!("{}\t{}\n", entry.when, cmd));
                commands.push(cmd);
            }
        }
    }
    if !new_lines.is_empty() {
        let saved = fs::create_dir_all(data_dir).and_then(|_| {
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&seen_path)?
                .write_all(new_lines.as_bytes())
        });
        if let Err(e) = saved {
            eprintln!("cmdflow: could not update {}: {e}", seen_path.display());
        }
    }

    // Live log from the fish_postexec hook
    if since.is_some() {
        if let Ok(content) = fs::read_to_string(data_dir.join(FISH_LIVE)) {
            commands.extend(
                content
                    .lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty())
                    .map(str::to_string),
            );
        }
    }

    commands
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(name: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("cmdflow-test-{}-{}", name, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn key_basic() {
        assert_eq!(command_key("cargo build --release"), Some("cargo".into()));
        assert_eq!(command_key("  ls  "), Some("ls".into()));
        assert_eq!(command_key(""), None);
    }

    #[test]
    fn key_keeps_case() {
        assert_eq!(command_key("Hyprland"), Some("Hyprland".into()));
    }

    #[test]
    fn key_skips_env_and_wrappers() {
        assert_eq!(command_key("RUST_LOG=debug cargo run"), Some("cargo".into()));
        assert_eq!(command_key("sudo pacman -Syu"), Some("pacman".into()));
        assert_eq!(command_key("sudo -E -u root nvim /etc/fstab"), Some("nvim".into()));
        assert_eq!(command_key("doas -u alice env FOO=1 make"), Some("make".into()));
        assert_eq!(command_key("sudo -i"), Some("sudo".into()));
        assert_eq!(command_key("FOO=1"), None);
    }

    #[test]
    fn key_stops_at_operators() {
        assert_eq!(command_key("make&&./run"), Some("make".into()));
        assert_eq!(command_key("ls;reboot"), Some("ls".into()));
        assert_eq!(command_key("cat file|grep x"), Some("cat".into()));
    }

    #[test]
    fn key_rejects_non_commands() {
        assert_eq!(command_key("./script.sh"), None);
        assert_eq!(command_key("(cd /tmp)"), None);
        assert_eq!(command_key("# comment"), None);
    }

    #[test]
    fn key_unicode() {
        assert_eq!(command_key("пинг 1.1.1.1"), Some("пинг".into()));
    }

    #[test]
    fn fish_parsing() {
        let h = "- cmd: git status\n  when: 100\n- cmd: echo a\\nb\n  when: 200\n  paths:\n    - b\n- cmd: echo \\\\n\n  when: 300\n";
        let e = parse_fish(h);
        assert_eq!(e.len(), 3);
        assert_eq!(e[0], FishEntry { cmd: "git status".into(), when: 100 });
        assert_eq!(e[1].cmd, "echo a\nb");
        assert_eq!(e[2].cmd, "echo \\n");
    }

    #[test]
    fn bash_parsing() {
        let h = "#1700000000\nls -la\n\n  git push \n";
        assert_eq!(parse_bash(h), vec!["ls -la", "git push"]);
    }

    #[test]
    fn zsh_parsing() {
        let h = b": 1700000000:0;ls -la\nplain cmd\n: 1700000001:3;for i in 1 2; do\\\necho $i\\\ndone\n: 1700000002:0;git log\n";
        assert_eq!(parse_zsh(h), vec!["ls -la", "plain cmd", "for i in 1 2; do", "git log"]);
    }

    #[test]
    fn zsh_unmetafy() {
        // "ё" is 0xD1 0x91; zsh writes 0x91 as 0x83 0xB1
        let h = [b'e', b'c', b'h', b'o', b' ', 0xD1, 0x83, 0x91 ^ 0x20, b'\n'];
        assert_eq!(parse_zsh(&h), vec!["echo ё"]);
    }

    #[test]
    fn histfile_expansion() {
        let home = Path::new("/home/u");
        let zdot = Path::new("/home/u/.config/zsh");
        let p = |rc: &str| histfile_from_rc(rc, home, zdot);
        assert_eq!(p("HISTFILE=~/.histfile"), Some("/home/u/.histfile".into()));
        assert_eq!(p("export HISTFILE=\"$HOME/.zsh_history\""), Some("/home/u/.zsh_history".into()));
        assert_eq!(
            p("HISTFILE=${ZDOTDIR:-$HOME}/.zsh_history"),
            Some("/home/u/.config/zsh/.zsh_history".into())
        );
        assert_eq!(p("HISTFILE=a\nHISTFILE=/tmp/h"), Some("/tmp/h".into()));
        assert_eq!(p("HISTFILE=$XDG_STATE_HOME/zsh"), None);
        assert_eq!(p("SAVEHIST=10"), None);
    }

    #[test]
    fn fish_accumulates_deduplicated_entries() {
        let dir = tmpdir("fish-acc");
        let hist = dir.join("fish_history");

        fs::write(&hist, "- cmd: ls\n  when: 1\n- cmd: git status\n  when: 2\n").unwrap();
        assert_eq!(collect_fish(&hist, &dir).len(), 2);

        // fish deduplicated `ls` and kept only the newest copy
        fs::write(&hist, "- cmd: git status\n  when: 2\n- cmd: ls\n  when: 3\n").unwrap();
        let cmds = collect_fish(&hist, &dir);
        assert_eq!(cmds.iter().filter(|c| *c == "ls").count(), 2);
        assert_eq!(cmds.len(), 3);

        // running again adds nothing new
        assert_eq!(collect_fish(&hist, &dir).len(), 3);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn fish_live_log_replaces_newer_history() {
        let dir = tmpdir("fish-live");
        let hist = dir.join("fish_history");
        fs::write(dir.join(FISH_SINCE), "10\n").unwrap();
        fs::write(dir.join(FISH_LIVE), "ls\nls\nls\n").unwrap();
        fs::write(&hist, "- cmd: vim\n  when: 5\n- cmd: ls\n  when: 12\n").unwrap();

        let cmds = collect_fish(&hist, &dir);
        assert_eq!(cmds, vec!["vim", "ls", "ls", "ls"]);
        let _ = fs::remove_dir_all(&dir);
    }
}
