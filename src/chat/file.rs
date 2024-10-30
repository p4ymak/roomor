// FIXME
#![allow(dead_code)]
use super::message::{new_id, Id, ShardCount, DATA_LIMIT_BYTES};
use std::{
    fs::File,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicU64},
    time::SystemTime,
};

#[derive(Debug, Clone)]
pub enum FileStatus {
    Link,
    InProgress,
    Ready,
}

#[derive(Debug)]
pub struct FileLink {
    id: Id,
    time_start: SystemTime,
    seconds_elapsed: AtomicU64,
    bandwidth: AtomicU64,
    pub name: String,
    pub path: PathBuf,
    pub size: u64,
    pub count: ShardCount,
    pub completed: AtomicU64,
    pub is_ready: AtomicBool,
    pub is_aborted: AtomicBool,
}

impl FileLink {
    pub fn new(id: Id, name: &str, dir: &Path, count: ShardCount) -> Self {
        let mut path = dir.to_owned();
        path.push(name);
        FileLink {
            id,
            time_start: SystemTime::now(),
            seconds_elapsed: AtomicU64::new(1),
            bandwidth: AtomicU64::new(0),
            name: name.to_string(),
            path,
            size: count * DATA_LIMIT_BYTES as ShardCount,
            count,
            completed: AtomicU64::new(0),
            is_ready: AtomicBool::new(false),
            is_aborted: AtomicBool::new(false),
        }
    }

    pub fn from_path(path: &Path) -> Option<Self> {
        let size = File::open(path).ok()?.metadata().ok()?.len();
        Some(FileLink {
            id: new_id(),
            time_start: SystemTime::now(),
            seconds_elapsed: AtomicU64::new(1),
            bandwidth: AtomicU64::new(0),
            name: path.file_name()?.to_string_lossy().to_string(),
            path: path.to_path_buf(),
            size,
            count: size.div_ceil(DATA_LIMIT_BYTES as ShardCount),
            completed: AtomicU64::new(0),
            is_ready: AtomicBool::new(false),
            is_aborted: AtomicBool::new(false),
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
            time_start: SystemTime::now(),
            seconds_elapsed: AtomicU64::new(1),
            bandwidth: AtomicU64::new(0),
            name,
            path: PathBuf::default(),
            size,
            count: size.div_ceil(DATA_LIMIT_BYTES as ShardCount),
            completed: AtomicU64::new(0),
            is_ready: AtomicBool::new(false),
            is_aborted: AtomicBool::new(false),
        })
    }
    pub fn progress(&self) -> f32 {
        (self.completed.load(std::sync::atomic::Ordering::Relaxed) as f32 / self.count as f32)
            .min(0.99)
    }
    pub fn abort(&self) {
        self.is_aborted
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
    pub fn set_ready(&self) {
        self.is_ready
            .store(true, std::sync::atomic::Ordering::Relaxed);

        let seconds = SystemTime::now()
            .duration_since(self.time_start)
            .map(|d| d.as_secs())
            .unwrap_or(1);
        let bandwidth = self.size / seconds.max(1);

        self.seconds_elapsed
            .store(seconds, std::sync::atomic::Ordering::Relaxed);
        self.bandwidth
            .store(bandwidth, std::sync::atomic::Ordering::Relaxed);
    }
    pub fn is_aborted(&self) -> bool {
        self.is_aborted.load(std::sync::atomic::Ordering::Relaxed)
    }
    pub fn is_ready(&self) -> bool {
        self.is_ready.load(std::sync::atomic::Ordering::Relaxed)
    }
    pub fn bandwidth(&self) -> u64 {
        self.bandwidth.load(std::sync::atomic::Ordering::Relaxed)
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
