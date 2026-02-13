# leftysay

`leftysay` is a cross-platform CLI greeter that prints a wrapped speech bubble plus an image rendered to the terminal via `chafa`.

## Requirements

- `chafa` installed and available in `PATH`.

Install hints:
- Debian/Ubuntu: `sudo apt install chafa`
- Arch: `sudo pacman -S chafa`
- macOS: `brew install chafa`

## Install

System install (Linux/macOS with GNU install):

```bash
make install
```

User install (no sudo):

```bash
make install PREFIX="$HOME/.local"
```

This installs the binary to `$(PREFIX)/bin` and packs to `$(PREFIX)/share/leftysay/packs`, which matches the pack search paths.

## Usage

```bash
leftysay
leftysay --text "Hello" --pack default
leftysay --image /path/to/pic.jpg --no-bubble
leftysay --list
leftysay --doctor
leftysay --text "$(fortune)"
fortune -a | leftysay
```

If `leftysay` receives text on stdin (piped), it uses that as the message when `--text` is not provided.

## Config

Config file: `~/.config/leftysay/config.toml`

```toml
enabled = true
default_pack = "default"
format = "auto" # use "symbols" if your chafa does not support "auto"
colors = "auto"
max_height_ratio = 0.55
bubble_style = "classic"
cache = true
cache_max_mb = 64
animate = false
```

CLI flags take precedence over config, then defaults.

Available format values: `auto`, `symbols`, `kitty`, `iterm`, `sixels`.
Available color values: `auto`, `full`, `256`, `16`.

See `config.example.toml` for a ready-to-copy config.

## Run On Terminal Startup

Bash (`~/.bashrc`):

```bash
if command -v leftysay >/dev/null 2>&1; then
  leftysay --text "$(fortune)"
fi
```

Zsh (`~/.zshrc`):

```bash
if command -v leftysay >/dev/null 2>&1; then
  leftysay --text "$(fortune)"
fi
```

Fish (`~/.config/fish/config.fish`):

```fish
if type -q leftysay
  leftysay --text (fortune)
end
```

## Packs

Packs are searched in:
- `~/.local/share/leftysay/packs/`
- `/usr/share/leftysay/packs/` (Linux)
- `$(brew --prefix)/share/leftysay/packs/` (macOS)
- `./packs` (for local development)

Each pack contains:

```
pack.toml
images/
messages.txt (optional)
LICENSES/ (optional)
```

Example `pack.toml`:

```toml
name = "default"
version = "0.1.0"
license = "CC0-1.0"
description = "Safe default pack"
images_dir = "images"
```

Default pack ships Kenney's platformer character sprites (from the Kenney Platformer Characters pack).

## License

This project is MIT licensed. See `LICENSE`.
