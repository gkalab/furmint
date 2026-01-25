A terminal file manager with dual-panel interface, tabs, and SSH/SFTP support.

## Installation
```bash
cargo install fm
```

## Usage
Run `fm` to start the file manager. Use arrow keys to navigate, Enter to open directories/files.

## Configuration
Create a config file at:
- Linux: `~/.config/fm/config.toml`
- macOS: `~/Library/Application Support/fm/config.toml`
- Windows: `%APPDATA%\fm\config.toml`

Run `fm --create-config` to generate a default configuration.

## Key Features
- Dual-panel interface
- Tabbed browsing per panel
- SSH/SFTP remote connections
- Configurable keyboard shortcuts
- Multiple themes
- File operations (copy, move, delete, rename)

## Default Shortcuts
- `F1`: Help
- `F2`: Rename
- `F4`: Edit
- `F5`: Copy
- `F6`: Move
- `F7`: New directory
- `F9`: Open terminal
- `F10`: Tasks
- `Delete`: Delete
- `Ctrl+Q`: Quit
- `Ctrl+T`: New tab
- `Ctrl+N`: Open SSH connection
- `Tab`: Switch panels
- `Arrow keys`: Navigate

## Example

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