use anyhow::Result;
use std::path::Path;

pub trait FileOpener: Send + Sync {
    fn open(&self, path: &Path) -> Result<()>;
}

pub struct SystemOpener;

impl FileOpener for SystemOpener {
    fn open(&self, path: &Path) -> Result<()> {
        #[cfg(target_os = "linux")]
        {
            let _ = std::process::Command::new("xdg-open")
                .arg(path)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
            Ok(())
        }
        #[cfg(not(target_os = "linux"))]
        {
            open::that(path).map_err(|e| anyhow::anyhow!("Error opening file: {}", e))
        }
    }
}
