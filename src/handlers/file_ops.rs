//! File operations helpers: recursive ops, item counters, decision state

use async_trait::async_trait;

#[async_trait]
pub trait FileSystem: Send + Sync {
    async fn try_exists(&self, path: &std::path::Path) -> anyhow::Result<bool>;
    async fn is_dir(&self, path: &std::path::Path) -> anyhow::Result<bool>;
    async fn create_dir_all(&self, path: &std::path::Path) -> anyhow::Result<()>;
    async fn read_dir(&self, path: &std::path::Path) -> anyhow::Result<Vec<std::path::PathBuf>>;
    async fn rename(&self, src: &std::path::Path, dst: &std::path::Path) -> anyhow::Result<()>;
    async fn remove_file(&self, path: &std::path::Path) -> anyhow::Result<()>;
    async fn copy(&self, src: &std::path::Path, dst: &std::path::Path) -> anyhow::Result<()>;
    async fn read_file(&self, path: &std::path::Path) -> anyhow::Result<Vec<u8>>;
    async fn write_file(&self, path: &std::path::Path, data: &[u8]) -> anyhow::Result<()>;
    async fn get_permissions(&self, path: &std::path::Path) -> Option<u32>;
    async fn get_modified_time(&self, path: &std::path::Path) -> Option<std::time::SystemTime>;
    async fn set_modified_time(
        &self,
        path: &std::path::Path,
        mtime: std::time::SystemTime,
    ) -> anyhow::Result<()>;
    async fn write_file_with_permissions(
        &self,
        path: &std::path::Path,
        data: &[u8],
        mode: Option<u32>,
    ) -> anyhow::Result<()>;
    fn context_key(&self) -> String;
}

#[allow(dead_code)]
pub struct StdFileSystem;

#[async_trait]
impl FileSystem for StdFileSystem {
    async fn try_exists(&self, path: &std::path::Path) -> anyhow::Result<bool> {
        Ok(tokio::fs::try_exists(path).await?)
    }
    async fn is_dir(&self, path: &std::path::Path) -> anyhow::Result<bool> {
        Ok(path.is_dir())
    }
    async fn create_dir_all(&self, path: &std::path::Path) -> anyhow::Result<()> {
        Ok(tokio::fs::create_dir_all(path).await?)
    }
    async fn read_dir(&self, path: &std::path::Path) -> anyhow::Result<Vec<std::path::PathBuf>> {
        let mut result = Vec::new();
        let mut rd = tokio::fs::read_dir(path).await?;
        while let Ok(Some(entry)) = rd.next_entry().await {
            result.push(entry.path());
        }
        Ok(result)
    }
    async fn rename(&self, src: &std::path::Path, dst: &std::path::Path) -> anyhow::Result<()> {
        Ok(tokio::fs::rename(src, dst).await?)
    }
    async fn remove_file(&self, path: &std::path::Path) -> anyhow::Result<()> {
        Ok(tokio::fs::remove_file(path).await?)
    }
    async fn copy(&self, src: &std::path::Path, dst: &std::path::Path) -> anyhow::Result<()> {
        let mtime = tokio::fs::metadata(src)
            .await
            .ok()
            .and_then(|m| m.modified().ok());
        tokio::fs::copy(src, dst)
            .await
            .map(|_| ())
            .map_err(anyhow::Error::from)?;
        if let Some(mt) = mtime {
            let _ = self.set_modified_time(dst, mt).await;
        }
        Ok(())
    }
    async fn read_file(&self, path: &std::path::Path) -> anyhow::Result<Vec<u8>> {
        Ok(tokio::fs::read(path).await?)
    }
    async fn write_file(&self, path: &std::path::Path, data: &[u8]) -> anyhow::Result<()> {
        tokio::fs::write(path, data).await?;
        Ok(())
    }

    async fn get_permissions(&self, _path: &std::path::Path) -> Option<u32> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            tokio::fs::metadata(_path)
                .await
                .ok()
                .map(|m| m.permissions().mode() & 0o777)
        }
        #[cfg(not(unix))]
        {
            None
        }
    }

    async fn get_modified_time(&self, path: &std::path::Path) -> Option<std::time::SystemTime> {
        tokio::fs::metadata(path)
            .await
            .ok()
            .and_then(|m| m.modified().ok())
    }

    async fn set_modified_time(
        &self,
        path: &std::path::Path,
        mtime: std::time::SystemTime,
    ) -> anyhow::Result<()> {
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || {
            let duration = mtime
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| std::io::Error::other("invalid mtime"))?;
            let _sec = duration.as_secs() as libc::time_t;
            let _nsec = duration.subsec_nanos() as libc::c_long;

            #[cfg(unix)]
            {
                let path_cstr =
                    std::ffi::CString::new(path.to_string_lossy().as_bytes()).map_err(|_| {
                        std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid path")
                    })?;
                let result = unsafe {
                    libc::utimensat(
                        libc::AT_FDCWD,
                        path_cstr.as_ptr(),
                        [
                            libc::timespec {
                                tv_sec: 0,
                                tv_nsec: libc::UTIME_OMIT,
                            },
                            libc::timespec {
                                tv_sec: _sec,
                                tv_nsec: _nsec,
                            },
                        ]
                        .as_ptr(),
                        0,
                    )
                };
                if result != 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            #[cfg(windows)]
            {
                use filetime::FileTime;
                let ft = FileTime::from_system_time(mtime);
                filetime::set_file_mtime(&path, ft)?;
            }
            Ok::<(), std::io::Error>(())
        })
        .await??;
        Ok(())
    }

    async fn write_file_with_permissions(
        &self,
        path: &std::path::Path,
        data: &[u8],
        mode: Option<u32>,
    ) -> anyhow::Result<()> {
        tokio::fs::write(path, data).await?;
        if let Some(_mode_val) = mode {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Ok(metadata) = tokio::fs::metadata(path).await {
                    let current_mode = metadata.permissions().mode();
                    let new_mode = (current_mode & !0o777) | (_mode_val & 0o777);
                    let mut perms = metadata.permissions();
                    perms.set_mode(new_mode);
                    tokio::fs::set_permissions(path, perms).await?;
                }
            }
        }
        Ok(())
    }

    fn context_key(&self) -> String {
        "std_local".to_string()
    }
}

pub struct ProviderFileSystem(pub std::sync::Arc<dyn crate::fs_provider::FileSystemProvider>);

#[async_trait]
impl FileSystem for ProviderFileSystem {
    async fn try_exists(&self, path: &std::path::Path) -> anyhow::Result<bool> {
        let p = self.0.clone();
        let path = path.to_path_buf();
        Ok(tokio::task::spawn_blocking(move || p.exists(&path)).await?)
    }
    async fn is_dir(&self, path: &std::path::Path) -> anyhow::Result<bool> {
        let p = self.0.clone();
        let path = path.to_path_buf();
        Ok(tokio::task::spawn_blocking(move || p.is_dir(&path)).await?)
    }
    async fn create_dir_all(&self, path: &std::path::Path) -> anyhow::Result<()> {
        let p = self.0.clone();
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || p.create_dir(&path)).await??;
        Ok(())
    }
    async fn read_dir(&self, path: &std::path::Path) -> anyhow::Result<Vec<std::path::PathBuf>> {
        let p = self.0.clone();
        let path_buf = path.to_path_buf();
        let path_buf_clone = path_buf.clone();
        let entries = tokio::task::spawn_blocking(move || p.list_dir(&path_buf)).await??;
        Ok(entries
            .into_iter()
            .filter(|e| e.name != "..")
            .map(|e| path_buf_clone.join(e.name))
            .collect())
    }
    async fn rename(&self, src: &std::path::Path, dst: &std::path::Path) -> anyhow::Result<()> {
        let p = self.0.clone();
        let src = src.to_path_buf();
        let dst = dst.to_path_buf();
        tokio::task::spawn_blocking(move || p.rename(&src, &dst)).await??;
        Ok(())
    }
    async fn remove_file(&self, path: &std::path::Path) -> anyhow::Result<()> {
        let p = self.0.clone();
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || p.delete(&path, false)).await??;
        Ok(())
    }
    async fn copy(&self, src: &std::path::Path, dst: &std::path::Path) -> anyhow::Result<()> {
        // Core cross-provider copy logic
        let data = self.read_file(src).await?;

        // Preserve permissions if the destination provider supports it
        let src_perms = self.0.get_permissions(src);

        // Try to write with permissions if supported, otherwise write normally and set afterwards
        if let Some(mode) = src_perms {
            let dst_clone = dst.to_path_buf();
            let data_clone = data.clone();
            let provider = self.0.clone();
            tokio::task::spawn_blocking(move || {
                provider.write_file_with_permissions(&dst_clone, &data_clone, Some(mode))
            })
            .await??;
        } else {
            self.write_file(dst, &data).await?;
        }

        Ok(())
    }
    async fn read_file(&self, path: &std::path::Path) -> anyhow::Result<Vec<u8>> {
        let p = self.0.clone();
        let path = path.to_path_buf();
        Ok(tokio::task::spawn_blocking(move || p.read_file(&path)).await??)
    }
    async fn write_file(&self, path: &std::path::Path, data: &[u8]) -> anyhow::Result<()> {
        let p = self.0.clone();
        let path = path.to_path_buf();
        let data = data.to_vec();
        tokio::task::spawn_blocking(move || p.write_file(&path, &data)).await??;
        Ok(())
    }

    async fn get_permissions(&self, path: &std::path::Path) -> Option<u32> {
        let p = self.0.clone();
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || p.get_permissions(&path))
            .await
            .unwrap_or(None)
    }

    async fn get_modified_time(&self, path: &std::path::Path) -> Option<std::time::SystemTime> {
        let p = self.0.clone();
        let path = path.to_path_buf();
        tokio::task::spawn_blocking(move || p.get_modified_time(&path))
            .await
            .ok()
            .flatten()
    }

    async fn set_modified_time(
        &self,
        path: &std::path::Path,
        mtime: std::time::SystemTime,
    ) -> anyhow::Result<()> {
        let p = self.0.clone();
        let path = path.to_path_buf();
        let success = tokio::task::spawn_blocking(move || p.set_modified_time(&path, mtime))
            .await
            .unwrap_or(false);
        if success {
            Ok(())
        } else {
            Err(anyhow::anyhow!("Failed to set modified time"))
        }
    }

    async fn write_file_with_permissions(
        &self,
        path: &std::path::Path,
        data: &[u8],
        mode: Option<u32>,
    ) -> anyhow::Result<()> {
        let p = self.0.clone();
        let path = path.to_path_buf();
        let data = data.to_vec();
        tokio::task::spawn_blocking(move || p.write_file_with_permissions(&path, &data, mode))
            .await??;
        Ok(())
    }

    fn context_key(&self) -> String {
        self.0.context_key()
    }
}

// Helper to count items recursively
pub async fn count_items(fs: &dyn FileSystem, paths: &[std::path::PathBuf]) -> usize {
    let mut count = 0;
    for path in paths {
        count += 1; // Count the item itself
        if let Ok(true) = fs.is_dir(path).await
            && let Ok(children) = fs.read_dir(path).await
        {
            count += Box::pin(count_items(fs, &children)).await;
        }
    }
    count
}

pub struct DecisionState {
    pub overwrite_all: bool,
    pub skip_all: bool,
    pub last_update: std::time::Instant,
}

// Recursive operation
// Returns Result<(), String>
// Iterative operation to avoid stack overflow
// Returns Result<(), String>
pub struct RecursiveOpContext<'a> {
    pub src_fs: &'a dyn FileSystem,
    pub dest_fs: &'a dyn FileSystem,
    pub src: &'a std::path::Path,
    pub dest: &'a std::path::Path,
    pub action: crate::app::CopyMoveAction,
    pub cancel: &'a std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub tx: &'a tokio::sync::mpsc::UnboundedSender<crate::tasks::TaskEvent>,
    pub id: usize,
    pub total: usize,
    pub processed: &'a std::sync::Arc<std::sync::atomic::AtomicUsize>,
    pub decision_rx: &'a std::sync::Arc<
        tokio::sync::Mutex<tokio::sync::mpsc::Receiver<crate::tasks::TaskDecision>>,
    >,
}

pub fn recursive_op<'a>(
    ctx: RecursiveOpContext<'a>,
    decision_state: &'a mut DecisionState,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + 'a>> {
    Box::pin(async move {
        // Internal enum for stack
        enum WorkItem {
            Process {
                src: std::path::PathBuf,
                dest: std::path::PathBuf,
            },
            PostProcessDir {
                src: std::path::PathBuf,
            },
        }

        let mut stack = vec![WorkItem::Process {
            src: ctx.src.to_path_buf(),
            dest: ctx.dest.to_path_buf(),
        }];

        while let Some(item) = stack.pop() {
            if ctx.cancel.load(std::sync::atomic::Ordering::Relaxed) {
                return Ok(()); // Cancelled
            }

            match item {
                WorkItem::PostProcessDir { src } => {
                    // Remove empty directory after move
                    let _ = ctx.src_fs.remove_file(&src).await;
                }
                WorkItem::Process { src, dest } => {
                    // Move optimization: Try rename first if it's a move operation and same FS
                    let same_fs = ctx.src_fs.context_key() == ctx.dest_fs.context_key();
                    if ctx.action == crate::app::CopyMoveAction::Move && same_fs {
                        let dest_exists = ctx.dest_fs.try_exists(&dest).await.unwrap_or(false);
                        if !dest_exists && ctx.src_fs.rename(&src, &dest).await.is_ok() {
                            // Successfully moved! Update progress and continue.
                            // count_items includes the root, but we've already counted it.
                            // Children will be processed individually, so we add count - 1.
                            let count = count_items(ctx.dest_fs, std::slice::from_ref(&dest)).await;
                            let increment = count.saturating_sub(1);
                            let p = ctx
                                .processed
                                .fetch_add(increment, std::sync::atomic::Ordering::Relaxed)
                                + increment;
                            let _ = ctx.tx.send(crate::tasks::TaskEvent::UpdateProgress(
                                ctx.id, p, ctx.total,
                            ));
                            continue;
                        }
                    }

                    if ctx.src_fs.is_dir(&src).await.unwrap_or(false) {
                        let dest_exists = ctx.dest_fs.try_exists(&dest).await.unwrap_or(false);

                        if !dest_exists {
                            if let Err(e) = ctx.dest_fs.create_dir_all(&dest).await {
                                return Err(format!(
                                    "Failed to create directory {}: {}",
                                    dest.display(),
                                    e
                                ));
                            }
                        } else if !ctx.dest_fs.is_dir(&dest).await.unwrap_or(true) {
                            return Err(format!(
                                "Destination {} exists and is not a directory",
                                dest.display()
                            ));
                        }

                        if ctx.action == crate::app::CopyMoveAction::Move {
                            stack.push(WorkItem::PostProcessDir { src: src.clone() });
                        }

                        let children = match ctx.src_fs.read_dir(&src).await {
                            Ok(v) => v,
                            Err(e) => {
                                return Err(format!(
                                    "Failed to read directory {}: {}",
                                    src.display(),
                                    e
                                ));
                            }
                        };
                        for path in children {
                            let Some(name) = path.file_name() else {
                                continue;
                            };
                            let child_dest = dest.join(name);
                            stack.push(WorkItem::Process {
                                src: path,
                                dest: child_dest,
                            });
                        }
                    } else {
                        // File handling
                        let mut perform = true;
                        let dest_exists = ctx.dest_fs.try_exists(&dest).await.unwrap_or(false);

                        if dest_exists {
                            // Conflict resolution
                            if decision_state.overwrite_all {
                                perform = true;
                            } else if decision_state.skip_all {
                                perform = false;
                            } else {
                                // Ask user
                                let _ = ctx.tx.send(crate::tasks::TaskEvent::Conflict(
                                    ctx.id,
                                    dest.clone(),
                                    crate::tasks::ConflictType::FileExists,
                                ));

                                // Wait for decision
                                let mut decision = None;
                                if let Some(rx) = ctx.decision_rx.lock().await.recv().await {
                                    decision = Some(rx);
                                }

                                match decision {
                                    Some(crate::tasks::TaskDecision::Overwrite) => perform = true,
                                    Some(crate::tasks::TaskDecision::OverwriteAll) => {
                                        decision_state.overwrite_all = true;
                                        perform = true;
                                    }
                                    Some(crate::tasks::TaskDecision::Skip) => perform = false,
                                    Some(crate::tasks::TaskDecision::SkipAll) => {
                                        decision_state.skip_all = true;
                                        perform = false;
                                    }
                                    Some(crate::tasks::TaskDecision::Cancel) => {
                                        ctx.cancel
                                            .store(true, std::sync::atomic::Ordering::Relaxed);
                                        let _ = ctx.tx.send(crate::tasks::TaskEvent::UpdateStatus(
                                            ctx.id,
                                            crate::tasks::TaskStatus::Cancelled,
                                        ));
                                        return Ok(());
                                    }
                                    _ => perform = false, // Default skip or error
                                }
                            }
                        }

                        if perform {
                            loop {
                                if dest_exists {
                                    let _ = ctx.dest_fs.remove_file(&dest).await;
                                }

                                // Check if it's the same filesystem type
                                let same_fs = ctx.src_fs.context_key() == ctx.dest_fs.context_key();
                                let copy_res = if same_fs {
                                    ctx.dest_fs.copy(&src, &dest).await
                                } else {
                                    match ctx.src_fs.read_file(&src).await {
                                        Ok(data) => {
                                            // For cross-filesystem copies, preserve permissions
                                            let perms = ctx.src_fs.get_permissions(&src).await;
                                            ctx.dest_fs
                                                .write_file_with_permissions(&dest, &data, perms)
                                                .await
                                        }
                                        Err(e) => Err(e),
                                    }
                                };

                                match copy_res {
                                    Ok(()) => {
                                        if let Some(mtime) =
                                            ctx.src_fs.get_modified_time(&src).await
                                        {
                                            let _ =
                                                ctx.dest_fs.set_modified_time(&dest, mtime).await;
                                        }
                                        break;
                                    }
                                    Err(e) => {
                                        if decision_state.skip_all {
                                            perform = false;
                                            break;
                                        }
                                        let _ = ctx.tx.send(crate::tasks::TaskEvent::Error(
                                            ctx.id,
                                            src.display().to_string(),
                                            format!("Failed to copy to {}: {}", dest.display(), e),
                                        ));
                                        let decision = ctx.decision_rx.lock().await.recv().await;
                                        match decision {
                                            Some(crate::tasks::TaskDecision::Retry) => {}
                                            Some(crate::tasks::TaskDecision::Skip) => {
                                                perform = false;
                                                break;
                                            }
                                            Some(crate::tasks::TaskDecision::SkipAll) => {
                                                decision_state.skip_all = true;
                                                perform = false;
                                                break;
                                            }
                                            Some(crate::tasks::TaskDecision::Cancel) => {
                                                return Ok(());
                                            }
                                            _ => {
                                                perform = false;
                                                break;
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        if ctx.action == crate::app::CopyMoveAction::Move && perform {
                            let _ = ctx.src_fs.remove_file(&src).await;
                        }

                        // Update progress
                        let p = ctx
                            .processed
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                            + 1;
                        let now = std::time::Instant::now();
                        if now.duration_since(decision_state.last_update)
                            > std::time::Duration::from_millis(100)
                            || p == ctx.total
                        {
                            let _ = ctx.tx.send(crate::tasks::TaskEvent::UpdateProgress(
                                ctx.id, p, ctx.total,
                            ));
                            decision_state.last_update = now;
                        }
                    }
                }
            }
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::path::PathBuf;

    #[tokio::test]
    async fn test_count_items_empty() {
        let empty: Vec<PathBuf> = vec![];
        assert_eq!(count_items(&StdFileSystem, &empty).await, 0);
    }

    #[tokio::test]
    async fn test_count_items_files_and_dirs() {
        // Setup temp dir structure: tmpdir/ (file1, subdir/file2, subdir2/)
        let tmp_dir = tempfile::tempdir().unwrap();
        let file1 = tmp_dir.path().join("file1.txt");
        File::create(&file1).unwrap();
        let subdir = tmp_dir.path().join("subdir");
        fs::create_dir(&subdir).unwrap();
        let file2 = subdir.join("file2.txt");
        File::create(&file2).unwrap();
        let subdir2 = tmp_dir.path().join("subdir2");
        fs::create_dir(&subdir2).unwrap();

        // Paths to test: root of tmp_dir only
        let root_entries: Vec<PathBuf> = fs::read_dir(tmp_dir.path())
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        // Should count: file1.txt, subdir, subdir2, file2.txt
        assert_eq!(count_items(&StdFileSystem, &root_entries).await, 4);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_copy_preserves_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let tmp_dir = tempfile::tempdir().unwrap();
        let src_file = tmp_dir.path().join("source.txt");
        let dst_file = tmp_dir.path().join("destination.txt");

        // Create source file with specific permissions
        {
            std::fs::write(&src_file, b"test content").unwrap();
            // Set executable permissions
            std::process::Command::new("chmod")
                .args(["755", &src_file.to_string_lossy()])
                .status()
                .unwrap();
        }

        // Verify source has correct permissions
        let src_perms = std::fs::metadata(&src_file).unwrap().permissions().mode() & 0o777;
        assert_eq!(src_perms, 0o755);

        // Copy the file
        StdFileSystem.copy(&src_file, &dst_file).await.unwrap();

        // Verify destination file exists and has same permissions
        assert!(dst_file.exists());
        let dst_perms = std::fs::metadata(&dst_file).unwrap().permissions().mode() & 0o777;
        assert_eq!(dst_perms, 0o755, "Expected 0o755, got 0{:o}", dst_perms);
    }

    #[tokio::test]
    async fn test_copy_preserves_modified_time() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let src_file = tmp_dir.path().join("source.txt");
        let dst_file = tmp_dir.path().join("destination.txt");

        std::fs::write(&src_file, b"test content").unwrap();

        let past_time = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1609459200);
        StdFileSystem
            .set_modified_time(&src_file, past_time)
            .await
            .unwrap();

        let src_mtime = StdFileSystem.get_modified_time(&src_file).await;
        assert!(src_mtime.is_some());

        StdFileSystem.copy(&src_file, &dst_file).await.unwrap();

        assert!(dst_file.exists());
        let dst_mtime = StdFileSystem.get_modified_time(&dst_file).await;

        if let Some(dst_mtime) = dst_mtime {
            let diff = dst_mtime.duration_since(past_time).unwrap();
            assert!(
                diff.as_secs() < 2,
                "Timestamp not preserved within 2 seconds"
            );
        }
    }

    #[tokio::test]
    async fn test_set_modified_time() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let test_file = tmp_dir.path().join("test_mtime.txt");

        std::fs::write(&test_file, b"test").unwrap();

        let past_time = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1609459200);
        let result = StdFileSystem.set_modified_time(&test_file, past_time).await;
        assert!(result.is_ok());

        let mtime = StdFileSystem.get_modified_time(&test_file).await;
        assert!(mtime.is_some());

        if let Some(mtime) = mtime {
            let diff = mtime.duration_since(past_time).unwrap();
            assert!(diff.as_secs() < 2, "Modified time not set correctly");
        }

        let content = std::fs::read(&test_file).unwrap();
        assert_eq!(
            content, b"test",
            "File content should be preserved after set_modified_time"
        );
    }

    #[tokio::test]
    async fn test_copy_preserves_content_and_modified_time() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let src_file = tmp_dir.path().join("source.txt");
        let dst_file = tmp_dir.path().join("destination.txt");

        let original_content = b"Hello, World! This is a test file with some content.";
        std::fs::write(&src_file, original_content).unwrap();

        let past_time = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1609459200);
        StdFileSystem
            .set_modified_time(&src_file, past_time)
            .await
            .unwrap();

        StdFileSystem.copy(&src_file, &dst_file).await.unwrap();

        let dst_content = std::fs::read(&dst_file).unwrap();
        assert_eq!(
            dst_content, original_content,
            "Copied file content should match source"
        );

        let dst_mtime = StdFileSystem.get_modified_time(&dst_file).await.unwrap();
        let diff = dst_mtime.duration_since(past_time).unwrap();
        assert!(diff.as_secs() < 2, "Timestamp should be preserved");
    }
}

#[cfg(test)]
mod mock_fs_tests {
    use super::*;
    use async_trait::async_trait;
    use std::collections::{HashMap, HashSet};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize};
    use tokio::sync::{Mutex, mpsc};

    #[derive(Default)]
    struct FakeEntry {
        is_dir: bool,
    }

    #[derive(Default)]
    struct MockFileSystem {
        files: Arc<Mutex<HashMap<PathBuf, FakeEntry>>>,
        copies: Arc<Mutex<Vec<(PathBuf, PathBuf)>>>,
        removed_files: Arc<Mutex<HashSet<PathBuf>>>,
        fail_next_copy: Arc<Mutex<usize>>,
        fail_next_create_dir: Arc<Mutex<usize>>,
    }

    #[async_trait]
    impl FileSystem for MockFileSystem {
        async fn try_exists(&self, path: &Path) -> anyhow::Result<bool> {
            Ok(self.files.lock().await.contains_key(path)
                && !self.removed_files.lock().await.contains(path))
        }
        async fn is_dir(&self, path: &Path) -> anyhow::Result<bool> {
            Ok(self
                .files
                .lock()
                .await
                .get(path)
                .map(|e| e.is_dir)
                .unwrap_or(false))
        }
        async fn create_dir_all(&self, path: &Path) -> anyhow::Result<()> {
            let mut fail = self.fail_next_create_dir.lock().await;
            if *fail > 0 {
                *fail -= 1;
                return Err(anyhow::anyhow!("Mock error creating directory"));
            }
            self.files
                .lock()
                .await
                .insert(path.to_path_buf(), FakeEntry { is_dir: true });
            Ok(())
        }
        async fn read_dir(&self, path: &Path) -> anyhow::Result<Vec<PathBuf>> {
            let files = self.files.lock().await;
            let mut out = Vec::new();
            for k in files.keys() {
                if k.parent() == Some(path) && !self.removed_files.lock().await.contains(k) {
                    out.push(k.clone());
                }
            }
            Ok(out)
        }
        async fn rename(&self, src: &Path, dst: &Path) -> anyhow::Result<()> {
            let mut files = self.files.lock().await;
            let mut to_move = Vec::new();
            for k in files.keys() {
                if k == src || k.starts_with(src) {
                    to_move.push(k.clone());
                }
            }
            if to_move.is_empty() {
                return Err(anyhow::anyhow!("No such file/dir for rename"));
            }

            // To avoid iterator invalidation or missing items, we collect first
            let mut moved_entries = Vec::new();
            for k in to_move {
                if let Some(entry) = files.remove(&k) {
                    let rel = k.strip_prefix(src).unwrap();
                    let new_path = dst.join(rel);
                    moved_entries.push((new_path, entry));
                }
            }
            for (p, e) in moved_entries {
                files.insert(p, e);
            }
            Ok(())
        }
        async fn remove_file(&self, path: &Path) -> anyhow::Result<()> {
            self.removed_files.lock().await.insert(path.to_path_buf());
            Ok(())
        }
        async fn copy(&self, src: &Path, dst: &Path) -> anyhow::Result<()> {
            let mut fail = self.fail_next_copy.lock().await;
            if *fail > 0 {
                *fail -= 1;
                return Err(anyhow::anyhow!("Mock error copying file"));
            }
            let files = self.files.lock().await;
            if files.contains_key(src) {
                drop(files);
                self.files
                    .lock()
                    .await
                    .insert(dst.to_path_buf(), FakeEntry { is_dir: false });
                self.copies
                    .lock()
                    .await
                    .push((src.to_path_buf(), dst.to_path_buf()));
                Ok(())
            } else {
                Err(anyhow::anyhow!("Missing file for copy"))
            }
        }
        async fn read_file(&self, path: &Path) -> anyhow::Result<Vec<u8>> {
            if self.files.lock().await.contains_key(path) {
                Ok(vec![]) // Fake empty content
            } else {
                Err(anyhow::anyhow!("File not found"))
            }
        }
        async fn write_file(&self, path: &Path, _data: &[u8]) -> anyhow::Result<()> {
            self.files
                .lock()
                .await
                .insert(path.to_path_buf(), FakeEntry { is_dir: false });
            Ok(())
        }

        async fn get_permissions(&self, _path: &Path) -> Option<u32> {
            Some(0o644) // Mock permissions
        }

        async fn get_modified_time(&self, _path: &Path) -> Option<std::time::SystemTime> {
            Some(std::time::SystemTime::UNIX_EPOCH)
        }

        async fn set_modified_time(
            &self,
            _path: &Path,
            _mtime: std::time::SystemTime,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn write_file_with_permissions(
            &self,
            path: &Path,
            _data: &[u8],
            _mode: Option<u32>,
        ) -> anyhow::Result<()> {
            self.files
                .lock()
                .await
                .insert(path.to_path_buf(), FakeEntry { is_dir: false });
            Ok(())
        }

        fn context_key(&self) -> String {
            "mock".to_string()
        }
    }

    #[tokio::test]
    async fn test_recursive_copy_nested() {
        let fs = MockFileSystem::default();
        let src_root = PathBuf::from("/src");
        let dest_root = PathBuf::from("/dest");

        // Hierarchy:
        // /src/file1.txt
        // /src/subdir/file2.txt

        {
            let mut files = fs.files.lock().await;
            files.insert(src_root.clone(), FakeEntry { is_dir: true });
            files.insert(src_root.join("file1.txt"), FakeEntry { is_dir: false });
            files.insert(src_root.join("subdir"), FakeEntry { is_dir: true });
            files.insert(
                src_root.join("subdir").join("file2.txt"),
                FakeEntry { is_dir: false },
            );
        }

        let (tx, _rx) = mpsc::unbounded_channel();
        let (_dtx, drx_real) = mpsc::channel(1);
        let processed = Arc::new(AtomicUsize::new(0));
        let decision_rx = Arc::new(Mutex::new(drx_real));
        let cancel = Arc::new(AtomicBool::new(false));
        let ctx = RecursiveOpContext {
            src_fs: &fs,
            dest_fs: &fs,
            src: &src_root,
            dest: &dest_root,
            action: crate::app::CopyMoveAction::Copy,
            cancel: &cancel,
            tx: &tx,
            id: 1,
            total: 3,
            processed: &processed,
            decision_rx: &decision_rx,
        };
        let mut decision_state = DecisionState {
            overwrite_all: false,
            skip_all: false,
            last_update: std::time::Instant::now(),
        };

        let res = recursive_op(ctx, &mut decision_state).await;

        assert!(res.is_ok(), "Recursive copy failed: {:?}", res.err());

        // Verify destination structure
        assert!(fs.try_exists(&dest_root).await.unwrap());
        assert!(fs.try_exists(&dest_root.join("file1.txt")).await.unwrap());
        assert!(fs.try_exists(&dest_root.join("subdir")).await.unwrap());
        assert!(
            fs.try_exists(&dest_root.join("subdir").join("file2.txt"))
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn test_recursive_copy_overwrite_all() {
        let fs = MockFileSystem::default();
        let src_root = PathBuf::from("/src");
        let dest_root = PathBuf::from("/dest");

        {
            let mut files = fs.files.lock().await;
            files.insert(src_root.clone(), FakeEntry { is_dir: true });
            files.insert(src_root.join("f1.txt"), FakeEntry { is_dir: false });
            files.insert(src_root.join("f2.txt"), FakeEntry { is_dir: false });

            // f1 and f2 exist in dest
            files.insert(dest_root.clone(), FakeEntry { is_dir: true });
            files.insert(dest_root.join("f1.txt"), FakeEntry { is_dir: false });
            files.insert(dest_root.join("f2.txt"), FakeEntry { is_dir: false });
        }

        let (tx, mut rx) = mpsc::unbounded_channel();
        let (dtx, drx_real) = mpsc::channel(1);
        let processed = Arc::new(AtomicUsize::new(0));
        let decision_rx = Arc::new(Mutex::new(drx_real));
        let ctx = RecursiveOpContext {
            src_fs: &fs,
            dest_fs: &fs,
            src: &src_root,
            dest: &dest_root,
            action: crate::app::CopyMoveAction::Copy,
            cancel: &Arc::new(AtomicBool::new(false)),
            tx: &tx,
            id: 1,
            total: 2,
            processed: &processed,
            decision_rx: &decision_rx,
        };
        let mut decision_state = DecisionState {
            overwrite_all: false,
            skip_all: false,
            last_update: std::time::Instant::now(),
        };

        // Decision task: send OverwriteAll on first conflict
        tokio::spawn(async move {
            if let Some(crate::tasks::TaskEvent::Conflict(_, _, _)) = rx.recv().await {
                let _ = dtx.send(crate::tasks::TaskDecision::OverwriteAll).await;
            }
        });

        let res = recursive_op(ctx, &mut decision_state).await;

        assert!(res.is_ok());
        assert!(decision_state.overwrite_all);

        let copies = fs.copies.lock().await;
        assert!(copies.iter().any(|(_, d)| d == &dest_root.join("f1.txt")));
        assert!(copies.iter().any(|(_, d)| d == &dest_root.join("f2.txt")));
        // Check removed_files for f1 and f2 (overwritten files are removed before copy in the implementation)
        let removed = fs.removed_files.lock().await;
        assert!(removed.contains(&dest_root.join("f1.txt")));
        assert!(removed.contains(&dest_root.join("f2.txt")));
    }

    #[tokio::test]
    async fn test_recursive_copy_cancel() {
        let fs = MockFileSystem::default();
        let src_root = PathBuf::from("/src");
        let dest_root = PathBuf::from("/dest");

        {
            let mut files = fs.files.lock().await;
            files.insert(src_root.clone(), FakeEntry { is_dir: true });
            files.insert(src_root.join("f1.txt"), FakeEntry { is_dir: false });
            files.insert(src_root.join("f2.txt"), FakeEntry { is_dir: false });

            // Both exist in dest to ensure conflict
            files.insert(dest_root.clone(), FakeEntry { is_dir: true });
            files.insert(dest_root.join("f1.txt"), FakeEntry { is_dir: false });
            files.insert(dest_root.join("f2.txt"), FakeEntry { is_dir: false });
        }

        let (tx, mut rx) = mpsc::unbounded_channel();
        let (dtx, drx_real) = mpsc::channel(1);
        let processed = Arc::new(AtomicUsize::new(0));
        let decision_rx = Arc::new(Mutex::new(drx_real));
        let ctx = RecursiveOpContext {
            src_fs: &fs,
            dest_fs: &fs,
            src: &src_root,
            dest: &dest_root,
            action: crate::app::CopyMoveAction::Copy,
            cancel: &Arc::new(AtomicBool::new(false)),
            tx: &tx,
            id: 1,
            total: 2,
            processed: &processed,
            decision_rx: &decision_rx,
        };
        let mut decision_state = DecisionState {
            overwrite_all: false,
            skip_all: false,
            last_update: std::time::Instant::now(),
        };

        // Decision task: send Cancel on first conflict
        tokio::spawn(async move {
            if let Some(crate::tasks::TaskEvent::Conflict(_, _, _)) = rx.recv().await {
                let _ = dtx.send(crate::tasks::TaskDecision::Cancel).await;
            }
        });

        let res = recursive_op(ctx, &mut decision_state).await;

        assert!(res.is_ok());

        // At most one file could have been processed (the one that triggered the conflict)
        // But since it returned on Cancel, no copy should have been performed for that file either.
        let copies = fs.copies.lock().await;
        assert_eq!(copies.len(), 0, "No files should have been copied");
    }

    #[tokio::test]
    async fn test_recursive_copy_skip_all() {
        let fs = MockFileSystem::default();
        let src_root = PathBuf::from("/src");
        let dest_root = PathBuf::from("/dest");

        {
            let mut files = fs.files.lock().await;
            files.insert(src_root.clone(), FakeEntry { is_dir: true });
            files.insert(src_root.join("f1.txt"), FakeEntry { is_dir: false });
            files.insert(src_root.join("f2.txt"), FakeEntry { is_dir: false });

            // f1 exists in dest
            files.insert(dest_root.clone(), FakeEntry { is_dir: true });
            files.insert(dest_root.join("f1.txt"), FakeEntry { is_dir: false });
        }

        let (tx, mut rx) = mpsc::unbounded_channel();
        let (dtx, drx_real) = mpsc::channel(1);
        let processed = Arc::new(AtomicUsize::new(0));
        let decision_rx = Arc::new(Mutex::new(drx_real));
        let ctx = RecursiveOpContext {
            src_fs: &fs,
            dest_fs: &fs,
            src: &src_root,
            dest: &dest_root,
            action: crate::app::CopyMoveAction::Copy,
            cancel: &Arc::new(AtomicBool::new(false)),
            tx: &tx,
            id: 1,
            total: 2,
            processed: &processed,
            decision_rx: &decision_rx,
        };
        let mut decision_state = DecisionState {
            overwrite_all: false,
            skip_all: false,
            last_update: std::time::Instant::now(),
        };

        // Decision task
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                if let crate::tasks::TaskEvent::Conflict(_, _, _) = event {
                    let _ = dtx.send(crate::tasks::TaskDecision::SkipAll).await;
                }
            }
        });

        let res = recursive_op(ctx, &mut decision_state).await;

        assert!(res.is_ok());
        assert!(decision_state.skip_all);

        let copies = fs.copies.lock().await;
        assert!(!copies.iter().any(|(_, d)| d == &dest_root.join("f1.txt")));
        assert!(copies.iter().any(|(_, d)| d == &dest_root.join("f2.txt")));
    }

    #[tokio::test]
    async fn test_mock_simple_move_conflict_overwrite() {
        let fs = MockFileSystem::default();
        let src = PathBuf::from("/src.txt");
        let dst = PathBuf::from("/dst.txt");
        fs.files
            .lock()
            .await
            .insert(src.clone(), FakeEntry { is_dir: false });
        fs.files
            .lock()
            .await
            .insert(dst.clone(), FakeEntry { is_dir: false }); // Simulate existing dest

        let (tx, mut rx) = mpsc::unbounded_channel();
        let (decision_tx, decision_rx_real) = mpsc::channel(1);
        let processed = Arc::new(AtomicUsize::new(0));
        let decision_rx = Arc::new(Mutex::new(decision_rx_real));
        let ctx = RecursiveOpContext {
            src_fs: &fs,
            dest_fs: &fs,
            src: &src,
            dest: &dst,
            action: crate::app::CopyMoveAction::Copy,
            cancel: &Arc::new(AtomicBool::new(false)),
            tx: &tx,
            id: 1,
            total: 1,
            processed: &processed,
            decision_rx: &decision_rx,
        };
        let mut decision_state = DecisionState {
            overwrite_all: false,
            skip_all: false,
            last_update: std::time::Instant::now(),
        };
        // Spawn task to send overwrite decision
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                if let crate::tasks::TaskEvent::Conflict(_id, _path, _ty) = event {
                    let _ = decision_tx
                        .send(crate::tasks::TaskDecision::Overwrite)
                        .await;
                }
            }
        });
        // Call operation
        let result = recursive_op(ctx, &mut decision_state).await;
        assert!(result.is_ok());
        // dst must exist
        assert!(fs.files.lock().await.contains_key(&dst));
        // src remains for Copy
        assert!(fs.files.lock().await.contains_key(&src));
    }

    #[tokio::test]
    async fn test_recursive_op_retry() {
        let fs = MockFileSystem::default();
        let src = PathBuf::from("/src.txt");
        let dst = PathBuf::from("/dst.txt");
        fs.files
            .lock()
            .await
            .insert(src.clone(), FakeEntry { is_dir: false });

        // First copy attempt fails, second succeeds
        {
            let mut fails = fs.fail_next_copy.lock().await;
            *fails = 1;
        }

        let (tx, mut rx) = mpsc::unbounded_channel();
        let (decision_tx, decision_rx_real) = mpsc::channel(1);
        let processed = Arc::new(AtomicUsize::new(0));
        let decision_rx = Arc::new(Mutex::new(decision_rx_real));
        let ctx = RecursiveOpContext {
            src_fs: &fs,
            dest_fs: &fs,
            src: &src,
            dest: &dst,
            action: crate::app::CopyMoveAction::Copy,
            cancel: &Arc::new(AtomicBool::new(false)),
            tx: &tx,
            id: 1,
            total: 1,
            processed: &processed,
            decision_rx: &decision_rx,
        };
        let mut decision_state = DecisionState {
            overwrite_all: false,
            skip_all: false,
            last_update: std::time::Instant::now(),
        };

        // Spawn task to send retry decision
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                if let crate::tasks::TaskEvent::Error(_, _, _) = event {
                    let _ = decision_tx.send(crate::tasks::TaskDecision::Retry).await;
                }
            }
        });

        let result = recursive_op(ctx, &mut decision_state).await;

        assert!(result.is_ok());
        assert!(fs.files.lock().await.contains_key(&dst));
    }

    #[tokio::test]
    async fn test_move_rename_optimization_no_conflict() {
        let fs = MockFileSystem::default();
        let src = PathBuf::from("/src.txt");
        let dst = PathBuf::from("/dst.txt");
        fs.files
            .lock()
            .await
            .insert(src.clone(), FakeEntry { is_dir: false });

        let (tx, mut rx) = mpsc::unbounded_channel();
        let (_dtx, drx_real) = mpsc::channel(1);
        let processed = Arc::new(AtomicUsize::new(0));
        let decision_rx = Arc::new(Mutex::new(drx_real));
        let cancel = Arc::new(AtomicBool::new(false));
        let ctx = RecursiveOpContext {
            src_fs: &fs,
            dest_fs: &fs,
            src: &src,
            dest: &dst,
            action: crate::app::CopyMoveAction::Move,
            cancel: &cancel,
            tx: &tx,
            id: 1,
            total: 2,
            processed: &processed,
            decision_rx: &decision_rx,
        };
        let mut decision_state = DecisionState {
            overwrite_all: false,
            skip_all: false,
            last_update: std::time::Instant::now(),
        };

        // If it triggers a conflict, it will wait for decision and we didn't send one,
        // but we can check the events.
        let res = recursive_op(ctx, &mut decision_state);

        // We wrap it in timeout to avoid hanging if the bug is present
        let res = tokio::time::timeout(std::time::Duration::from_millis(500), res).await;

        match res {
            Ok(r) => assert!(r.is_ok(), "Operation failed: {:?}", r.err()),
            Err(_) => {
                // Check if there was a conflict event
                if let Ok(crate::tasks::TaskEvent::Conflict(_, _, _)) = rx.try_recv() {
                    panic!("BUG DETECTED: Conflict triggered for successful move!");
                }
                panic!("Operation timed out - likely waiting for conflict resolution!");
            }
        }

        assert!(!fs.try_exists(&src).await.unwrap());
        assert!(fs.try_exists(&dst).await.unwrap());
    }

    #[tokio::test]
    async fn test_move_rename_optimization_directory() {
        let fs = MockFileSystem::default();
        let src_dir = PathBuf::from("/src_dir");
        let dest_dir = PathBuf::from("/dest_dir");

        {
            let mut files = fs.files.lock().await;
            files.insert(src_dir.clone(), FakeEntry { is_dir: true });
            files.insert(src_dir.join("file1.txt"), FakeEntry { is_dir: false });
            files.insert(src_dir.join("file2.txt"), FakeEntry { is_dir: false });
        }

        let (tx, mut rx) = mpsc::unbounded_channel();
        let (_dtx, drx_real) = mpsc::channel(1);
        let processed = Arc::new(AtomicUsize::new(0));
        let decision_rx = Arc::new(Mutex::new(drx_real));
        let ctx = RecursiveOpContext {
            src_fs: &fs,
            dest_fs: &fs,
            src: &src_dir,
            dest: &dest_dir,
            action: crate::app::CopyMoveAction::Move,
            cancel: &Arc::new(AtomicBool::new(false)),
            tx: &tx,
            id: 1,
            total: 3, // src_dir + file1.txt + file2.txt
            processed: &processed,
            decision_rx: &decision_rx,
        };
        let mut decision_state = DecisionState {
            overwrite_all: false,
            skip_all: false,
            last_update: std::time::Instant::now(),
        };

        let res = recursive_op(ctx, &mut decision_state).await;

        assert!(res.is_ok());

        // Verify progress update
        let mut progress_events = 0;
        let mut last_p = 0;
        while let Ok(event) = rx.try_recv() {
            if let crate::tasks::TaskEvent::UpdateProgress(_, p, _) = event {
                progress_events += 1;
                last_p = p;
            }
        }

        assert_eq!(last_p, 2); // With increment = count - 1 = 2, and total = 3, last_p ends at 2 because the first update sets it to 2 and subsequent updates don't happen (p != total)
        assert!(progress_events >= 1);

        assert!(!fs.try_exists(&src_dir).await.unwrap());
        assert!(fs.try_exists(&dest_dir).await.unwrap());
        assert!(fs.try_exists(&dest_dir.join("file1.txt")).await.unwrap());
        assert!(fs.try_exists(&dest_dir.join("file2.txt")).await.unwrap());
    }
}

#[cfg(test)]
mod decision_state_tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn test_initial_state() {
        let before = Instant::now();
        let d = DecisionState {
            overwrite_all: false,
            skip_all: false,
            last_update: Instant::now(),
        };
        assert!(!d.overwrite_all);
        assert!(!d.skip_all);
        assert!(d.last_update >= before);
    }

    #[test]
    fn test_overwrite_and_skip_flags() {
        let t = Instant::now();
        let mut d = DecisionState {
            overwrite_all: false,
            skip_all: false,
            last_update: t,
        };
        d.overwrite_all = true;
        assert!(d.overwrite_all);
        d.skip_all = true;
        assert!(d.skip_all);
        d.overwrite_all = false;
        assert!(!d.overwrite_all);
    }

    #[test]
    fn test_last_update_mutability() {
        let mut d = DecisionState {
            overwrite_all: false,
            skip_all: false,
            last_update: Instant::now(),
        };
        let old = d.last_update;
        std::thread::sleep(Duration::from_millis(10));
        d.last_update = Instant::now();
        assert!(d.last_update > old);
    }
}
