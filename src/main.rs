mod fix_history;
mod history;

use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;

use colored::*;

const FLAGS: &[(&str, &str)] = &[
    ("--fish", "Read only fish commands"),
    ("--bash", "Read only bash commands"),
    ("--zsh", "Read only zsh commands"),
    ("--working", "Show top N working commands"),
    ("--broken", "Show top N broken commands"),
    ("--fix-history", "Make shells record every command, including repeats"),
    ("--version", "Show program version"),
    ("--help", "Show this help message"),
];

const DEFAULT_TOP: usize = 10;
const MAX_CMD_LEN: usize = 12;
const MAX_BAR_LEN: usize = 20;

/// Returns the subset of `cmds` that the system can run.
///
/// Command names are passed to `sh` as arguments, never as part of the script,
/// so history entries can't execute anything. One `sh` call checks them all.
/// If fish is installed, fish builtins and functions are checked too.
fn runnable_commands(cmds: &[String], check_fish: bool) -> HashSet<String> {
    let mut found = HashSet::new();
    if cmds.is_empty() {
        return found;
    }
    let wanted: HashSet<&str> = cmds.iter().map(String::as_str).collect();

    let mut collect = |program: &str, script: &str| {
        let mut command = Command::new(program);
        command.arg("-c").arg(script);
        if program == "sh" {
            command.arg("sh"); // $0
        }
        if let Ok(out) = command.args(cmds).output() {
            for line in String::from_utf8_lossy(&out.stdout).lines() {
                if wanted.contains(line) {
                    found.insert(line.to_string());
                }
            }
        }
    };

    collect(
        "sh",
        r#"for c do command -v "$c" >/dev/null 2>&1 && printf '%s\n' "$c"; done"#,
    );
    if check_fish {
        collect(
            "fish",
            r#"for c in $argv; type -q -- $c; and printf '%s\n' $c; end"#,
        );
    }
    found
}

/// Shortens `s` to `max` characters (not bytes), ending with `…`.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max - 1).collect();
        out.push('…');
        out
    }
}

fn print_top(title: &str, data: &[(String, usize)]) {
    let soft_rainbow = [
        (200, 150, 255), (180, 180, 255), (150, 220, 255),
        (150, 255, 200), (180, 255, 150), (220, 255, 150),
        (255, 220, 150), (255, 180, 150), (255, 150, 180),
    ];

    let max_count = data.first().map(|x| x.1).unwrap_or(1).max(1);

    println!("{}", title.bold().underline());

    for (i, (cmd, count)) in data.iter().enumerate() {
        let bar_len = ((*count as f32 / max_count as f32) * MAX_BAR_LEN as f32).round() as usize;
        let bar = "█".repeat(bar_len.max(1));
        let line = format!(
            "{:>5} │ {:<width$} {}",
            count,
            truncate(cmd, MAX_CMD_LEN),
            bar,
            width = MAX_CMD_LEN
        );
        let c = soft_rainbow[i % soft_rainbow.len()];
        println!("{}", line.truecolor(c.0, c.1, c.2));
    }

    println!();
}

/// Sorts by count (descending), then by name, so equal counts keep a stable order.
fn rank(counter: HashMap<String, usize>) -> Vec<(String, usize)> {
    let mut ranked: Vec<_> = counter.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    ranked
}

fn count_commands<'a>(lines: impl IntoIterator<Item = &'a String>) -> HashMap<String, usize> {
    let mut counter = HashMap::new();
    for line in lines {
        if let Some(key) = history::command_key(line) {
            *counter.entry(key).or_insert(0) += 1;
        }
    }
    counter
}

/// Old versions rebuilt these files on every run; they are no longer used.
fn remove_legacy_logs(data_dir: &Path) {
    for name in ["fish.log", "bash.log", "zsh.log"] {
        let _ = fs::remove_file(data_dir.join(name));
    }
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let has = |flag: &str| args.iter().any(|a| a == flag);

    if has("--help") {
        println!("Usage: cmdflow [N] [OPTIONS]\n");
        println!("  {:<14} How many commands to show (default {DEFAULT_TOP})", "N");
        for (flag, description) in FLAGS {
            println!("  {:<14} {}", flag, description);
        }
        return;
    }

    if let Some(unknown) = args
        .iter()
        .find(|a| a.starts_with('-') && !FLAGS.iter().any(|(flag, _)| flag == &a.as_str()))
    {
        eprintln!("Unknown option: {}", unknown);
        eprintln!("Use --help for usage.");
        std::process::exit(2);
    }

    if has("--version") {
        println!("cmdflow v{}", env!("CARGO_PKG_VERSION"));
        return;
    }

    let home = match dirs::home_dir() {
        Some(h) => h,
        None => {
            eprintln!("Could not determine home directory.");
            std::process::exit(1);
        }
    };

    if has("--fix-history") {
        fix_history::fix_shell_histories(&home);
        return;
    }

    let (working, broken) = (has("--working"), has("--broken"));
    if working && broken {
        eprintln!("--working and --broken can't be used together.");
        std::process::exit(2);
    }

    let top_n = match args.iter().find(|a| !a.starts_with('-')) {
        None => DEFAULT_TOP,
        Some(n) => match n.parse::<usize>() {
            Ok(n) if n > 0 => n,
            _ => {
                eprintln!("Invalid number: {n}. Use a positive number, e.g. cmdflow 20");
                std::process::exit(2);
            }
        },
    };

    // Shell selection
    let (has_fish, has_bash, has_zsh) = (has("--fish"), has("--bash"), has("--zsh"));
    let no_flags = !has_fish && !has_bash && !has_zsh;
    let use_fish = has_fish || no_flags;
    let use_bash = has_bash || no_flags;
    let use_zsh = has_zsh || no_flags;

    let data_dir = history::cmdflow_dir(&home);
    remove_legacy_logs(&data_dir);

    // Fish history is always read so the cache keeps up even when only other
    // shells are selected: fish drops repeated commands over time.
    let fish_lines = history::collect_fish(&history::fish_history_path(&home), &data_dir);

    let mut lines: Vec<String> = Vec::new();
    if use_fish {
        lines.extend(fish_lines);
    }
    if use_bash {
        if let Ok(content) = fs::read_to_string(history::bash_history_path(&home)) {
            lines.extend(history::parse_bash(&content));
        }
    }
    if use_zsh {
        if let Some(bytes) = history::zsh_history_path(&home).and_then(|p| fs::read(p).ok()) {
            lines.extend(history::parse_zsh(&bytes));
        }
    }

    let mut ranked = rank(count_commands(&lines));
    if ranked.is_empty() {
        println!("No commands found for the selected shell(s).");
        return;
    }

    if working || broken {
        let names: Vec<String> = ranked.iter().map(|(c, _)| c.clone()).collect();
        let check_fish = use_fish && Command::new("fish").arg("--version").output().is_ok();
        let runnable = runnable_commands(&names, check_fish);
        ranked.retain(|(cmd, _)| runnable.contains(cmd) == working);
    }
    ranked.truncate(top_n);

    let mut active_shells = Vec::new();
    if use_fish { active_shells.push("fish"); }
    if use_bash { active_shells.push("bash"); }
    if use_zsh { active_shells.push("zsh"); }

    let label = if working {
        "Working Commands"
    } else if broken {
        "Broken Commands"
    } else {
        "Commands"
    };

    if ranked.is_empty() {
        println!("No {} found for the selected shell(s).", label.to_lowercase());
        return;
    }

    print_top(
        &format!("Top {} {} ({})", ranked.len(), label, active_shells.join(" + ")),
        &ranked,
    );

    if fix_history::needs_fix(&home, use_fish, use_bash, use_zsh) {
        println!(
            "{} {} {}",
            "Tip: repeated commands are missing from your history. Run".dimmed(),
            "cmdflow --fix-history".bold(),
            "once.".dimmed()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_is_unicode_safe() {
        assert_eq!(truncate("ls", 12), "ls");
        assert_eq!(truncate("абвгдеёжзийкл", 12), "абвгдеёжзий…");
        assert_eq!(truncate("🦀🦀🦀🦀🦀🦀🦀🦀🦀🦀🦀🦀🦀", 12).chars().count(), 12);
    }

    #[test]
    fn rank_is_stable() {
        let counter = HashMap::from([
            ("b".to_string(), 2),
            ("a".to_string(), 2),
            ("c".to_string(), 5),
        ]);
        let names: Vec<_> = rank(counter).into_iter().map(|(c, _)| c).collect();
        assert_eq!(names, vec!["c", "a", "b"]);
    }

    #[test]
    fn runnable_check_does_not_execute_history() {
        let marker = env::temp_dir().join(format!("cmdflow-pwned-{}", std::process::id()));
        let _ = fs::remove_file(&marker);
        let evil = format!("ls;touch {}", marker.display());
        let found = runnable_commands(&["sh".to_string(), evil.clone()], false);
        assert!(found.contains("sh"));
        assert!(!found.contains(&evil));
        assert!(!marker.exists(), "command from history was executed");
    }
}
