# AGENTS.md

## Project Status
This repository currently contains no source code or configuration files. Please update this file as the project evolves.

## Build, Lint, and Test Commands (for Rust projects)
- Build: `cargo build`
- Lint: `cargo clippy`
- Format: `cargo fmt`
- Test all: `cargo test`
- Test single: `cargo test <test_name>`

## Code Style Guidelines (Rust)
- Use `rustfmt` for formatting; run `cargo fmt` before committing.
- Organize imports: standard, external, then internal crates.
- Prefer explicit types; avoid `unwrap()` in production code.
- Use `Result` and `Option` for error handling; propagate errors with `?`.
- Use snake_case for functions/variables, CamelCase for types/structs, and SCREAMING_SNAKE_CASE for constants.
- Document public items with `///` doc comments.
- Keep functions short and focused; prefer small modules.
- Avoid unused dependencies and dead code.
- **Always fix warnings and remove unused imports before committing.**

## Cursor/Copilot Rules
No Cursor or Copilot rules found. Add them if needed in `.cursor/rules/` or `.github/copilot-instructions.md`.
