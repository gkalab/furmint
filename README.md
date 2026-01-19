Here’s a detailed summary so future work can seamlessly continue:

Project Context

You are refactoring and extending a Norton Commander-style TUI file manager written in Rust using ratatui for the TUI and crossterm for terminal input handling. 
It provides a dual-panel file browser with tabs, SSH/SFTP support, and various file operations.

The main files involved are:
- src/main.rs (entry point, sets up app state and delegates to event loop)
- src/event_loop.rs (main event loop, event polling, event handling, split by action)
- src/app.rs (application state structs)
- src/fs_ops.rs (filesystem operations, file entry type)
- src/ui.rs (UI rendering: panels, table, status bar, scroll bar)
- src/panel_status.rs (status bar drawing)

Key Dependencies:
- ratatui (0.30) - TUI rendering
- crossterm (0.29) - Terminal input/events
- tokio (1.48) - Async runtime
- notify/notify-debouncer-full - File system watching
- ssh2 - SSH/SFTP support
- Various utilities for fuzzy matching, syntax highlighting, etc.

---

## Tab Management

The file manager supports multiple tabs per panel, allowing you to work with multiple directories simultaneously.

### Tab Features
- **Multiple tabs per panel**: Each panel (left and right) can have multiple tabs
- **Independent state**: Each tab maintains its own directory, cursor position, and navigation history
- **Tab bar display**: Tab titles show the last component of the directory path (truncated if needed)
- **Visual indicators**: Active tab is highlighted based on panel focus

### Tab Keyboard Shortcuts
- **Ctrl+T**: Open new tab in active panel (same directory and cursor position as current tab)
- **Alt+Right**: Switch to next tab (cycles: Tab1→Tab2→Tab3→Tab1)
- **Alt+Left**: Switch to previous tab (cycles: Tab3→Tab2→Tab1→Tab3)
- **Ctrl+W**: Close current tab (minimum 1 tab per panel)
- **Ctrl+U**: Swap current active and passive tabs

---

## Windows Drive Selection (Windows only)

On Windows, you can quickly change the drive for either panel using dedicated shortcuts.

### Features
- **Drive Dropdown**: Open a list of all available system drives (A:, C:, D:, etc.)
- **Side Specific**: Change drives independently for the left or right panel
- **Responsive**: Enter to select, Esc to cancel

### Keyboard Shortcuts
- **Alt+F1**: Open drive selection for the **left** panel
- **Alt+F2**: Open drive selection for the **right** panel
- **Arrow keys**: Navigate drive list
- **Drive Letters (A, C, D, etc.)**: Switch and close immediately
- **Enter**: Switch to selected drive
- **Esc**: Close dropdown without changing drive
- **Tab**: Switch between panels (existing functionality)

---

### Command Line Options

Support for basic maintenance and information:
- `--version`: Display the application version and exit.
- `--create-config`: Generate a default configuration file at the system's default location (fails if it already exists).

### Config File Location
- **Linux:** `~/.config/fm/config.toml`
- **macOS:** `~/Library/Application Support/fm/config.toml`
- **Windows:** `%APPDATA%\fm\config.toml`

### Example `config.toml`
```toml
[global]
theme = "mariana"
borders = false
# Optional: Configure default terminal to launch
# terminal = "gnome-terminal"

[editor]
command = "nvim"
in_terminal = true # Launch nvim in terminal (CLI editor)

[viewer]
command = "evince"
in_terminal = false # Launch evince directly (do NOT use terminal)

[keyboard]
new_file = ["Shift-F4"]
quit = ["Ctrl-q"]
forward = ["Shift-Right"]
back = ["Shift-Left"]
enter_dir = ["Right"]
up_dir = ["Backspace", "Left"]
edit_file = ["F4"]
new_tab = ["Ctrl-t"]
tab_next = ["Alt-Right"]
tab_prev = ["Alt-Left"]
tab_close = ["Ctrl-w"]
search = ["Ctrl-p"]
sort_name = ["Ctrl-F2"]
sort_ext = ["Ctrl-F4"]
sort_date = ["Ctrl-F5"]
sort_size = ["Ctrl-F6"]
copy_to = ["F5"]
move_to = ["F6"]
rename = ["F2"]
delete = ["Delete"]
delete_force = ["Shift-Delete"]
empty_trash = ["Ctrl-F8"]
tasks = ["F10"]
new_dir = ["F7"]
select_all = ["Ctrl-a"]
open_terminal = ["F9"]
toggle_console = ["Ctrl-o"]
swap_tabs = ["Ctrl-u"]
help = ["F1"]
change_drive_left = ["Alt-F1"]
change_drive_right = ["Alt-F2"] 
open_ssh = ["Ctrl-n"]
reconnect_ssh = ["Ctrl-r"]
```

- For CLI editors (like `nvim`, `vim`, `nano`, `less`), set `in_terminal = true`.
- For graphical editors/viewers (like `gedit`, `evince`, `code`, `okular`), set `in_terminal = false`.
- If in_terminal is omitted, the default is `true` (conservative, for backward compatibility).

- You can assign multiple shortcuts to the same action (e.g., `quit = ["Ctrl-q", "Esc"]`).
- Shortcut format: `Ctrl-q`, `Esc`, `Ctrl-Up`, `Left`, `Ctrl-Down`, `Alt-Right`, etc.
- Supported values include any key combination usable elsewhere in the config.
- Supported themes:
  - **Catppuccin** (dark): `catppuccin macchiato`, `catppuccin frappe`, `catppuccin mocha`
  - **Catppuccin** (light): `catppuccin latte`
  - **Dracula** (dark): `dracula`
  - **Nord** (dark): `nord`
  - **Mariana** (dark): `mariana`
  - **Solarized** (light): `solarized light`
  - **Breakers** (light): `breakers`

### Behavior
- If the config file is missing, the app uses sensible defaults (as above).
- If the config file is present but invalid, the app prints an error and exits.
- Keyboard shortcuts and theme are loaded at startup and used throughout the app.

For future work:
- Continue to keep modules focused and maintainable.
- Add more tests and documentation as features grow.
- Consider further UI/UX improvements and performance profiling as needed.

### Clipboard functionality

#### Known Limitations on Windows
* Remote paths to Explorer: Direct "Paste" into Windows Explorer for remote files (SSH) currently does nothing because Explorer requires actual local file paths.

### TODO
- Refactoring
