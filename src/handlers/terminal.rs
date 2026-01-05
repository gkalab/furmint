//! Terminal event handlers for opening/spawning terminals and toggling console

use crate::app::{AppState, PanelSide};
use crossterm::ExecutableCommand;
use crossterm::terminal::{Clear, ClearType, disable_raw_mode, enable_raw_mode};
use std::process::Command;

pub fn spawn_terminal(
    dir: &std::path::Path,
    configured_terminal: Option<String>,
    args: Vec<String>,
    wrap_shell: bool,
) -> anyhow::Result<()> {
    use std::process::Command;

    #[cfg(target_os = "linux")]
    {
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
                    let cmdline = join_args(&args);
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
    {
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
                for arg in &args {
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
    {
        if !args.is_empty() {
            let mut cmd = Command::new("cmd");
            if wrap_shell {
                cmd.arg("/C").arg("start");
            }
            for arg in &args {
                cmd.arg(arg);
            }
            cmd.current_dir(dir).spawn()?;
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

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    Err(anyhow::anyhow!("Unsupported OS"))
}

pub fn handle_open_terminal(app: &mut AppState) {
    let tab_manager = match app.active {
        PanelSide::Left => &mut app.left,
        PanelSide::Right => &mut app.right,
    };
    let current_dir = tab_manager.active_tab().current_dir.clone();
    let configured_terminal = app.global.terminal.clone();

    if let Err(e) = spawn_terminal(&current_dir, configured_terminal, Vec::new(), false) {
        tab_manager.active_tab_mut().error = Some(format!("Error opening terminal: {e}"));
    }
}

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
        .execute(Clear(ClearType::All))?
        .execute(crossterm::cursor::MoveTo(0, 0))
        .map_err(|e| anyhow::anyhow!("Failed to reset terminal: {e}"))?;

    // 3. Pause watcher
    let panel_current_dir = {
        let tab_manager = match app.active {
            PanelSide::Left => &mut app.left,
            PanelSide::Right => &mut app.right,
        };
        let panel = tab_manager.active_tab_mut();
        panel.current_dir.clone()
    };
    if let Some(watcher) = &mut app.watcher {
        let paths = watcher.watched_paths.clone();
        for path in &paths {
            let _ = watcher.unwatch(path);
        }
    }

    // 4. Run shell
    println!("\r\n--- Dropping to shell. Type 'exit' to return to fm ---\r\n");
    let result = tokio::task::spawn_blocking({
        let dir = panel_current_dir.clone();
        move || {
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string());
            Command::new(shell).current_dir(dir).status()
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

    // 7. Restart input polling
    app.input_polling_handle = Some(crate::event_loop::spawn_input_polling(input_tx.clone()));

    // 8. Refresh all tabs in both panels
    let refresh_tab = |tab: &mut crate::app::Tab| {
        if let Ok(entries) = crate::fs_ops::list_dir(&tab.current_dir) {
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
