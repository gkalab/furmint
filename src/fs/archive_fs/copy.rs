//! Copying into and out of an archive, and rescanning its index.

use super::ArchiveFs;
use crate::fs::archive::NewEntry;
use crate::fs::provider::{FileSystemProvider, TaskProgressContext};
use anyhow::Result;
use std::path::{Path, PathBuf};

/// Cap on how much entry data is decoded into memory before the destination
/// archive is rewritten. Archives can only be rewritten wholesale, so a batch
/// boundary costs one full rewrite.
const ADD_BATCH_BUDGET: usize = 64 * 1024 * 1024;

impl ArchiveFs {
    pub(super) fn scan_archive(&self) -> Result<()> {
        let scan = self.handler.scan()?;
        *self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = scan.entries;
        *self
            .tree
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = scan.tree;
        *self
            .symlinks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = scan.symlinks;
        *self
            .read_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        Ok(())
    }

    #[cfg(test)]
    pub async fn extract(
        &self,
        src: &Path,
        dest_fs: &dyn FileSystemProvider,
        dest: &Path,
        progress: &TaskProgressContext,
    ) -> Option<anyhow::Result<()>> {
        self.copy_to_local(src, dest_fs, dest, progress).await
    }

    /// Copies a file or a whole directory tree from `src_fs` into this archive.
    ///
    /// Archives cannot be appended to piecemeal, so entries are collected from
    /// the source provider, decoded into memory, and written in batches: one
    /// rewrite per batch rather than one per entry.
    pub(super) async fn copy_from_source_into(
        &self,
        src_fs: &dyn FileSystemProvider,
        src: &Path,
        dest: &Path,
        progress: &TaskProgressContext,
    ) -> anyhow::Result<()> {
        let dest_str = Self::archive_internal_dest(dest);

        if !src_fs.is_dir(src).await {
            let entry = NewEntry {
                dest: dest_str,
                mtime: src_fs.get_modified_time(src).await,
                mode: None,
                data: Some(src_fs.read_file(src).await?),
            };
            return self.add_entries_blocking(vec![entry]).await;
        }

        let mut queue: Vec<(PathBuf, String)> = vec![(src.to_path_buf(), dest_str.clone())];
        let mut batch: Vec<NewEntry> = vec![NewEntry {
            dest: dest_str,
            mtime: src_fs.get_modified_time(src).await,
            mode: None,
            data: None,
        }];
        let mut buffered = 0usize;

        while let Some((dir_src, dir_dest)) = queue.pop() {
            let children = src_fs.read_dir(&dir_src).await?;
            for child in children {
                if progress.cancel.load(std::sync::atomic::Ordering::Relaxed) {
                    return Ok(());
                }

                let Some(name) = child.file_name() else {
                    continue;
                };
                let name = name.to_string_lossy().to_string();
                let child_dest = if dir_dest.is_empty() {
                    name
                } else {
                    format!("{dir_dest}/{name}")
                };

                if src_fs.is_dir(&child).await {
                    batch.push(NewEntry {
                        dest: child_dest.clone(),
                        mtime: src_fs.get_modified_time(&child).await,
                        mode: None,
                        data: None,
                    });
                    queue.push((child, child_dest));
                } else {
                    let data = src_fs.read_file(&child).await?;
                    buffered += data.len();
                    batch.push(NewEntry {
                        dest: child_dest,
                        mtime: src_fs.get_modified_time(&child).await,
                        // Archive sources do not expose the stored unix mode, so
                        // the format default is used.
                        mode: None,
                        data: Some(data),
                    });
                }

                if buffered >= ADD_BATCH_BUDGET {
                    self.add_entries_blocking(std::mem::take(&mut batch))
                        .await?;
                    buffered = 0;
                }
            }
        }

        if !batch.is_empty() {
            self.add_entries_blocking(batch).await?;
        }
        Ok(())
    }

    /// Appends a batch of entries and rescans, off the async runtime.
    async fn add_entries_blocking(&self, entries: Vec<NewEntry>) -> anyhow::Result<()> {
        let handler = self.handler.clone();
        let this = self.clone();
        tokio::task::spawn_blocking(move || {
            handler.add_entries(&entries)?;
            this.scan_archive()?;
            anyhow::Ok(())
        })
        .await
        .unwrap_or_else(|e| Err(anyhow::anyhow!("Join error: {e}")))
    }
}
