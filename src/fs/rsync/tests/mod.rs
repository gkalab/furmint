use super::*;
use std::path::PathBuf;

struct MockFs {
    local: bool,
    ctx: String,
}

#[async_trait::async_trait]
impl crate::fs::provider::FileSystemProvider for MockFs {
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
    async fn get_file_info(&self, _: &Path) -> Option<crate::fs::provider::FileMetadata> {
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
    fn context_key(&self) -> crate::fs::provider::ContextKey {
        if self.local {
            crate::fs::provider::ContextKey::Local
        } else if let Some(rest) = self.ctx.strip_prefix("archive:") {
            crate::fs::provider::ContextKey::Archive(std::path::PathBuf::from(rest))
        } else {
            crate::fs::provider::ContextKey::Ssh {
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
