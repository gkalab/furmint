pub mod archive;
pub mod fs_archive;
pub mod fs_local;
pub mod fs_provider;
pub mod fs_rsync;
#[cfg(unix)]
pub mod fs_sftp;
#[cfg(windows)]
pub mod fs_sftp_russh;
#[cfg(windows)]
pub mod fs_sftp {
    pub use super::fs_sftp_russh::SftpFs;
}
pub mod local;
pub mod ops;
pub mod provider;
pub mod traits;
pub mod utils;
pub mod watcher;
