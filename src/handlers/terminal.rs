//! Terminal event handlers for opening/spawning terminals and toggling console

use crate::app::AppState;
use crossterm::ExecutableCommand;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::terminal::{Clear, ClearType, disable_raw_mode, enable_raw_mode};
use std::process::Command;

#[cfg(target_os = "linux")]
fn spawn_terminal_linux(
    dir: &std::path::Path,
    configured_terminal: Option<String>,
    args: &[String],
    wrap_shell: bool,
) -> anyhow::Result<()> {
    let shell_trap = |cmdline: String| {
        format!("{cmdline} || (echo; echo 'Command failed. Press Enter to close...'; read)")
    };

    let template_terminals = [
        ("alacritty", vec!["--command"]),
        ("kitty", vec!["sh", "-c"]),
        ("gnome-terminal", vec!["--", "bash", "-c"]),
        ("xfce4-terminal", vec!["--command"]),
        ("konsole", vec!["-e"]),
        ("xterm", vec!["-e"]),
        ("urxvt", vec!["-e"]),
        ("st", vec!["-e"]),
        ("termite", vec!["-e"]),
        ("foot", vec!["-e"]),
        ("x-terminal-emulator", vec!["-e"]),
    ];

    let mut tried_terms = Vec::new();
    let terminals: Vec<(String, Vec<&str>)> = if let Some(term) = configured_terminal {
        let bin = term.trim().to_string();
        // Search for terminal in known list
        let args = template_terminals
            .iter()
            .find(|(name, _)| bin.contains(*name))
            .map(|(_, v)| v.clone())
            .unwrap_or(vec!["-e"]);
        vec![(bin, args)]
    } else {
        template_terminals
            .iter()
            .map(|(n, v)| ((*n).to_string(), v.clone()))
            .collect()
    };

    for (terminal, opt_args) in &terminals {
        tried_terms.push(terminal.clone());
        let mut cmd = Command::new(terminal);
        cmd.current_dir(dir);
        if args.is_empty() {
            // No program: just open an interactive shell/terminal
            if terminal == "alacritty" {
                cmd.args(["--command", "bash"]);
            } else if terminal == "kitty" {
                cmd.args(["sh"]);
            } else if terminal == "gnome-terminal" {
                cmd.args(["--"]);
            } else {
                // fallback, try terminal without extra args
            }
        } else {
            let join_args = |args: &[String]| {
                args.iter()
                    .map(|a| shell_escape::escape(a.into()))
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            if wrap_shell {
                let cmdline = join_args(args);
                let shell_cmd = shell_trap(cmdline);
                if terminal == "alacritty" {
                    cmd.arg("--command").arg("bash").arg("-c").arg(shell_cmd);
                } else if terminal == "kitty" {
                    cmd.args(["sh", "-c", &shell_cmd]);
                } else if terminal == "gnome-terminal" {
                    cmd.args(["--", "bash", "-c", &shell_cmd]);
                } else if terminal == "xfce4-terminal" {
                    // This terminal allows --command, no -e
                    cmd.arg("--command")
                        .arg(format!("bash -c '{}'", shell_cmd.replace('\'', "'\\''")));
                } else {
                    // fallback -e sh -c
                    cmd.arg("-e").arg("bash").arg("-c").arg(shell_cmd);
                }
            } else {
                let joined = args.to_owned();
                if !opt_args.is_empty() {
                    cmd.args(opt_args.clone());
                }
                for arg in joined {
                    cmd.arg(arg);
                }
            }
        }
        // Check if the terminal exists and works
        if Command::new(terminal).arg("--version").output().is_ok()
            || terminal == "x-terminal-emulator"
        {
            match cmd.spawn() {
                Ok(_) => return Ok(()),
                Err(e) => {
                    // On error, propagate the error context back to the caller for UI display
                    return Err(anyhow::anyhow!(format!(
                        "Failed to spawn terminal {terminal}: {e}"
                    )));
                }
            }
        }
    }
    Err(anyhow::anyhow!(format!(
        "No suitable terminal emulator found (tried: {tried_terms:?})"
    )))
}

#[cfg(target_os = "macos")]
fn spawn_terminal_macos(
    dir: &std::path::Path,
    configured_terminal: Option<String>,
    args: &[String],
    wrap_shell: bool,
) -> anyhow::Result<()> {
    if !args.is_empty() {
        let mut cmd = Command::new("open");
        cmd.arg("-a").arg("Terminal").arg("-e");
        if wrap_shell {
            let mut shell_cmd = args
                .iter()
                .map(|a| format!("\"{}\"", a.replace("\"", "\\\"")))
                .collect::<Vec<_>>()
                .join(" ");
            shell_cmd = format!(
                "{} || (echo; echo 'Command failed. Press Enter to close...'; read)",
                shell_cmd
            );
            cmd.arg("bash").arg("-c").arg(shell_cmd);
        } else {
            for arg in args {
                cmd.arg(arg);
            }
        }
        cmd.current_dir(dir).spawn()?;
    } else if let Some(term) = configured_terminal {
        Command::new("open").arg("-a").arg(term).arg(dir).spawn()?;
    } else {
        Command::new("open")
            .arg("-a")
            .arg("Terminal")
            .arg(dir)
            .spawn()?;
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn spawn_terminal_windows(
    dir: &std::path::Path,
    configured_terminal: Option<String>,
    args: &[String],
    wrap_shell: bool,
) -> anyhow::Result<()> {
    if !args.is_empty() {
        // Detect if the target is a GUI application to avoid background terminals
        let is_gui = if args[0].to_lowercase().ends_with(".exe") {
            crate::fs::utils::is_gui_executable(std::path::Path::new(&args[0]))
        } else {
            false
        };

        if is_gui {
            // GUI apps should always use 'start' to launch without a parent terminal window staying open
            let mut cmd = Command::new("cmd");
            cmd.arg("/C").arg("start").arg("");
            for arg in args {
                cmd.arg(arg);
            }
            cmd.current_dir(dir).spawn()?;
            return Ok(());
        }

        if let Some(term) = configured_terminal {
            let mut cmd = Command::new(&term);
            cmd.current_dir(dir);
            let bin = term.to_lowercase();
            if bin.contains("wt") || bin.contains("windows terminal") {
                cmd.arg("-d").arg(".");
                if wrap_shell {
                    cmd.arg("cmd").arg("/K");
                }
            } else if bin.contains("alacritty") {
                cmd.arg("--command");
                if wrap_shell {
                    cmd.arg("cmd").arg("/K");
                }
            } else if bin.contains("powershell") || bin.contains("pwsh") {
                if wrap_shell {
                    cmd.arg("-NoExit");
                }
                cmd.arg("-Command");
            } else {
                // Default fallback for unknown terminal: try to run the command directly
            }
            for arg in args {
                cmd.arg(arg);
            }
            cmd.spawn()?;
        } else {
            let mut cmd = Command::new("cmd");
            // Use 'start' with an empty title to launch in a new window
            cmd.arg("/C").arg("start").arg("");
            if wrap_shell {
                cmd.arg("cmd").arg("/K");
            }
            for arg in args {
                cmd.arg(arg);
            }
            cmd.current_dir(dir).spawn()?;
        }
    } else if let Some(term) = configured_terminal {
        Command::new("cmd")
            .arg("/C")
            .arg("start")
            .arg("")
            .arg(term)
            .current_dir(dir)
            .spawn()?;
    } else {
        Command::new("cmd")
            .arg("/C")
            .arg("start")
            .arg("")
            .arg("cmd")
            .current_dir(dir)
            .spawn()?;
    }
    Ok(())
}

/// Spawns a terminal in the given directory.
///
/// # Errors
///
/// Returns an error if the terminal cannot be spawned.
pub fn spawn_terminal(
    dir: &std::path::Path,
    configured_terminal: Option<String>,
    args: &[String],
    wrap_shell: bool,
) -> anyhow::Result<()> {
    #[cfg(target_os = "linux")]
    return spawn_terminal_linux(dir, configured_terminal, args, wrap_shell);

    #[cfg(target_os = "macos")]
    return spawn_terminal_macos(dir, configured_terminal, args, wrap_shell);

    #[cfg(target_os = "windows")]
    return spawn_terminal_windows(dir, configured_terminal, args, wrap_shell);

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    Err(anyhow::anyhow!("Unsupported OS"))
}

pub fn handle_open_terminal(app: &mut AppState) {
    let current_dir = app.active_tab().current_dir.clone();
    let configured_terminal = app.global.terminal.clone();

    if let Err(e) = spawn_terminal(&current_dir, configured_terminal, &[], false) {
        app.active_tab_mut().error = Some(format!("Error opening terminal: {e}"));
    }
}

/// Toggles the console panel.
///
/// # Errors
///
/// Returns an error if the console cannot be toggled.
pub async fn handle_toggle_console(
    app: &mut AppState,
    input_tx: &tokio::sync::mpsc::UnboundedSender<crossterm::event::Event>,
) -> anyhow::Result<()> {
    // 1. Abort input polling
    if let Some(handle) = app.input_polling_handle.take() {
        handle.abort();
    }

    // 2. Disable raw mode and show cursor
    disable_raw_mode()?;
    std::io::stdout()
        .execute(crossterm::cursor::Show)?
        .execute(Clear(ClearType::All))?;

    if app.global.mouse.unwrap_or(true) {
        std::io::stdout().execute(DisableMouseCapture)?;
    }

    std::io::stdout()
        .execute(crossterm::cursor::MoveTo(0, 0))
        .map_err(|e| anyhow::anyhow!("Failed to reset terminal: {e}"))?;

    // 3. Pause watcher
    let panel_current_dir = app.active_tab().current_dir.clone();
    if let Some(watcher) = &mut app.watcher {
        let paths = watcher.watched_paths();
        for path in &paths {
            let _ = watcher.unwatch(path);
        }
    }

    // 4. Run shell
    println!("\r\n--- Dropping to shell. Type 'exit' to return to fm ---\r\n");
    let result = tokio::task::spawn_blocking({
        let dir = panel_current_dir.clone();
        move || {
            #[cfg(target_os = "windows")]
            let shell = std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string());
            #[cfg(not(target_os = "windows"))]
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string());

            Command::new(&shell).current_dir(dir).status()
        }
    })
    .await;

    let err = match result {
        Ok(Ok(_)) => None,
        Ok(Err(e)) => Some(format!("Error running shell: {e}")),
        Err(e) => Some(format!("Error launching shell: {e}")),
    };

    // 5. Restart watcher
    if let Some(watcher) = &mut app.watcher {
        let _ = watcher.watch(&panel_current_dir);
    }
    app.sync_watcher();

    // 6. Restore raw mode
    enable_raw_mode()?;
    if app.global.mouse.unwrap_or(true) {
        std::io::stdout().execute(EnableMouseCapture)?;
    }

    // 7. Restart input polling
    app.input_polling_handle = Some(crate::event_loop::spawn_input_polling(input_tx.clone()));

    // 8. Refresh all tabs in both panels
    let refresh_tab = |tab: &mut crate::app::Tab| {
        if let Ok(entries) = tab.provider.list_dir(&tab.current_dir) {
            tab.entries = entries;
            tab.sort_entries();
        }
    };

    for tab in &mut app.left.tabs {
        refresh_tab(tab);
    }
    for tab in &mut app.right.tabs {
        refresh_tab(tab);
    }

    app.needs_redraw = true;

    // 8. Return error if any
    if let Some(e) = err {
        Err(anyhow::anyhow!(e))
    } else {
        Ok(())
    }
}
