//! Terminal event handlers for opening/spawning terminals and toggling console

use crate::app::AppState;
use crate::handlers::suspended_ui::SuspendedUi;
use directories::UserDirs;
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::process::Command;
#[cfg(target_os = "windows")]
use std::process::Stdio;

use std::io::Write;

/// Enables SGR mouse capture mode.
///
/// # Errors
///
/// Returns an error if writing to stdout fails.
pub fn enable_mouse_capture() -> std::io::Result<()> {
    write!(std::io::stdout(), "\x1b[?1000h\x1b[?1002h\x1b[?1006h")?;
    std::io::stdout().flush()
}

/// Disables SGR mouse capture mode.
///
/// # Errors
///
/// Returns an error if writing to stdout fails.
pub fn disable_mouse_capture() -> std::io::Result<()> {
    write!(std::io::stdout(), "\x1b[?1006l\x1b[?1002l\x1b[?1000l")?;
    std::io::stdout().flush()
}

#[cfg(target_os = "linux")]
fn spawn_terminal_linux(
    dir: &std::path::Path,
    configured_terminal: Option<String>,
    args: &[String],
    wrap_shell: bool,
    sshpass: Option<&secrecy::SecretString>,
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
            if let Some(pw) = &sshpass {
                use secrecy::ExposeSecret;
                cmd.env("SSHPASS", pw.expose_secret());
            }
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
    _sshpass: Option<&secrecy::SecretString>,
) -> anyhow::Result<()> {
    if !args.is_empty() {
        let mut cmd = Command::new("open");
        cmd.arg("-a").arg("Terminal").arg("-e");
        if wrap_shell {
            let joined = args
                .iter()
                .map(|a| shell_escape::escape(a.into()))
                .collect::<Vec<_>>()
                .join(" ");
            let trap = " || (echo; echo 'Command failed. Press Enter to close...'; read)";
            let esc = |s: &str| s.replace('\'', "'\\''");
            let shell_cmd = format!("'{}{}'", esc(joined), esc(trap));
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
    _sshpass: Option<&secrecy::SecretString>,
) -> anyhow::Result<()> {
    let mut cmd = if !args.is_empty() {
        // Detect if the target is a GUI application to avoid background terminals
        let is_gui = if args[0].to_lowercase().ends_with(".exe") {
            crate::fs::utils::is_gui_executable(std::path::Path::new(&args[0]))
        } else {
            false
        };

        if is_gui {
            // GUI apps should always use 'start' to launch without a parent terminal window staying open.
            let mut cmd = Command::new("cmd");
            cmd.arg("/C").arg("start").arg("");
            for arg in args {
                cmd.arg(arg);
            }
            cmd
        } else if let Some(term) = configured_terminal {
            let mut cmd = Command::new(&term);
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
            }
            for arg in args {
                cmd.arg(arg);
            }
            cmd
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
            cmd
        }
    } else if let Some(term) = configured_terminal {
        let mut cmd = Command::new("cmd");
        cmd.arg("/C").arg("start").arg("").arg(term);
        cmd
    } else {
        let mut cmd = Command::new("cmd");
        cmd.arg("/C").arg("start").arg("").arg("cmd");
        cmd
    };

    cmd.current_dir(dir)
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW (0x08000000) for the intermediate 'cmd /C' to avoid a flash
        .stdin(Stdio::null()) // Stdio::null() to avoid inheriting the parent console's handles
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;

    Ok(())
}

/// Spawns a terminal in the given directory.
///
/// `sshpass` is an optional `SSHPASS` environment value to set on the spawned
/// terminal's environment (used for `sshpass -e` password authentication).
/// Only applied on Linux; on macOS and Windows authentication is interactive.
///
/// # Errors
///
/// Returns an error if the terminal cannot be spawned.
pub fn spawn_terminal(
    dir: &std::path::Path,
    configured_terminal: Option<String>,
    args: &[String],
    wrap_shell: bool,
    sshpass: Option<&secrecy::SecretString>,
) -> anyhow::Result<()> {
    #[cfg(target_os = "linux")]
    return spawn_terminal_linux(dir, configured_terminal, args, wrap_shell, sshpass);

    #[cfg(target_os = "macos")]
    return spawn_terminal_macos(dir, configured_terminal, args, wrap_shell, sshpass);

    #[cfg(target_os = "windows")]
    return spawn_terminal_windows(dir, configured_terminal, args, wrap_shell, sshpass);

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    Err(anyhow::anyhow!("Unsupported OS"))
}

fn get_terminal_working_dir(app: &AppState) -> std::path::PathBuf {
    let tab = app.active_tab();
    if tab.provider.is_local() {
        tab.current_dir.clone()
    } else {
        UserDirs::new().map_or_else(
            || std::path::PathBuf::from("."),
            |u| u.home_dir().to_path_buf(),
        )
    }
}

/// Returns `true` if the given command is available on the system PATH.
fn is_command_available(command: &str) -> bool {
    #[cfg(target_os = "windows")]
    {
        Command::new("where")
            .arg(command)
            .output()
            .is_ok_and(|o| o.status.success())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Command::new("sh")
            .arg("-c")
            .arg(format!("command -v {command} >/dev/null 2>&1"))
            .output()
            .is_ok_and(|o| o.status.success())
    }
}

/// Adds the `sshpass` prefix to `args` and returns the `SSHPASS` secret.
///
/// On Linux, when a cached password is available for the provider and `sshpass`
/// is installed, prepends `sshpass -e` to `args` and returns the password to be
/// exported as `SSHPASS` (kept out of the process list).
#[cfg(target_os = "linux")]
fn sshpass_args(
    args: &mut Vec<String>,
    provider: &dyn crate::fs::fs_provider::FileSystemProvider,
) -> Option<secrecy::SecretString> {
    if let Some(pw) = provider.get_password()
        && is_command_available("sshpass")
    {
        args.push("sshpass".to_string());
        args.push("-e".to_string());
        Some(pw)
    } else {
        None
    }
}

#[cfg(not(target_os = "linux"))]
fn sshpass_args(
    _args: &mut Vec<String>,
    _provider: &dyn crate::fs::fs_provider::FileSystemProvider,
) -> Option<secrecy::SecretString> {
    None
}

/// Builds the argument list for an `ssh` command that opens an interactive
/// shell on `user@host` in the remote directory `dir`.
#[must_use]
fn build_ssh_args(user: &str, host: &str, port: u16, dir: &std::path::Path) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();
    args.push("ssh".to_string());
    // Allocate a TTY since we run a command on the remote side.
    args.push("-t".to_string());
    if port != 22 {
        args.push("-p".to_string());
        args.push(port.to_string());
    }
    args.push(format!("{user}@{host}"));

    // Remote command to start an interactive shell in the current directory.
    let dir_str = crate::fs::utils::normalize_sftp_path(dir);
    let quoted_dir = format!("'{}'", dir_str.replace('\'', "'\\''"));
    let remote_cmd = format!("cd {quoted_dir} && exec \"${{SHELL:-/bin/sh}}\"");
    args.push(remote_cmd);
    args
}

/// Builds an SSH command for opening a terminal on the remote host of the
/// active tab. Returns `None` if the tab is not a remote connection, `ssh` is
/// not available, or the connection lacks host/user info.
///
/// On Linux, when a cached password is available and `sshpass` is installed,
/// the password is passed via the `SSHPASS` environment variable.
struct SshTerminalPlan {
    args: Vec<String>,
    /// SSHPASS value to set on the child process environment.
    sshpass: Option<secrecy::SecretString>,
}

fn build_ssh_terminal_plan(app: &AppState) -> Option<SshTerminalPlan> {
    let tab = app.active_tab();
    if tab.provider.is_local() {
        return None;
    }

    let host = tab.provider.get_host()?.to_string();
    let user = tab.provider.get_user()?.to_string();
    let port = tab.provider.get_port();

    if !is_command_available("ssh") {
        return None;
    }

    let mut args: Vec<String> = Vec::new();
    let sshpass = sshpass_args(&mut args, tab.provider.as_ref());
    args.extend(build_ssh_args(&user, &host, port, &tab.current_dir));

    Some(SshTerminalPlan { args, sshpass })
}

pub fn handle_open_terminal(app: &mut AppState) {
    let current_dir = get_terminal_working_dir(app);
    let configured_terminal = app.global.terminal.clone();

    if let Some(plan) = build_ssh_terminal_plan(app) {
        let sshpass = plan.sshpass;
        #[cfg(target_os = "windows")]
        let wrap = false;
        #[cfg(not(target_os = "windows"))]
        let wrap = true;
        if let Err(e) = spawn_terminal(
            &current_dir,
            configured_terminal,
            &plan.args,
            wrap,
            sshpass.as_ref(),
        ) {
            app.active_tab_mut().error = Some(format!("Error opening terminal: {e}"));
        }
        return;
    }

    if let Err(e) = spawn_terminal(&current_dir, configured_terminal, &[], false, None) {
        app.active_tab_mut().error = Some(format!("Error opening terminal: {e}"));
    }
}

/// Toggles the console panel.
///
/// # Errors
///
/// Returns an error if the console cannot be toggled.
pub async fn handle_toggle_console(app: &mut AppState) -> anyhow::Result<()> {
    app.pending_action = Some(crate::app::PendingAction::ToggleConsole);
    Ok(())
}

/// Executes the console toggle: drops to a shell and restores the TUI on exit.
///
/// # Errors
///
/// Returns an error if the shell command or terminal restoration fails.
pub async fn execute_toggle_console(app: &mut AppState) -> anyhow::Result<()> {
    async fn refresh_tab(tab: &mut crate::app_state::tabs::Tab) {
        if let Ok(entries) = tab.provider.list_dir(&tab.current_dir).await {
            tab.entries = entries;
            tab.sort_entries();
        }
    }

    // 1. Suspend the TUI (input polling, screen, mouse, watcher)
    let mut suspended = SuspendedUi::enter_cleared(app);

    // 2. Run shell
    println!("\r\n--- Dropping to shell. Type 'exit' to return to fm ---\r\n");
    let result = tokio::task::spawn_blocking({
        let dir = get_terminal_working_dir(app);
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

    // 3. Restore TUI state (watcher, mouse)
    suspended.restore(app);

    // 4. Refresh all tabs in both panels
    for tab in &mut app.panels.left.tabs {
        refresh_tab(tab).await;
    }
    for tab in &mut app.panels.right.tabs {
        refresh_tab(tab).await;
    }

    // 5. Return error if any
    if let Some(e) = err {
        Err(anyhow::anyhow!(e))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::create_test_app;
    use async_trait::async_trait;

    struct MockRemoteFs {
        host: Option<String>,
        user: Option<String>,
        port: u16,
    }

    #[async_trait]
    impl crate::fs::fs_provider::FileSystemProvider for MockRemoteFs {
        fn is_local(&self) -> bool {
            false
        }

        fn get_host(&self) -> Option<&str> {
            self.host.as_deref()
        }

        fn get_user(&self) -> Option<&str> {
            self.user.as_deref()
        }

        fn get_port(&self) -> u16 {
            self.port
        }
        async fn list_dir(
            &self,
            _: &std::path::Path,
        ) -> anyhow::Result<Vec<crate::fs::utils::FileEntry>> {
            Ok(vec![])
        }
        async fn create_dir(&self, _: &std::path::Path) -> anyhow::Result<()> {
            Ok(())
        }
        async fn create_dir_all(&self, _: &std::path::Path) -> anyhow::Result<()> {
            Ok(())
        }
        async fn create_file(&self, _: &std::path::Path) -> anyhow::Result<()> {
            Ok(())
        }
        async fn delete(&self, _: &std::path::Path, _: bool) -> anyhow::Result<()> {
            Ok(())
        }
        async fn rename(&self, _: &std::path::Path, _: &std::path::Path) -> anyhow::Result<()> {
            Ok(())
        }
        async fn read_file(&self, _: &std::path::Path) -> anyhow::Result<Vec<u8>> {
            Ok(vec![])
        }
        async fn read_file_at(
            &self,
            _: &std::path::Path,
            _: u64,
            _: usize,
        ) -> anyhow::Result<Vec<u8>> {
            Ok(vec![])
        }
        async fn write_file(&self, _: &std::path::Path, _: &[u8]) -> anyhow::Result<()> {
            Ok(())
        }
        async fn write_file_at(&self, _: &std::path::Path, _: u64, _: &[u8]) -> anyhow::Result<()> {
            Ok(())
        }
        fn display_prefix(&self) -> &'static str {
            ""
        }
        async fn exists(&self, _: &std::path::Path) -> bool {
            true
        }
        async fn is_dir(&self, _: &std::path::Path) -> bool {
            true
        }
        async fn canonicalize(&self, path: &std::path::Path) -> anyhow::Result<std::path::PathBuf> {
            Ok(path.to_path_buf())
        }
        async fn get_file_info(
            &self,
            _: &std::path::Path,
        ) -> Option<crate::fs::fs_provider::FileMetadata> {
            None
        }
        async fn get_permissions(&self, _: &std::path::Path) -> Option<u32> {
            None
        }
        async fn set_permissions(&self, _: &std::path::Path, _: u32) -> bool {
            false
        }
        async fn get_modified_time(&self, _: &std::path::Path) -> Option<std::time::SystemTime> {
            None
        }
        async fn set_modified_time(&self, _: &std::path::Path, _: std::time::SystemTime) -> bool {
            false
        }
        fn context_key(&self) -> crate::fs::fs_provider::ContextKey {
            crate::fs::fs_provider::ContextKey::Ssh {
                user: self.user.clone().unwrap_or_default(),
                host: self.host.clone().unwrap_or_default(),
                port: self.port,
            }
        }
        fn display_path(&self, path: &std::path::Path) -> String {
            path.to_string_lossy().to_string()
        }
        async fn calc_dir_size(&self, _: &std::path::Path) -> anyhow::Result<u64> {
            Ok(0)
        }
    }

    #[test]
    fn test_get_terminal_working_dir_local() {
        let mut app = create_test_app();
        app.active_tab_mut().current_dir = std::path::PathBuf::from("/some/local/path");
        let dir = get_terminal_working_dir(&app);
        assert_eq!(dir, std::path::PathBuf::from("/some/local/path"));
    }

    #[test]
    fn test_get_terminal_working_dir_remote() {
        let mut app = create_test_app();
        app.active_tab_mut().provider = std::sync::Arc::new(MockRemoteFs {
            host: None,
            user: None,
            port: 22,
        });
        let dir = get_terminal_working_dir(&app);
        let home = UserDirs::new().unwrap().home_dir().to_path_buf();
        assert_eq!(dir, home);
    }

    #[test]
    fn test_build_ssh_args_default_port() {
        let args = build_ssh_args("user", "example.com", 22, std::path::Path::new("/home/g"));
        assert_eq!(
            args,
            vec![
                "ssh",
                "-t",
                "user@example.com",
                "cd '/home/g' && exec \"${SHELL:-/bin/sh}\"",
            ]
        );
    }

    #[test]
    fn test_build_ssh_args_custom_port() {
        let args = build_ssh_args("g", "h", 2222, std::path::Path::new("/tmp"));
        assert_eq!(
            args,
            vec![
                "ssh",
                "-t",
                "-p",
                "2222",
                "g@h",
                "cd '/tmp' && exec \"${SHELL:-/bin/sh}\""
            ]
        );
    }

    #[test]
    fn test_build_ssh_args_dir_with_space() {
        let args = build_ssh_args("g", "h", 22, std::path::Path::new("/home/user/my docs"));
        assert_eq!(
            args.last().unwrap(),
            "cd '/home/user/my docs' && exec \"${SHELL:-/bin/sh}\""
        );
    }

    #[test]
    fn test_build_ssh_args_dir_with_single_quote() {
        let args = build_ssh_args("g", "h", 22, std::path::Path::new("/home/user/it's dir"));
        assert_eq!(
            args.last().unwrap(),
            "cd '/home/user/it'\\''s dir' && exec \"${SHELL:-/bin/sh}\""
        );
    }

    #[test]
    fn test_build_ssh_terminal_plan_local() {
        let app = create_test_app();
        assert!(build_ssh_terminal_plan(&app).is_none());
    }

    #[test]
    fn test_build_ssh_terminal_plan_missing_host_info() {
        let mut app = create_test_app();
        app.active_tab_mut().provider = std::sync::Arc::new(MockRemoteFs {
            host: None,
            user: Some("g".to_string()),
            port: 22,
        });
        assert!(build_ssh_terminal_plan(&app).is_none());
    }
}
