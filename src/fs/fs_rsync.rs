#[cfg(unix)]
use anyhow::{Result, anyhow};
#[cfg(unix)]
use secrecy::ExposeSecret;
#[cfg(unix)]
use std::path::Path;
#[cfg(unix)]
use tokio::io::{AsyncBufReadExt, BufReader};

/// Detect if rsync should be used for this transfer
/// Returns true only for local-remote or remote-local COPY operations
#[cfg(unix)]
#[must_use]
pub fn should_use_rsync(
    src_fs: &dyn crate::fs::fs_provider::FileSystemProvider,
    dest_fs: &dyn crate::fs::fs_provider::FileSystemProvider,
    action: crate::state::CopyMoveAction,
) -> bool {
    // Only use rsync for copy operations
    if action != crate::state::CopyMoveAction::Copy {
        return false;
    }

    let src_is_local = src_fs.is_local();
    let dest_is_local = dest_fs.is_local();

    // Only use rsync when one side is local and the other is remote
    if src_is_local == dest_is_local {
        return false;
    }

    // Verify specifically that the remote side is SFTP
    let src_ctx = src_fs.context_key();
    let dest_ctx = dest_fs.context_key();

    let src_is_sftp = matches!(src_ctx, crate::fs::fs_provider::ContextKey::Ssh { .. });
    let dest_is_sftp = matches!(dest_ctx, crate::fs::fs_provider::ContextKey::Ssh { .. });

    src_is_sftp || dest_is_sftp
}

#[cfg(not(unix))]
pub fn should_use_rsync(
    _src_fs: &dyn crate::fs::fs_provider::FileSystemProvider,
    _dest_fs: &dyn crate::fs::fs_provider::FileSystemProvider,
    _action: crate::state::CopyMoveAction,
) -> bool {
    false
}

/// Execute rsync for local → remote or remote → local file transfer
/// Uses SSH agent or key-based authentication to avoid password prompts
/// Returns Ok(()) if rsync succeeded, Err if it failed or is not applicable
///
/// # Errors
///
/// Returns an error if the rsync transfer fails.
#[cfg(unix)]
pub async fn rsync_transfer(
    src_fs: &dyn crate::fs::fs_provider::FileSystemProvider,
    dest_fs: &dyn crate::fs::fs_provider::FileSystemProvider,
    src: &Path,
    dest: &Path,
    progress_ctx: &crate::fs::fs_provider::TaskProgressContext,
) -> Result<()> {
    let mut cmd = build_rsync_command(src_fs, dest_fs, src, dest).await?;

    // Signal that rsync is starting
    let _ = progress_ctx
        .tx
        .send(crate::tasks::TaskEvent::SetRsyncMode(progress_ctx.id, true));

    let mut child = cmd
        .spawn()
        .map_err(|e| anyhow!("Failed to spawn rsync: {e}"))?;

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
        .map_err(|e| anyhow!("Failed to wait for rsync: {e}"))?;

    progress_task.abort();

    // Signal that rsync has finished
    let _ = progress_ctx.tx.send(crate::tasks::TaskEvent::SetRsyncMode(
        progress_ctx.id,
        false,
    ));

    if !status.success() {
        let stderr = child.stderr.take();
        let error_msg = if let Some(mut stderr) = stderr {
            use tokio::io::AsyncReadExt;
            let mut error_buf = String::new();
            let _ = stderr.read_to_string(&mut error_buf).await;
            error_buf
        } else {
            "Unknown error".to_string()
        };
        return Err(anyhow!("rsync failed with status {status}: {error_msg}"));
    }

    Ok(())
}

#[cfg(not(unix))]
///
/// # Errors
///
/// Not supported on this platform
#[allow(clippy::unused_async)]
pub async fn rsync_transfer(
    _src_fs: &dyn crate::fs::fs_provider::FileSystemProvider,
    _dest_fs: &dyn crate::fs::fs_provider::FileSystemProvider,
    _src: &std::path::Path,
    _dest: &std::path::Path,
    _progress_ctx: &crate::fs::fs_provider::TaskProgressContext,
) -> anyhow::Result<()> {
    anyhow::bail!("rsync is not supported on this platform")
}

/// Get SSH options for rsync to use existing SSH authentication
/// This tells rsync to use SSH agent or available keys without prompting for passwords
#[cfg(unix)]
fn get_ssh_options(
    _fs: &dyn crate::fs::fs_provider::FileSystemProvider,
    has_password: bool,
) -> String {
    // Use SSH with the following options:
    // - BatchMode=yes: Never prompt for password (fail instead) - ONLY if no password provided
    // - StrictHostKeyChecking=no: Auto-accept host keys (for convenience)
    // - UserKnownHostsFile=/dev/null: Don't save host keys
    // - LogLevel=ERROR: Reduce noise
    // - ConnectTimeout=10: Don't wait forever
    //
    // This ensures rsync uses only SSH agent or key-based auth from the existing session
    let batch_mode = if has_password { "no" } else { "yes" };
    format!(
        "ssh -o BatchMode={batch_mode} -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR -o ConnectTimeout=10"
    )
}

/// Format remote path for rsync (e.g., "user@host:/path/to/file")
#[cfg(unix)]
fn format_remote_path(
    fs: &dyn crate::fs::fs_provider::FileSystemProvider,
    path: &std::path::Path,
) -> Result<String> {
    // Extract user@host from the structured context key
    let user_host = match fs.context_key() {
        crate::fs::fs_provider::ContextKey::Ssh { user, host, .. } => {
            format!("{user}@{host}")
        }
        _ => return Err(anyhow!("Cannot determine remote host from filesystem")),
    };

    if user_host.is_empty() {
        return Err(anyhow!("Cannot determine remote host from filesystem"));
    }

    Ok(format!("{}:{}", user_host, path.display()))
}

/// Parse rsync progress output
#[cfg(unix)]
struct RsyncProgress {
    bytes_transferred: u64,
    total_bytes: u64,
}

#[cfg(unix)]
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
        #[allow(
            clippy::cast_sign_loss,
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation
        )]
        let total_bytes = (bytes_transferred as f64 / percent * 100.0).max(0.0) as u64;
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

#[cfg(unix)]
async fn build_rsync_command(
    src_fs: &dyn crate::fs::fs_provider::FileSystemProvider,
    dest_fs: &dyn crate::fs::fs_provider::FileSystemProvider,
    src: &Path,
    dest: &Path,
) -> Result<tokio::process::Command> {
    let src_is_local = src_fs.is_local();
    let dest_is_local = dest_fs.is_local();

    // Determine rsync source and destination arguments
    let is_dir = src_fs.is_dir(src).await;
    let src_str = if is_dir {
        let mut s = src.to_string_lossy().to_string();
        if !s.ends_with('/') {
            s.push('/');
        }
        s
    } else {
        src.to_string_lossy().to_string()
    };

    let (src_arg, dest_arg, remote_fs) = if src_is_local && !dest_is_local {
        // Local → Remote
        let remote_spec = format_remote_path(dest_fs, dest)?;
        (src_str, remote_spec, dest_fs)
    } else if !src_is_local && dest_is_local {
        // Remote → Local
        let remote_spec = format_remote_path(src_fs, src)?;
        // Also add trailing slash to remote spec if it's a directory
        let remote_spec = if is_dir && !remote_spec.ends_with('/') {
            format!("{remote_spec}/")
        } else {
            remote_spec
        };
        (remote_spec, dest.to_string_lossy().to_string(), src_fs)
    } else {
        return Err(anyhow!("rsync only supports local-remote transfers"));
    };

    // Get SSH options to reuse existing authentication
    let password = remote_fs.get_password();
    let ssh_opts = get_ssh_options(remote_fs, password.is_some());

    // Build rsync command with progress monitoring
    let mut cmd = if let Some(ref pass) = password {
        let mut c = tokio::process::Command::new("sshpass");
        c.arg("-e")
            .arg("rsync")
            .env("SSHPASS", pass.expose_secret());
        c
    } else {
        tokio::process::Command::new("rsync")
    };

    cmd.arg("-avz") // archive, verbose, compress
        .arg("--partial") // keep partial files for resume
        .arg("--progress") // show progress
        .arg("--info=progress2") // better progress format
        .arg("--no-whole-file") // force delta-transfer algorithm
        .arg("-e")
        .arg(&ssh_opts) // SSH options to reuse authentication
        .arg(&src_arg)
        .arg(&dest_arg)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    Ok(cmd)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct MockFs {
        local: bool,
        ctx: String,
    }

    #[async_trait::async_trait]
    impl crate::fs::fs_provider::FileSystemProvider for MockFs {
        async fn list_dir(&self, _: &Path) -> anyhow::Result<Vec<crate::fs::utils::FileEntry>> {
            Ok(vec![])
        }
        async fn create_dir(&self, _: &Path) -> anyhow::Result<()> {
            Ok(())
        }
        async fn create_dir_all(&self, _: &Path) -> anyhow::Result<()> {
            Ok(())
        }
        async fn create_file(&self, _: &Path) -> anyhow::Result<()> {
            Ok(())
        }
        async fn delete(&self, _: &Path, _: bool) -> anyhow::Result<()> {
            Ok(())
        }
        async fn rename(&self, _: &Path, _: &Path) -> anyhow::Result<()> {
            Ok(())
        }
        async fn read_file(&self, _: &Path) -> anyhow::Result<Vec<u8>> {
            Ok(vec![])
        }
        async fn read_file_at(&self, _: &Path, _: u64, _: usize) -> anyhow::Result<Vec<u8>> {
            Ok(vec![])
        }
        async fn write_file(&self, _: &Path, _: &[u8]) -> anyhow::Result<()> {
            Ok(())
        }
        async fn write_file_at(&self, _: &Path, _: u64, _: &[u8]) -> anyhow::Result<()> {
            Ok(())
        }
        async fn exists(&self, _: &Path) -> bool {
            true
        }
        async fn is_dir(&self, _: &Path) -> bool {
            false
        }
        async fn canonicalize(&self, path: &Path) -> anyhow::Result<PathBuf> {
            Ok(path.to_path_buf())
        }
        async fn get_file_info(&self, _: &Path) -> Option<crate::fs::fs_provider::FileMetadata> {
            None
        }
        async fn get_permissions(&self, _: &Path) -> Option<u32> {
            None
        }
        async fn set_permissions(&self, _: &Path, _: u32) -> bool {
            false
        }
        async fn get_modified_time(&self, _: &Path) -> Option<std::time::SystemTime> {
            None
        }
        async fn set_modified_time(&self, _: &Path, _: std::time::SystemTime) -> bool {
            false
        }
        fn display_prefix(&self) -> &'static str {
            ""
        }
        fn is_local(&self) -> bool {
            self.local
        }
        fn context_key(&self) -> crate::fs::fs_provider::ContextKey {
            if self.local {
                crate::fs::fs_provider::ContextKey::Local
            } else if let Some(rest) = self.ctx.strip_prefix("archive:") {
                crate::fs::fs_provider::ContextKey::Archive(std::path::PathBuf::from(rest))
            } else {
                crate::fs::fs_provider::ContextKey::Ssh {
                    user: "user".to_string(),
                    host: "host".to_string(),
                    port: 22,
                }
            }
        }
        fn display_path(&self, path: &Path) -> String {
            path.to_string_lossy().to_string()
        }
        async fn calc_dir_size(&self, _: &Path) -> anyhow::Result<u64> {
            Ok(0)
        }
    }

    #[test]
    fn test_should_use_rsync_copy_local_to_remote() {
        let src = MockFs {
            local: true,
            ctx: "local".into(),
        };
        let dest = MockFs {
            local: false,
            ctx: "[user@host]".into(),
        };
        assert!(should_use_rsync(
            &src,
            &dest,
            crate::state::CopyMoveAction::Copy
        ));
    }

    #[test]
    fn test_should_use_rsync_copy_remote_to_local() {
        let src = MockFs {
            local: false,
            ctx: "[user@host]".into(),
        };
        let dest = MockFs {
            local: true,
            ctx: "local".into(),
        };
        assert!(should_use_rsync(
            &src,
            &dest,
            crate::state::CopyMoveAction::Copy
        ));
    }

    #[test]
    fn test_should_not_use_rsync_move() {
        let src = MockFs {
            local: true,
            ctx: "local".into(),
        };
        let dest = MockFs {
            local: false,
            ctx: "[user@host]".into(),
        };
        assert!(!should_use_rsync(
            &src,
            &dest,
            crate::state::CopyMoveAction::Move
        ));
    }

    #[test]
    fn test_should_not_use_rsync_local_to_local() {
        let src = MockFs {
            local: true,
            ctx: "local".into(),
        };
        let dest = MockFs {
            local: true,
            ctx: "local".into(),
        };
        assert!(!should_use_rsync(
            &src,
            &dest,
            crate::state::CopyMoveAction::Copy
        ));
    }

    #[test]
    fn test_should_not_use_rsync_remote_to_remote() {
        let src = MockFs {
            local: false,
            ctx: "[user@host]".into(),
        };
        let dest = MockFs {
            local: false,
            ctx: "[user@host]".into(),
        };
        assert!(!should_use_rsync(
            &src,
            &dest,
            crate::state::CopyMoveAction::Copy
        ));
    }

    #[test]
    fn test_should_not_use_rsync_archive() {
        let src = MockFs {
            local: false,
            ctx: "archive:/test.zip".into(),
        };
        let dest = MockFs {
            local: true,
            ctx: "local".into(),
        };
        assert!(!should_use_rsync(
            &src,
            &dest,
            crate::state::CopyMoveAction::Copy
        ));
    }

    #[test]
    fn test_parse_rsync_progress_valid() {
        let line = "  1,234,567  45%  123.45kB/s    0:00:12";
        let result = parse_rsync_progress(line);
        assert!(result.is_some());
        let progress = result.unwrap();
        assert_eq!(progress.bytes_transferred, 1_234_567);
        assert_eq!(progress.total_bytes, 2_743_482); // 1234567 / 0.45
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
