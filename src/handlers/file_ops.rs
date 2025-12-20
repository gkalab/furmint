//! File operations helpers: recursive ops, item counters, decision state

// Helper to count items recursively
pub fn count_items(paths: &[std::path::PathBuf]) -> usize {
    let mut count = 0;
    for path in paths {
        count += 1; // Count the item itself
        if path.is_dir()
            && let Ok(entries) = std::fs::read_dir(path)
        {
            let mut children = Vec::new();
            for entry in entries.flatten() {
                children.push(entry.path());
            }
            count += count_items(&children);
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
// Recursive operation
// Returns Result<(), String>
// Recursive operation
// Returns Result<(), String>
// Iterative operation to avoid stack overflow
// Returns Result<(), String>
pub fn recursive_op<'a>(
    src: &'a std::path::Path,
    dest: &'a std::path::Path,
    action: crate::app::CopyMoveAction,
    cancel: &'a std::sync::Arc<std::sync::atomic::AtomicBool>,
    tx: &'a tokio::sync::mpsc::UnboundedSender<crate::tasks::TaskEvent>,
    id: usize,
    total: usize,
    processed: &'a std::sync::Arc<std::sync::atomic::AtomicUsize>,
    decision_rx: &'a std::sync::Arc<
        tokio::sync::Mutex<tokio::sync::mpsc::Receiver<crate::tasks::TaskDecision>>,
    >,
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
            src: src.to_path_buf(),
            dest: dest.to_path_buf(),
        }];

        while let Some(item) = stack.pop() {
            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                return Ok(()); // Cancelled
            }

            match item {
                WorkItem::PostProcessDir { src } => {
                    // Remove empty directory after move
                    let _ = tokio::fs::remove_dir(src).await;
                }
                WorkItem::Process { src, dest } => {
                    // Move optimization: Try rename first if it's a move operation
                    if action == crate::app::CopyMoveAction::Move {
                        // Only try rename if dest doesn't exist to avoid implicit overwrite
                        if let Ok(false) = tokio::fs::try_exists(&dest).await
                            && tokio::fs::rename(&src, &dest).await.is_ok()
                        {
                            // Success, no need to process children or post-process
                            continue;
                        }
                    }

                    if src.is_dir() {
                        // Directory handling
                        let dest_exists = tokio::fs::try_exists(&dest).await.unwrap_or(false);

                        if !dest_exists {
                            if let Err(e) = tokio::fs::create_dir_all(&dest).await {
                                return Err(format!(
                                    "Failed to create directory {}: {}",
                                    dest.display(),
                                    e
                                ));
                            }
                        } else if !dest.is_dir() {
                            return Err(format!(
                                "Destination {} exists and is not a directory",
                                dest.display()
                            ));
                        }

                        // If Move, we need to remove this dir AFTER processing children
                        if action == crate::app::CopyMoveAction::Move {
                            stack.push(WorkItem::PostProcessDir { src: src.clone() });
                        }

                        // Read children
                        let mut entries = match tokio::fs::read_dir(&src).await {
                            Ok(e) => e,
                            Err(e) => {
                                return Err(format!(
                                    "Failed to read directory {}: {}",
                                    src.display(),
                                    e
                                ));
                            }
                        };

                        while let Ok(Some(entry)) = entries.next_entry().await {
                            let path = entry.path();
                            let name = match path.file_name() {
                                Some(n) => n,
                                None => continue,
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
                        let dest_exists = tokio::fs::try_exists(&dest).await.unwrap_or(false);

                        if dest_exists {
                            // Conflict resolution
                            if decision_state.overwrite_all {
                                perform = true;
                            } else if decision_state.skip_all {
                                perform = false;
                            } else {
                                // Ask user
                                let _ = tx.send(crate::tasks::TaskEvent::Conflict(
                                    id,
                                    dest.clone(),
                                    crate::tasks::ConflictType::FileExists,
                                ));

                                // Wait for decision
                                let mut decision = None;
                                if let Some(rx) = decision_rx.try_lock().ok().as_mut() {
                                    // We need to wait for a decision.
                                    // NOTE: This blocks the async task, but that's what we want.
                                    // The UI runs in a separate thread/event loop.
                                    decision = rx.recv().await;
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
                                    Some(crate::tasks::TaskDecision::Cancel) => return Ok(()),
                                    _ => perform = false, // Default skip or error
                                }
                            }
                        }

                        if perform {
                            loop {
                                if dest_exists {
                                    // Try to remove destination if it exists (overwrite)
                                    let _ = tokio::fs::remove_file(&dest).await;
                                }

                                match tokio::fs::copy(&src, &dest).await {
                                    Ok(_) => break, // Success
                                    Err(e) => {
                                        // Check SkipAll flag
                                        if decision_state.skip_all {
                                            perform = false;
                                            break;
                                        }

                                        // Ask user
                                        let _ = tx.send(crate::tasks::TaskEvent::Error(
                                            id,
                                            src.display().to_string(),
                                            format!("Failed to copy to {}: {}", dest.display(), e),
                                        ));

                                        // Wait for decision
                                        let mut decision = None;
                                        if let Some(rx) = decision_rx.try_lock().ok().as_mut() {
                                            decision = rx.recv().await;
                                        }

                                        match decision {
                                            Some(crate::tasks::TaskDecision::Retry) => continue, // Retry loop
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

                        // If Move AND perform was success (or if we skipped, we usually DON'T delete source?
                        // Wait, if we 'Skip', we shouldn't delete source in a Move.
                        // Standard Move behavior: if we copy successfully, we delete source.
                        // If we skip copying, the source remains.
                        // So only delete source if perform was true AND success.
                        if action == crate::app::CopyMoveAction::Move && perform {
                            let _ = tokio::fs::remove_file(&src).await;
                        }

                        // Update progress
                        let p = processed.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                        let now = std::time::Instant::now();
                        if now.duration_since(decision_state.last_update)
                            > std::time::Duration::from_millis(100)
                            || p == total
                        {
                            let _ = tx.send(crate::tasks::TaskEvent::UpdateProgress(id, p, total));
                            decision_state.last_update = now;
                        }
                    }
                }
            }
        }
        Ok(())
    })
}
