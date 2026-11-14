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


## [English README.md](README.md)

cmdflow — это CLI утилита для Linux, которая подсчитывает ваши команды Fish/Bash/Zsh, создаёт топ-N команд и выводит его в терминал.

- Считает каждую команду, включая повторы, которые шеллы обычно выбрасывают
- Помнит историю fish даже после того, как fish удалил из неё дубликаты
- Топ-N с цветными барами
- Удобно для анализа вашей командной истории

---

###   Функционал

- Считает все введённые команды Fish, Bash и Zsh, включая их повторы
- Берёт только первые аргументы команд (например, `cargo build` → `cargo`)
- Видит сквозь обёртки и переменные: `sudo pacman -Syu` и `RUST_LOG=debug cargo run` считаются как `pacman` и `cargo`
- Находит историю где угодно: `HISTFILE` из rc-файлов, `$ZDOTDIR`, `~/.histfile`, `$XDG_DATA_HOME`
- Команды — радужная градиентная аллея

**Пример вывода `cmdflow --working 50`:**

![](img/output.png)

---
⚠️ Важное примечание по поводу дубликатов команд

По умолчанию шеллы записывают не все команды:

- **Fish** всегда удаляет повторы из файла истории
- **Bash** и **Zsh** часто работают в режиме `ignoredups`, а zsh без `SAVEHIST` вообще не сохраняет историю на диск

Если вы введёте `fastfetch` 5 раз подряд, в истории может остаться только один вызов. Чтобы графики cmdflow отражали реальную картину, один раз запустите встроенный фиксер конфигурации:

```bash
cmdflow --fix-history
```

Он допишет блок `# Added by cmdflow` в `~/.bashrc` и `~/.zshrc` (безлимитная история, без фильтра дубликатов) и добавит в `~/.config/fish/config.fish` маленький хук, который записывает каждую команду в `~/.local/share/cmdflow/`. Команды, начинающиеся с пробела, остаются приватными и не записываются. Запускать повторно безопасно: добавится только то, чего не хватает. Пока это не сделано, cmdflow напоминает об этом подсказкой под графиком.

---

###   Установка


**Через AUR:**

Если установлен `yay`:

```bash
yay -S cmdflow
```


**Через GitHub:**

```bash
git clone https://github.com/voideez/cmdflow.git
cd cmdflow
cargo build --release
mkdir -p ~/.local/bin
ln -sf "$(pwd)/target/release/cmdflow" ~/.local/bin/cmdflow
```


---

Теперь команда `cmdflow` доступна в любом терминале:

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

###   Требования

- Cargo
- Хотя бы один из шеллов: Fish, Bash, Zsh

---

###   Разработка

Клонируем проект:

```bash
git clone https://github.com/voideez/cmdflow.git
cd cmdflow
```

Сборка, тесты и запуск в режиме разработки:

```bash
cargo build
cargo test
cargo run
```

После изменений можно обновить команду в терминале:

```bash
cargo build
ln -sf "$(pwd)/target/debug/cmdflow" ~/.local/bin/cmdflow
cmdflow
```

---

###   Выпуск релиза

1. Поднимите `version` в `Cargo.toml`
2. По желанию напишите описание релиза в `.github/releases/vX.Y.Z.md` (первая строка — название релиза)
3. Закоммитьте, затем создайте и запушьте тег:

```bash
git tag vX.Y.Z && git push origin project vX.Y.Z
```

CI проверит, что тег совпадает с `Cargo.toml`, прогонит тесты, опубликует GitHub Release с бинарником и обновит пакет в AUR.
