// FIXME
#![allow(dead_code)]
use std::{
    fs::File,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicU64},
};

use super::message::{new_id, Id, ShardCount, DATA_LIMIT_BYTES};

#[derive(Debug, Clone)]
pub enum FileStatus {
    Link,
    InProgress,
    Ready,
}

#[derive(Debug)]
pub struct LinkFile {
    id: Id,
    pub name: String,
    pub path: PathBuf,
    pub size: u64,
    pub count: ShardCount,
    pub completed: AtomicU64,
    pub is_ready: AtomicBool,
}

impl LinkFile {
    pub fn new(name: &str, dir: &Path, count: ShardCount) -> Self {
        let mut path = dir.to_owned();
        path.push(name);
        LinkFile {
            id: new_id(),
            name: name.to_string(),
            path,
            size: count * DATA_LIMIT_BYTES as ShardCount,
            count,
            completed: AtomicU64::new(0),
            is_ready: AtomicBool::new(false),
        }
    }

    pub fn from_path(path: &Path) -> Option<Self> {
        let size = File::open(path).ok()?.metadata().ok()?.len();
        Some(LinkFile {
            id: new_id(),
            name: path.file_name()?.to_string_lossy().to_string(),
            path: path.to_path_buf(),
            size,
            count: size.div_ceil(DATA_LIMIT_BYTES as ShardCount),
            completed: AtomicU64::new(0),
            is_ready: AtomicBool::new(false),
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
        Some(LinkFile {
            id,
            name,
            path: PathBuf::default(),
            size,
            count: size.div_ceil(DATA_LIMIT_BYTES as ShardCount),
            completed: AtomicU64::new(0),
            is_ready: AtomicBool::new(false),
        })
    }
    pub fn progress(&self) -> f32 {
        self.completed.load(std::sync::atomic::Ordering::Relaxed) as f32 / self.count as f32
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
