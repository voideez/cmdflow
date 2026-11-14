<h1 align="center">cmdflow🌈</h1>

<p align="center">
  <img src="https://img.shields.io/github/actions/workflow/status/voideez/cmdflow/rust.yml?branch=project&style=for-the-badge&logo=github-actions&label=build" alt="Build">
  <img src="https://img.shields.io/github/repo-size/voideez/cmdflow?style=for-the-badge&logo=gitlab" alt="Repo Size">
  <img src="https://img.shields.io/github/last-commit/voideez/cmdflow?style=for-the-badge&logo=git" alt="Last Commit">
  <img src="https://img.shields.io/badge/Language-Rust-white?style=for-the-badge&logo=rust" alt="Language Rust">
  <img src="https://img.shields.io/github/v/release/voideez/cmdflow?style=for-the-badge&logo=github&label=Version&color=blue" alt="Version">
  <img src="https://img.shields.io/aur/version/cmdflow?style=for-the-badge&logo=archlinux&label=AUR&color=1793d1" alt="AUR">
  <img src="https://img.shields.io/badge/OS-Unix--like-orange?style=for-the-badge&logo=linux" alt="OS">
  <img src="https://img.shields.io/badge/License-MIT-lightgrey?style=for-the-badge&logo=opensourcehardware" alt="License">
</p>

## [Russian README.md](README_ru.md)

cmdflow is a CLI utility for Linux that counts your Fish/Bash/Zsh commands, generates a Top-N list, and displays it beautifully right in your terminal.

- Counts every command, including repeats that shells normally drop
- Remembers fish history even after fish deduplicates it
- Top-N leaderboard with colored progress bars
- Perfect for analyzing your terminal activity

---

### Features

- Counts all entered Fish, Bash, and Zsh commands, including duplicates
- Extracts only the base command (e.g., `cargo build` → `cargo`)
- Sees through wrappers and variables: `sudo pacman -Syu` and `RUST_LOG=debug cargo run` count as `pacman` and `cargo`
- Finds your history wherever it lives: `HISTFILE` from your rc files, `$ZDOTDIR`, `~/.histfile`, `$XDG_DATA_HOME`
- Renders commands in a vibrant rainbow gradient alley

**Example output for ``cmdflow --working 50``:**
![](img/output.png)
---

⚠️ Important Note on Command Duplicates

Shells don't record every command by default:

- **Fish** removes repeated commands from its history file, always
- **Bash** and **Zsh** often run in `ignoredups` mode, and zsh doesn't save history to disk at all unless `SAVEHIST` is set

If you type `fastfetch` 5 times in a row, the history may show it only once. To make cmdflow's charts reflect your real activity, run the built-in configuration fixer once:

```bash
cmdflow --fix-history
```

It appends a `# Added by cmdflow` block to `~/.bashrc` and `~/.zshrc` (unlimited history, no duplicate filtering) and adds a small hook to `~/.config/fish/config.fish` that logs every command to `~/.local/share/cmdflow/`. Commands starting with a space stay private and aren't logged. It's safe to run again: only missing settings are added. cmdflow reminds you with a tip under the chart until this is done.

---

### Installation

**Via AUR:**

If you use yay:

```bash
yay -S cmdflow
```

**From Source (GitHub):**

```bash
git clone https://github.com/voideez/cmdflow.git
cd cmdflow
cargo build --release
mkdir -p ~/.local/bin
ln -sf "$(pwd)/target/release/cmdflow" ~/.local/bin/cmdflow
```

---

Now the cmdflow command is globally available in your terminal:

```bash
cmdflow          # top 10 (fish + bash + zsh)
--fish           # only fish
--bash           # only bash
--zsh            # only zsh
cmdflow 15       # top 15 (fish + bash + zsh)
--fish 20        # top 20 (only fish)
--working        # only working commands
--broken         # only non-working commands
--fix-history    # history fix
```

---

### Requirements

* Cargo
* At least one of: Fish, Bash, Zsh

---

### Development

Clone the repository:

```bash
git clone https://github.com/voideez/cmdflow.git
cd cmdflow
```

Build, test and run in development mode:

```bash
cargo build
cargo test
cargo run
```

To update the local binary after making changes:

```bash
cargo build
ln -sf "$(pwd)/target/debug/cmdflow" ~/.local/bin/cmdflow
cmdflow
```

---

### Releasing

1. Bump `version` in `Cargo.toml`
2. Optionally write release notes in `.github/releases/vX.Y.Z.md` (first line is the release title)
3. Commit, then tag and push:

```bash
git tag vX.Y.Z && git push origin project vX.Y.Z
```

CI checks that the tag matches `Cargo.toml`, runs the tests, publishes a GitHub Release with the binary and updates the AUR package.
