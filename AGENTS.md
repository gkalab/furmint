# AGENTS.md

## Project Overview
Norton Commander-style TUI file manager in Rust using ratatui + crossterm. Features dual-panel browsing with tabs, SSH/SFTP support, and extensive configuration.

## Architecture
- **Entry**: `src/main.rs` - CLI args → `run()`
- **Core**: `src/event_loop.rs`, `src/app.rs` - Event processing & app state
- **UI**: `src/ui.rs` - TUI rendering
- **FS**: `src/fs_ops.rs`, `src/fs_local.rs`, `src/fs_sftp.rs` - File operations
- **Handlers**: `src/handlers/` - Event handlers
- **State**: `src/state/` - Popup/component states
- **Config**: `src/config.rs` - TOML configuration

## Code Style

### Error Handling
- Use `anyhow::Result<T>` for application errors
- Use error state system for user-facing errors (avoid panics)

### State Management
- Use dedicated state structs for popups (see `src/state/`)
- Use `Option<T>` for optional popup states
- Keep popup state mutable only through designated handlers

### UI Components
- Follow ratatui patterns
- Use theme system for consistent styling
- Implement responsive layouts

### Configuration
- All user options must support TOML configuration
- Support multiple keyboard shortcuts per action using `Vec<String>`

## Project-Specific Conventions

### Keyboard Shortcuts
- Must be configurable through `config.toml`
- Use crossterm key event parsing
- Document new shortcuts in README.md

### SSH/SFTP
- Use `ssh2` crate
- Handle connection errors gracefully

### File Operations
- Use async operations for responsiveness
- Handle file conflicts with user dialogs
- Support both local and remote through unified interface

### Cross-Platform
- Handle Windows drive selection through `DriveSelectState`
- Consider terminal differences in crossterm

## Development Best Practices
- **Always run `cargo clippy --all -- -W clippy::all -W clippy::pedantic` and `cargo fmt` before committing**
- **Fix simple pedantic warnings by running `cargo clippy --fix --allow-dirty --all -- -W clippy::all -W clippy::pedantic` 
- Fix all warnings; treat warnings as errors
- Remove unused imports and dead code
- Add `///` documentation for public APIs
- Keep functions short - extract functionality into smaller functions when feasible
- Use theme system rather than hardcoding colors
- Follow existing popup patterns when adding new dialogs
- Maintain keyboard shortcut consistency

## Common Patterns
- New popups: state in `src/state/`, UI in `src/ui/*_ui.rs`, handlers in `src/handlers/`
- Use clipboard system for copy/paste
- Use fuzzy search system for file selection
