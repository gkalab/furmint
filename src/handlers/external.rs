//! Shared logic for launching external programs (editors, viewers, etc.)

use crate::app::AppState;
use crate::handlers::terminal::spawn_terminal;
use std::process::Stdio;

pub async fn launch_external_program(
    app: &mut AppState,
    cmd: &str,
    file_path: std::path::PathBuf,
    in_terminal: bool,
    program_name: &str, // e.g., "editor" or "viewer"
) -> Result<(), String> {
    let (program, mut args) = crate::config::parse_command(cmd);
    if program.is_empty() {
        return Err(format!("Invalid {program_name} command"));
    }

    let file_arg = file_path.to_string_lossy().to_string();
    args.push(file_arg);

    let active_tab = app.active_tab();
    let current_dir = active_tab.current_dir.clone();

    if in_terminal {
        let mut t_args = vec![program.clone()];
        t_args.extend(args);
        spawn_terminal(&current_dir, app.global.terminal.clone(), t_args, true)
            .map_err(|e| format!("Error launching {program_name} in terminal: {e}"))
    } else {
        // Launch directly (background)
        match std::process::Command::new(program)
            .args(&args)
            .current_dir(&current_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(_) => Ok(()),
            Err(e) => Err(format!("Error launching {program_name}: {e}")),
        }
    }
}
