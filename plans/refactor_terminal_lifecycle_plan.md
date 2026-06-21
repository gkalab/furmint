# Refactor Terminal Lifecycle for ratatui-termina

The app needs to migrate from `crossterm`'s `disable_raw_mode` to dropping and recreating `ratatui`'s `Terminal` when running external programs like editors or shells, in order to properly support the new `ratatui-termina` backend.

## User Review Required

> [!WARNING]
> This requires a significant structural change. Instead of deep handler functions calling `disable_raw_mode()`, we will use an event loop that returns "Pending Actions" so the main entrypoint can drop the terminal, execute the external program, and then recreate the terminal and resume the event loop.

## Proposed Changes

### `src/app.rs`
- Introduce a new `PendingAction` enum to capture actions that need to run outside the terminal:
  ```rust
  pub enum PendingAction {
      OpenEditorLocal(std::path::PathBuf, Option<String>),
      OpenEditorRemote {
          temp_path: std::path::PathBuf,
          remote_path: std::path::PathBuf,
          provider: std::sync::Arc<dyn crate::fs::fs_provider::FileSystemProvider>,
          original_checksum: [u8; 16],
      },
      ToggleConsole,
      WindowsContextMenu(std::path::PathBuf),
  }
  ```
- Replace `pending_context_menu: Option<std::path::PathBuf>` with `pending_action: Option<PendingAction>`.

### `src/event_loop.rs`
- Change `run_event_loop` to return `anyhow::Result<Option<PendingAction>>`.
- Inside the event loop, check for `app.pending_action.take()`. If an action exists, break the loop and return it.
- If `should_exit` is true, return `Ok(None)`.

### `src/handlers/editor.rs`
- Change `handle_edit` to set `app.pending_action` instead of calling `launch_and_wait_for_editor` synchronously with `disable_raw_mode`.
- Move the actual execution and upload logic (for remote files) into separate `pub async fn execute_...` functions that the main loop will call.
- Remove references to `crossterm::terminal::disable_raw_mode` and `enable_raw_mode`.

### `src/handlers/terminal.rs`
- Change `handle_toggle_console` to set `app.pending_action = Some(PendingAction::ToggleConsole)`.
- Extract the actual shell execution into `pub async fn execute_toggle_console(app: &mut AppState)`.
- Remove `crossterm::terminal::disable_raw_mode` calls.

### `src/lib.rs`
- Update `fm::run()` to implement the "Drop, Execute, Recreate" pattern:
  ```rust
  pub async fn run() -> Result<()> {
      // Setup app state...
      loop {
          // 1. Create Terminal
          let mut output = PlatformTerminal::new()?;
          output.enter_raw_mode()?;
          let mut terminal = Terminal::new(TerminaBackend::new(output))?;

          // 2. Run Event Loop
          let action = run_event_loop(&mut terminal, &mut app, ...).await?;

          // 3. Drop Terminal to restore cooked mode
          drop(terminal);

          // 4. Handle Pending Action
          match action {
              Some(PendingAction::OpenEditorLocal(path, name)) => execute_open_editor_local(...).await,
              Some(PendingAction::OpenEditorRemote { ... }) => execute_open_editor_remote(...).await,
              Some(PendingAction::ToggleConsole) => execute_toggle_console(...).await,
              Some(PendingAction::WindowsContextMenu(path)) => show_context_menu(&path),
              None => break, // Normal Quit
          }
      }
      Ok(())
  }
  ```

### `src/main.rs`
- Remove the terminal initialization from `main.rs` since `fm::run()` will handle creating the terminal on each loop iteration.
- Leave only the argument parsing and `fm::run().await` call.

## Verification Plan
1. Build the project to ensure no lifetime or missing import errors.
2. Open a local file in an external editor; verify the terminal suspends and resumes properly.
3. Open a remote file (if possible) to verify temp file handling and checksum logic.
4. Toggle the console terminal; verify it works properly.
