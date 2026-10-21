//! Terminal event handlers for opening/spawning terminals and toggling console

use crate::app::AppState;
use crate::handlers::suspended_ui::SuspendedUi;
use directories::UserDirs;
use std::process::Command;

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

/// Returns `true` if `name` resolves to an executable file: either a path
/// (containing a separator) to an existing executable, or a bare command name
/// found on `PATH`.
///
/// Uses plain stat calls only, never spawns a process, so it is safe to call
/// from the UI thread in a loop.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn terminal_available(name: &str) -> bool {
    let path = std::path::Path::new(name);
    #[cfg(unix)]
    {
        if name.contains('/') {
            return is_executable(path);
        }
    }
    #[cfg(windows)]
    {
        if name.contains('/') || name.contains('\\') {
            return path.is_file();
        }
    }
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&paths).any(|dir| {
        let candidate = dir.join(name);
        #[cfg(windows)]
        {
            // Try the bare name plus each PATHEXT extension (.exe, .cmd, ...).
            if candidate.is_file() {
                return true;
            }
            let exts: Vec<String> = std::env::var("PATHEXT")
                .ok()
                .map(|p| {
                    p.split(';')
                        .filter(|e| !e.is_empty())
                        .map(str::to_lowercase)
                        .collect()
                })
                .unwrap_or_default();
            exts.iter()
                .any(|ext| dir.join(format!("{name}{ext}")).is_file())
        }
        #[cfg(not(windows))]
        is_executable(&candidate)
    })
}

/// Returns `true` if `path` is a regular file with any executable bit set.
#[cfg(unix)]
fn is_executable(path: &std::path::Path) -> bool {
    if let Ok(meta) = std::fs::metadata(path) {
        use std::os::unix::fs::PermissionsExt;
        meta.is_file() && (meta.permissions().mode() & 0o111) != 0
    } else {
        false
    }
}

/// Spawns `cmd` detached from the UI loop and reaps it on exit so a closed
/// terminal does not linger as a zombie process on Unix. The reaper thread
/// exits together with the child.
#[cfg_attr(not(any(target_os = "linux", target_os = "macos")), allow(dead_code))]
fn spawn_detached(cmd: &mut Command) -> std::io::Result<()> {
    #[cfg(not(target_os = "windows"))]
    {
        let mut child = cmd.spawn()?;
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(())
    }
    #[cfg(target_os = "windows")]
    {
        // Windows has no zombie processes; dropping the Child handle is fine.
        cmd.spawn()?;
        Ok(())
    }
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
        // Cheap existence check without spawning the terminal itself
        // (a `--version` probe would run the real binary on the UI thread,
        // which is slow for GUI terminals and may even open a window).
        if !terminal_available(terminal) {
            continue;
        }
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
        if let Some(pw) = &sshpass {
            use secrecy::ExposeSecret;
            cmd.env("SSHPASS", pw.expose_secret());
        }
        match spawn_detached(&mut cmd) {
            Ok(()) => return Ok(()),
            Err(e) => {
                // On error, propagate the error context back to the caller for UI display
                return Err(anyhow::anyhow!(format!(
                    "Failed to spawn terminal {terminal}: {e}"
                )));
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
        cmd.current_dir(dir);
        spawn_detached(&mut cmd)?;
    } else if let Some(term) = configured_terminal {
        let mut cmd = Command::new("open");
        cmd.arg("-a").arg(term).arg(dir);
        spawn_detached(&mut cmd)?;
    } else {
        let mut cmd = Command::new("open");
        cmd.arg("-a").arg("Terminal").arg(dir);
        spawn_detached(&mut cmd)?;
    }
    Ok(())
}

/// Quotes `arg` so that it survives `CommandLineToArgvW` parsing, producing
/// the same quoting `std::process::Command` applies to each argument.
#[cfg(target_os = "windows")]
fn quote_windows_arg(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
        return arg.to_string();
    }
    let mut quoted = String::with_capacity(arg.len() + 2);
    quoted.push('"');
    let mut backslashes = 0usize;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                // Backslashes before a quote must be escaped, plus the quote.
                quoted.push_str(&"\\".repeat(backslashes * 2 + 1));
                quoted.push('"');
                backslashes = 0;
            }
            _ => {
                quoted.push_str(&"\\".repeat(backslashes));
                backslashes = 0;
                quoted.push(c);
            }
        }
    }
    // Backslashes before the closing quote must be escaped as well.
    quoted.push_str(&"\\".repeat(backslashes * 2));
    quoted.push('"');
    quoted
}

/// Joins `args` into a single Windows command line.
#[cfg(target_os = "windows")]
fn join_windows_args(args: &[String]) -> String {
    args.iter()
        .map(|a| quote_windows_arg(a))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Quotes `arg` as a PowerShell single-quoted literal, so PowerShell passes it
/// to the invoked program verbatim.
#[cfg(target_os = "windows")]
fn quote_powershell_arg(arg: &str) -> String {
    format!("'{}'", arg.replace('\'', "''"))
}

/// Builds a PowerShell script that invokes `args` as one command, keeping every
/// argument intact (spaces, quotes and `&` characters included).
#[cfg(target_os = "windows")]
fn build_powershell_script(args: &[String]) -> String {
    let mut script = String::from("&");
    for arg in args {
        script.push(' ');
        script.push_str(&quote_powershell_arg(arg));
    }
    script
}

/// Encodes a PowerShell script for `-EncodedCommand` (base64 of UTF-16LE).
///
/// `-EncodedCommand` is used instead of `-Command` because the latter re-parses
/// the remaining command-line arguments and drops their quoting. An SSH remote
/// command such as `cd '/tmp' && exec "$SHELL"` would then reach PowerShell
/// unquoted and fail on the `&&`, closing the new window immediately.
#[cfg(target_os = "windows")]
fn encode_powershell_command(script: &str) -> String {
    use base64::Engine;
    let utf16: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    base64::engine::general_purpose::STANDARD.encode(utf16)
}

/// Builds the program to execute and its parameter string for a new terminal
/// window.
#[cfg(target_os = "windows")]
fn build_windows_terminal_command(
    configured_terminal: Option<String>,
    args: &[String],
    wrap_shell: bool,
) -> (String, String) {
    if args.is_empty() {
        // Just open a terminal: the configured one, or a plain shell.
        return (
            configured_terminal.unwrap_or_else(|| "cmd".to_string()),
            String::new(),
        );
    }

    // GUI applications do not need a terminal, so launch them as they are.
    let is_gui = args[0].to_lowercase().ends_with(".exe")
        && crate::fs::utils::is_gui_executable(std::path::Path::new(&args[0]));
    if is_gui {
        return (args[0].clone(), join_windows_args(&args[1..]));
    }

    let Some(term) = configured_terminal else {
        if wrap_shell {
            let mut params = vec!["/K".to_string()];
            params.extend(args.iter().cloned());
            return ("cmd".to_string(), join_windows_args(&params));
        }
        // Launch the command directly; Windows gives console programs their
        // own console window.
        return (args[0].clone(), join_windows_args(&args[1..]));
    };

    let bin = term.to_lowercase();
    let mut params: Vec<String> = Vec::new();
    if bin.contains("wt") || bin.contains("windows terminal") {
        params.push("-d".to_string());
        params.push(".".to_string());
        if wrap_shell {
            params.push("cmd".to_string());
            params.push("/K".to_string());
        }
    } else if bin.contains("alacritty") {
        params.push("--command".to_string());
        if wrap_shell {
            params.push("cmd".to_string());
            params.push("/K".to_string());
        }
    } else if bin.contains("powershell") || bin.contains("pwsh") {
        if wrap_shell {
            params.push("-NoExit".to_string());
        }
        params.push("-EncodedCommand".to_string());
        params.push(encode_powershell_command(&build_powershell_script(args)));
        return (term, join_windows_args(&params));
    }
    params.extend(args.iter().cloned());
    (term, join_windows_args(&params))
}

/// Builds the command line that starts `program` in a new window.
#[cfg(target_os = "windows")]
fn build_windows_launch_line(program: &str, params: &str) -> String {
    let mut line = String::from("cmd.exe /C start \"\" ");
    line.push_str(&quote_windows_arg(program));
    if !params.is_empty() {
        line.push(' ');
        line.push_str(params);
    }
    line
}

/// Starts `program` with `params` in a new window.
///
/// The program is started through `cmd /C start ""`, which creates a new
/// console window for console programs and opens GUI programs normally (in
/// the foreground). The intermediate `cmd` is created with `CREATE_NO_WINDOW`
/// and without inherited handles, for two reasons:
///
/// - GUI programs such as Alacritty attach themselves to their parent's
///   console (`AttachConsole(ATTACH_PARENT_PROCESS)`) when it has one. The
///   intermediate `cmd` has its own, invisible console, so the program cannot
///   attach to fm's console. Otherwise that console, and with it fm's terminal
///   window, would stay alive until the spawned terminal exits.
/// - fm's stdio are the console/ConPTY handles inherited from the terminal
///   that started fm, so passing them on would keep that terminal alive too.
#[cfg(target_os = "windows")]
fn launch_windowed(program: &str, params: &str, dir: &std::path::Path) -> anyhow::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        CREATE_NO_WINDOW, CreateProcessW, PROCESS_INFORMATION, STARTUPINFOW,
    };
    use windows::core::{PCWSTR, PWSTR};

    fn wide(value: &std::ffi::OsStr) -> Vec<u16> {
        value.encode_wide().chain(std::iter::once(0)).collect()
    }

    let command_line = build_windows_launch_line(program, params);

    // CreateProcessW may modify the command line in place, so keep it mutable.
    let mut command_line = wide(std::ffi::OsStr::new(&command_line));
    let dir = wide(dir.as_os_str());
    let startup = STARTUPINFOW {
        cb: u32::try_from(std::mem::size_of::<STARTUPINFOW>())
            .expect("STARTUPINFOW size fits in u32"),
        ..Default::default()
    };
    let mut process_info = PROCESS_INFORMATION::default();

    // SAFETY: the command line is a mutable, null-terminated UTF-16 buffer and
    // the directory a null-terminated UTF-16 buffer, both outliving the call.
    unsafe {
        CreateProcessW(
            None,
            Some(PWSTR(command_line.as_mut_ptr())),
            None,
            None,
            false, // do not pass fm's handles on to the child
            CREATE_NO_WINDOW,
            None,
            PCWSTR(dir.as_ptr()),
            &raw const startup,
            &raw mut process_info,
        )?;
        let _ = CloseHandle(process_info.hProcess);
        let _ = CloseHandle(process_info.hThread);
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
    let (program, params) = build_windows_terminal_command(configured_terminal, args, wrap_shell);
    launch_windowed(&program, &params, dir)
        .map_err(|e| anyhow::anyhow!("Failed to spawn terminal {program}: {e}"))
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
    provider: &dyn crate::fs::provider::FileSystemProvider,
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
    _provider: &dyn crate::fs::provider::FileSystemProvider,
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
mod tests;
