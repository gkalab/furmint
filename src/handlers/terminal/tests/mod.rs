use super::*;
use crate::test_utils::create_test_app;
use async_trait::async_trait;

struct MockRemoteFs {
    host: Option<String>,
    user: Option<String>,
    port: u16,
}

#[async_trait]
impl crate::fs::provider::FileSystemProvider for MockRemoteFs {
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
    async fn read_file_at(&self, _: &std::path::Path, _: u64, _: usize) -> anyhow::Result<Vec<u8>> {
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
    ) -> Option<crate::fs::provider::FileMetadata> {
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
    fn context_key(&self) -> crate::fs::provider::ContextKey {
        crate::fs::provider::ContextKey::Ssh {
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
