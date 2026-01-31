use anyhow::{Result, anyhow};
use std::path::Path;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

/// Detect if rsync should be used for this transfer
/// Returns true only for local-remote or remote-local COPY operations
pub fn should_use_rsync(
    src_is_local: bool,
    dest_is_local: bool,
    action: crate::app::CopyMoveAction,
) -> bool {
    // Only use rsync for copy operations
    if action != crate::app::CopyMoveAction::Copy {
        return false;
    }

    // Only use rsync when one side is local and the other is remote
    // local → remote OR remote → local
    src_is_local != dest_is_local
}

/// Execute rsync for local → remote or remote → local file transfer
/// Uses SSH agent or key-based authentication to avoid password prompts
/// Returns Ok(()) if rsync succeeded, Err if it failed or is not applicable
pub async fn rsync_transfer(
    src_fs: &dyn crate::fs::traits::FileSystem,
    dest_fs: &dyn crate::fs::traits::FileSystem,
    src: &Path,
    dest: &Path,
    progress_ctx: &crate::fs::traits::TaskProgressContext,
) -> Result<()> {
    let src_is_local = src_fs.is_local();
    let dest_is_local = dest_fs.is_local();

    // Determine rsync source and destination arguments
    let (src_arg, dest_arg, remote_fs) = if src_is_local && !dest_is_local {
        // Local → Remote
        let remote_spec = format_remote_path(dest_fs, dest)?;
        (src.to_string_lossy().to_string(), remote_spec, dest_fs)
    } else if !src_is_local && dest_is_local {
        // Remote → Local
        let remote_spec = format_remote_path(src_fs, src)?;
        (remote_spec, dest.to_string_lossy().to_string(), src_fs)
    } else {
        return Err(anyhow!("rsync only supports local-remote transfers"));
    };

    // Get SSH options to reuse existing authentication
    let password = remote_fs.get_password();
    let ssh_opts = get_ssh_options(remote_fs, password.is_some())?;

    // Build rsync command with progress monitoring
    let mut cmd = if let Some(ref pass) = password {
        let mut c = Command::new("sshpass");
        c.arg("-p").arg(pass).arg("rsync");
        c
    } else {
        Command::new("rsync")
    };

    cmd.arg("-avz") // archive, verbose, compress
        .arg("--partial") // keep partial files for resume
        .arg("--progress") // show progress
        .arg("--info=progress2") // better progress format
        .arg("-e")
        .arg(&ssh_opts) // SSH options to reuse authentication
        .arg(&src_arg)
        .arg(&dest_arg)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // Signal that rsync is starting
    let _ = progress_ctx
        .tx
        .send(crate::tasks::TaskEvent::SetRsyncMode(progress_ctx.id, true));

    let mut child = cmd
        .spawn()
        .map_err(|e| anyhow!("Failed to spawn rsync: {}", e))?;

    // Monitor progress from stdout
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("Failed to capture rsync stdout"))?;
    let mut reader = BufReader::new(stdout).lines();

    let progress_task = tokio::spawn({
        let progress_ctx = progress_ctx.clone();
        async move {
            while let Ok(Some(line)) = reader.next_line().await {
                if progress_ctx
                    .cancel
                    .load(std::sync::atomic::Ordering::Relaxed)
                {
                    break;
                }

                // Parse rsync progress output
                // Format: "  1,234,567  45%  123.45kB/s    0:00:12"
                if let Some(parsed) = parse_rsync_progress(&line) {
                    let _ = progress_ctx
                        .tx
                        .send(crate::tasks::TaskEvent::UpdateByteProgress(
                            progress_ctx.id,
                            parsed.bytes_transferred,
                            parsed.total_bytes,
                        ));

                    progress_ctx.processed_bytes.store(
                        parsed.bytes_transferred,
                        std::sync::atomic::Ordering::Relaxed,
                    );
                }
            }
        }
    });

    // Wait for rsync to complete
    let status = child
        .wait()
        .await
        .map_err(|e| anyhow!("Failed to wait for rsync: {}", e))?;

    progress_task.abort();

    // Signal that rsync has finished
    let _ = progress_ctx.tx.send(crate::tasks::TaskEvent::SetRsyncMode(
        progress_ctx.id,
        false,
    ));

    if !status.success() {
        let stderr = child.stderr.take();
        let error_msg = if let Some(mut stderr) = stderr {
            let mut error_buf = String::new();
            use tokio::io::AsyncReadExt;
            let _ = stderr.read_to_string(&mut error_buf).await;
            error_buf
        } else {
            "Unknown error".to_string()
        };
        return Err(anyhow!(
            "rsync failed with status {}: {}",
            status,
            error_msg
        ));
    }

    Ok(())
}

/// Get SSH options for rsync to use existing SSH authentication
/// This tells rsync to use SSH agent or available keys without prompting for passwords
fn get_ssh_options(_fs: &dyn crate::fs::traits::FileSystem, has_password: bool) -> Result<String> {
    // Use SSH with the following options:
    // - BatchMode=yes: Never prompt for password (fail instead) - ONLY if no password provided
    // - StrictHostKeyChecking=no: Auto-accept host keys (for convenience)
    // - UserKnownHostsFile=/dev/null: Don't save host keys
    // - LogLevel=ERROR: Reduce noise
    // - ConnectTimeout=10: Don't wait forever
    //
    // This ensures rsync uses only SSH agent or key-based auth from the existing session
    let batch_mode = if has_password { "no" } else { "yes" };
    Ok(format!(
        "ssh -o BatchMode={} -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR -o ConnectTimeout=10",
        batch_mode
    ))
}

/// Format remote path for rsync (e.g., "user@host:/path/to/file")
fn format_remote_path(fs: &dyn crate::fs::traits::FileSystem, path: &Path) -> Result<String> {
    // Extract user@host from fs.context_key() which is formatted as "[user@host]"
    let context = fs.context_key();
    let user_host = context
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_string();

    if user_host.is_empty() {
        return Err(anyhow!("Cannot determine remote host from filesystem"));
    }

    Ok(format!("{}:{}", user_host, path.display()))
}

/// Parse rsync progress output
struct RsyncProgress {
    bytes_transferred: u64,
    total_bytes: u64,
}

fn parse_rsync_progress(line: &str) -> Option<RsyncProgress> {
    // rsync --info=progress2 format:
    // "  1,234,567  45%  123.45kB/s    0:00:12"
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 2 {
        return None;
    }

    // First part: bytes transferred (with commas)
    let bytes_str = parts[0].replace(',', "");
    let bytes_transferred = bytes_str.parse::<u64>().ok()?;

    // Second part: percentage
    let percent_str = parts[1].trim_end_matches('%');
    let percent = percent_str.parse::<f64>().ok()?;

    if percent > 0.0 {
        let total_bytes = (bytes_transferred as f64 / percent * 100.0) as u64;
        Some(RsyncProgress {
            bytes_transferred,
            total_bytes,
        })
    } else {
        Some(RsyncProgress {
            bytes_transferred,
            total_bytes: bytes_transferred,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_use_rsync_copy_local_to_remote() {
        assert!(should_use_rsync(
            true,
            false,
            crate::app::CopyMoveAction::Copy
        ));
    }

    #[test]
    fn test_should_use_rsync_copy_remote_to_local() {
        assert!(should_use_rsync(
            false,
            true,
            crate::app::CopyMoveAction::Copy
        ));
    }

    #[test]
    fn test_should_not_use_rsync_move() {
        assert!(!should_use_rsync(
            true,
            false,
            crate::app::CopyMoveAction::Move
        ));
    }

    #[test]
    fn test_should_not_use_rsync_local_to_local() {
        assert!(!should_use_rsync(
            true,
            true,
            crate::app::CopyMoveAction::Copy
        ));
    }

    #[test]
    fn test_should_not_use_rsync_remote_to_remote() {
        assert!(!should_use_rsync(
            false,
            false,
            crate::app::CopyMoveAction::Copy
        ));
    }

    #[test]
    fn test_parse_rsync_progress_valid() {
        let line = "  1,234,567  45%  123.45kB/s    0:00:12";
        let result = parse_rsync_progress(line);
        assert!(result.is_some());
        let progress = result.unwrap();
        assert_eq!(progress.bytes_transferred, 1234567);
        assert_eq!(progress.total_bytes, 2743482); // 1234567 / 0.45
    }

    #[test]
    fn test_parse_rsync_progress_zero_percent() {
        let line = "  100  0%  0kB/s    0:00:00";
        let result = parse_rsync_progress(line);
        assert!(result.is_some());
        let progress = result.unwrap();
        assert_eq!(progress.bytes_transferred, 100);
        assert_eq!(progress.total_bytes, 100);
    }

    #[test]
    fn test_parse_rsync_progress_invalid() {
        let line = "Invalid line";
        let result = parse_rsync_progress(line);
        assert!(result.is_none());
    }
}
