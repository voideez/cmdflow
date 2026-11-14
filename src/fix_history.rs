//! `--fix-history`: makes shells record every command, including repeats.
//!
//! Every change is appended to the end of the rc file under a `# Added by cmdflow`
//! comment, so it overrides earlier settings. Running it again only adds what is
//! still missing.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use colored::*;

use crate::history::{self, FISH_LIVE, FISH_SINCE};

/// A line cmdflow wants in an rc file.
enum Want {
    /// `VAR=value`: satisfied when the *last* assignment of `VAR` has exactly this value.
    Assign { var: &'static str, line: &'static str },
    /// `VAR=n`: satisfied when the last assignment of `VAR` is a number `>= min`.
    AtLeast { var: &'static str, min: i64, line: &'static str },
    /// `VAR=value`: only added when `VAR` is not assigned anywhere.
    Default { var: &'static str, line: &'static str },
    /// Any other line: satisfied when it is present.
    Line(&'static str),
}

/// Value of the last `VAR=...` / `export VAR=...` in `content`.
fn last_assignment<'a>(content: &'a str, var: &str) -> Option<&'a str> {
    content
        .lines()
        .filter_map(|l| {
            let l = l.trim();
            let l = l.strip_prefix("export ").unwrap_or(l);
            l.strip_prefix(var)?.strip_prefix('=')
        })
        .next_back()
}

fn has_line(content: &str, line: &str) -> bool {
    content.lines().any(|l| l.trim() == line)
}

fn missing_lines(content: &str, wants: &[Want]) -> Vec<&'static str> {
    wants
        .iter()
        .filter_map(|w| match *w {
            Want::Assign { var, line } => {
                let expected = line.split_once('=').map(|(_, v)| v);
                (last_assignment(content, var) != expected).then_some(line)
            }
            Want::AtLeast { var, min, line } => {
                let ok = last_assignment(content, var)
                    .and_then(|v| v.trim_matches(|c| c == '"' || c == '\'').parse::<i64>().ok())
                    .is_some_and(|n| n < 0 || n >= min);
                (!ok).then_some(line)
            }
            Want::Default { var, line } => {
                last_assignment(content, var).is_none().then_some(line)
            }
            Want::Line(line) => (!has_line(content, line)).then_some(line),
        })
        .collect()
}

fn append(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    write!(file, "{text}")
}

/// Appends whatever is missing and reports the result.
fn ensure(path: &Path, label: &str, wants: &[Want]) {
    let content = fs::read_to_string(path).unwrap_or_default();
    let missing = missing_lines(&content, wants);
    if missing.is_empty() {
        println!("✓ Settings are already configured in {label}");
        return;
    }
    let block = format!("\n# Added by cmdflow\n{}\n", missing.join("\n"));
    match append(path, &block) {
        Ok(()) => println!("✓ {label} updated: {}", missing.join(", ")),
        Err(e) => eprintln!("✗ Could not update {label}: {e}"),
    }
}

const BASH: &[Want] = &[
    Want::Assign { var: "HISTCONTROL", line: "export HISTCONTROL=" },
    Want::AtLeast { var: "HISTSIZE", min: 1_000_000, line: "export HISTSIZE=-1" },
    Want::AtLeast { var: "HISTFILESIZE", min: 1_000_000, line: "export HISTFILESIZE=-1" },
    Want::Line("shopt -s histappend"),
    Want::Line("export PROMPT_COMMAND=\"history -a; $PROMPT_COMMAND\""),
];

const ZSH: &[Want] = &[
    Want::Default { var: "HISTFILE", line: "HISTFILE=~/.zsh_history" },
    Want::AtLeast { var: "HISTSIZE", min: 1_000_000, line: "HISTSIZE=1000000" },
    Want::AtLeast { var: "SAVEHIST", min: 1_000_000, line: "SAVEHIST=1000000" },
    Want::Line("unsetopt HIST_IGNORE_DUPS"),
    Want::Line("unsetopt HIST_IGNORE_ALL_DUPS"),
    Want::Line("unsetopt HIST_SAVE_NO_DUPS"),
    Want::Line("setopt INC_APPEND_HISTORY"),
];

const FISH_HOOK_NAME: &str = "__cmdflow_log";

/// Quotes a string for fish: `'...'` with `\` and `'` escaped.
fn fish_quote(s: &str) -> String {
    format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"))
}

fn fish_hook(log: &Path) -> String {
    format!(
        r#"
# Added by cmdflow: fish drops repeated commands from its history, so log every command.
# Commands starting with a space are private in fish and are not logged.
function {FISH_HOOK_NAME} --on-event fish_postexec
    string match -qr '^\s' -- $argv[1]; and return
    string replace -a \n ' ' -- $argv[1] >> {}
end
"#,
        fish_quote(&log.to_string_lossy())
    )
}

fn fix_fish(home: &Path) {
    let fish_dir = history::config_home(home).join("fish");
    if !fish_dir.exists() {
        return;
    }
    let config = fish_dir.join("config.fish");
    let data_dir = history::cmdflow_dir(home);

    if let Err(e) = fs::create_dir_all(&data_dir) {
        eprintln!("✗ Could not create {}: {e}", data_dir.display());
        return;
    }
    // The hook's start time: history entries before it come from fish_history,
    // everything after it from the live log.
    let since = data_dir.join(FISH_SINCE);
    if !since.exists() {
        if let Err(e) = fs::write(&since, format!("{}\n", history::now_unix())) {
            eprintln!("✗ Could not write {}: {e}", since.display());
            return;
        }
    }

    let content = fs::read_to_string(&config).unwrap_or_default();
    if content.contains(FISH_HOOK_NAME) {
        println!("✓ Logging hook is already present in config.fish");
        return;
    }
    match append(&config, &fish_hook(&data_dir.join(FISH_LIVE))) {
        Ok(()) => println!("✓ Logging hook added to config.fish"),
        Err(e) => eprintln!("✗ Could not update config.fish: {e}"),
    }
}

/// Whether any of the selected shells still drops repeated commands.
pub fn needs_fix(home: &Path, fish: bool, bash: bool, zsh: bool) -> bool {
    let rc_incomplete = |path: &Path, wants: &[Want]| {
        fs::read_to_string(path).is_ok_and(|c| !missing_lines(&c, wants).is_empty())
    };
    let fish_incomplete = || {
        history::config_home(home).join("fish").exists()
            && !history::cmdflow_dir(home).join(FISH_SINCE).exists()
    };
    (bash && rc_incomplete(&home.join(".bashrc"), BASH))
        || (zsh && rc_incomplete(&history::zdotdir(home).join(".zshrc"), ZSH))
        || (fish && fish_incomplete())
}

pub fn fix_shell_histories(home: &Path) {
    println!("{}", ":: Checking shell configurations...".bold().blue());

    let bashrc = home.join(".bashrc");
    if bashrc.exists() {
        ensure(&bashrc, "~/.bashrc", BASH);
    }

    fix_fish(home);

    let zshrc = history::zdotdir(home).join(".zshrc");
    if zshrc.exists() {
        ensure(&zshrc, ".zshrc", ZSH);
    }

    println!(
        "\n{}",
        "Done! Please restart your terminals for changes to take effect."
            .green()
            .bold()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bash_overridden_histcontrol_is_fixed() {
        let rc = "export HISTCONTROL=\nexport HISTCONTROL=ignoredups\n";
        assert!(missing_lines(rc, BASH).contains(&"export HISTCONTROL="));
    }

    #[test]
    fn bash_old_cmdflow_block_gets_missing_lines_only() {
        let rc = "# Added by cmdflow\nexport HISTCONTROL=\nexport PROMPT_COMMAND=\"history -a; $PROMPT_COMMAND\"\n";
        let missing = missing_lines(rc, BASH);
        assert!(!missing.contains(&"export HISTCONTROL="));
        assert!(missing.contains(&"export HISTSIZE=-1"));
        assert!(missing.contains(&"shopt -s histappend"));
    }

    #[test]
    fn bash_complete_config_needs_nothing() {
        let rc = BASH
            .iter()
            .map(|w| match w {
                Want::Assign { line, .. }
                | Want::AtLeast { line, .. }
                | Want::Default { line, .. }
                | Want::Line(line) => *line,
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(missing_lines(&rc, BASH).is_empty());
    }

    #[test]
    fn zsh_respects_existing_histfile_and_big_sizes() {
        let rc = "HISTFILE=~/.histfile\nHISTSIZE=5000000\nSAVEHIST=100\n";
        let missing = missing_lines(rc, ZSH);
        assert!(!missing.contains(&"HISTFILE=~/.zsh_history"));
        assert!(!missing.contains(&"HISTSIZE=1000000"));
        assert!(missing.contains(&"SAVEHIST=1000000"));
    }

    #[test]
    fn fish_quoting() {
        assert_eq!(fish_quote("/home/a b/x"), "'/home/a b/x'");
        assert_eq!(fish_quote("it's"), "'it\\'s'");
    }
}
