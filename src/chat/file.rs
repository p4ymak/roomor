// FIXME
#![allow(dead_code)]
use std::{
    fs::File,
    path::{Path, PathBuf},
    sync::{atomic::AtomicU8, Arc},
};

use super::message::{new_id, Id};

#[derive(Debug, Clone)]
pub enum FileStatus {
    Link,
    InProgress,
    Ready,
}

#[derive(Debug, Clone)]
pub struct FileLink {
    id: Id,
    pub name: String,
    pub path: PathBuf,
    pub size: u64,
    pub progress: Arc<AtomicU8>,
    pub status: FileStatus,
}

impl FileLink {
    pub fn new(name: &str, dir: &Path, size: u64) -> Self {
        let mut path = dir.to_owned();
        path.push(name);
        FileLink {
            id: new_id(),
            name: name.to_string(),
            path,
            size,
            progress: Arc::new(AtomicU8::new(0)),
            status: FileStatus::InProgress,
        }
    }

    pub fn from_path(path: &Path, progress: Arc<AtomicU8>) -> Option<Self> {
        Some(FileLink {
            id: new_id(),
            name: path.file_name()?.to_string_lossy().to_string(),
            path: path.to_path_buf(),
            size: File::open(path).ok()?.metadata().ok()?.len(),
            progress,
            status: FileStatus::InProgress,
        })
    }
    pub fn id(&self) -> Id {
        self.id
    }
    pub fn to_text(&self) -> String {
        format!("\n{}\n{}\n{}", self.id(), self.name, self.size)
    }
    pub fn from_text(text: &str) -> Option<Self> {
        let mut lines = text.trim().lines();
        let id = lines.next()?.parse::<u32>().ok()?;
        let name = lines.next()?.to_string();
        let size = lines.next()?.parse::<u64>().ok()?;
        Some(FileLink {
            id,
            name,
            path: PathBuf::default(),
            size,
            progress: Arc::new(AtomicU8::new(0)),
            status: FileStatus::InProgress,
        })
    }
}

#[derive(Debug, Clone)]
pub struct FileData {
    id: Id,
    data: Vec<u8>,
}
#[derive(Debug, Clone)]
pub struct FileEnding {
    id: Id,
    checksum: u32,
}
