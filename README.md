Furmint (fm), a terminal file manager with dual-panel interface, tabs, and SSH/SFTP support.

[Furmint](resources/demo.png)

## AI Disclosure
This project started out as a vibe coding experiment and AI is heavily used for development.

## State of the project
Although I use it everyday, be aware that this is beta software - don't trust it with any of your data.

## Why
Why yet another file manager? 
I’ve always wanted a cross-platform TUI file manager (Windows/Linux) with a dual-panel interface like Midnight Commander, 
but with tabs and a different, more modern look.
[fman](https://fman.io/) was an inspiration for the GoTo on steroids (Ctrl+P) feature, [Yazi](https://github.com/sxyazi/yazi) for the image preview.

## Alternatives
If you're looking for a stable, open-source, cross-platform, dual-panel file manager, there is
- TUI: [Midnight Commander](https://github.com/MidnightCommander/mc)
- GUI: [Double Commander](https://github.com/doublecmd/doublecmd)

## Key Features
- Single file executable
- Dual-panel interface
- Tabs
- SSH/SFTP remote connections

## Installation

Download a prebuilt binary from the [latest release](https://github.com/gkalab/furmint/releases/latest) and place it in your $PATH.

You can also clone this repository and install with cargo:

```bash
cargo install --path .
```

## Usage
Run `fm` to start Furmint. Use arrow keys to navigate, Enter to open directories/files, Tab to switch panels.

## Configuration
Create a config file at:
- Linux: `~/.config/fm/config.toml`
- Windows: `%APPDATA%\fm\config.toml`

Run `fm --create-config` to generate a default configuration.

## Main Default Shortcuts
- `Arrow keys`: Navigate
- `Tab`: Switch panels
- `F1`: Help
- `F2`: Rename
- `F3`: File viewer
- `F4`: Edit
- `F5`: Copy
- `F6`: Move
- `F7`: New directory
- `F9`: Open terminal
- `Delete`: Delete
- `Ctrl+Q`: Quit
- `Ctrl+P`: Search/select recently used directories
- `Ctrl+T`: New tab
- `Ctrl+N`: Open SSH connection

## Example Configuration

#### Example
```toml
[global]
theme = "mariana"
borders = true
icons = true

[editor]
command = "nvim"
in_terminal = true

[viewer]
command = "evince"
in_terminal = false

[keyboard]
quit = ["Ctrl-q"]
edit_file = ["F4"]
copy_to = ["F5"]
move_to = ["F6"]
open_ssh = ["Ctrl-n"]
```

#### Editor/Viewer configuration
- For CLI editors, set `in_terminal = true`.
- For graphical editors/viewers, set `in_terminal = false`.

#### Keyboard configuration
- You can assign multiple shortcuts to the same action (e.g., `quit = ["Ctrl-q", "Esc"]`).
- Shortcut format: `Ctrl-q`, `Esc`, `Ctrl-Up`, `Left`, `Ctrl-Down`, `Alt-Right`, etc.

## Supported Themes
- **Catppuccin** (dark): `catppuccin macchiato`, `catppuccin frappe`, `catppuccin mocha`
- **Catppuccin** (light): `catppuccin latte`
- **Dracula** (dark): `dracula`
- **Nord** (dark): `nord`
- **Mariana** (dark): `mariana`
- **Solarized** (light): `solarized light`
- **Breakers** (light): `breakers`