use crate::fs::archive::ArchiveFormat;
use crate::fs::archive::zip::ZipHandler;
use crate::fs::archive_fs::ArchiveFs;
use crate::fs::local::LocalFs;
use crate::fs::provider::FileSystemProvider;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime};

mod rewrite;
mod scan;
mod time;
