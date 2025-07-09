use flume::Receiver;
use log::error;

use super::{
    message::{send_shards, Id, ShardCount, DATA_LIMIT_BYTES},
    notifier::Repaintable,
    peers::PeerId,
    ErrorBoxed, Recepients,
};
use std::{
    fs::{File, OpenOptions},
    net::{SocketAddrV4, UdpSocket},
    ops::RangeInclusive,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::SystemTime,
};

pub struct ShardsInfo {
    pub link: Arc<FileLink>,
    pub range: RangeInclusive<ShardCount>,
    pub id: Id,
    pub recepients: Recepients,
}
impl ShardsInfo {
    pub fn new(
        link: Arc<FileLink>,
        range: RangeInclusive<ShardCount>,
        id: Id,
        recepients: Recepients,
    ) -> Self {
        ShardsInfo {
            link,
            range,
            id,
            recepients,
        }
    }
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
    pub breath: AtomicBool,
    pub inbox: bool,
}

impl FileLink {
    pub fn inbox(id: Id, name: &str, dir: &Path, count: ShardCount) -> Self {
        let mut aborted = false;
        let mut path = dir.to_owned();
        path.push(name);
        while std::fs::exists(&path).is_ok_and(|t| t) {
            path = increment_path(&path).unwrap_or(path);
            error!("INCREMENT: {path:?}");
        }
        if let Some(path_wip) = path_wip(&path) {
            let _file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&path_wip)
                .inspect_err(|e| {
                    aborted = true;
                    error!("{e} : {path_wip:?}")
                });
        } else {
            aborted = true
        }
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
            is_aborted: AtomicBool::new(aborted),
            breath: AtomicBool::new(false),
            inbox: true,
        }
    }

    pub fn outbox(id: Id, path: &Path) -> Option<Self> {
        let size = File::open(path).ok()?.metadata().ok()?.len();

        Some(FileLink {
            id,
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
            breath: AtomicBool::new(false),
            inbox: false,
        })
    }
    pub fn id(&self) -> Id {
        self.id
    }
    pub fn progress(&self) -> f32 {
        (self.completed.load(Ordering::Relaxed) as f32 / self.count as f32).min(0.99)
    }
    pub fn abort(&self) {
        if self.inbox {
            if let Some(path_wip) = path_wip(&self.path) {
                std::fs::remove_file(&path_wip).ok();
            }
        }
        self.is_aborted.store(true, Ordering::Relaxed);
        self.breath_in();
    }
    pub fn set_ready(&self) {
        self.is_ready.store(true, Ordering::Relaxed);

        let seconds = SystemTime::now()
            .duration_since(self.time_start)
            .map(|d| d.as_secs_f32())
            .unwrap_or(f32::MIN_POSITIVE);
        let bandwidth = self.size as f32 / seconds.max(f32::MIN_POSITIVE);

        self.seconds_elapsed
            .store(seconds as u64, Ordering::Relaxed);
        self.bandwidth.store(bandwidth as u64, Ordering::Relaxed);
        self.breath_in();
    }
    pub fn is_aborted(&self) -> bool {
        self.is_aborted.load(Ordering::Relaxed)
    }
    pub fn is_ready(&self) -> bool {
        self.is_ready.load(Ordering::Relaxed)
    }
    pub fn bandwidth(&self) -> u64 {
        self.bandwidth.load(Ordering::Relaxed)
    }
    pub fn completed_add(&self, value: u64) {
        let mut completed = self.completed.load(Ordering::Relaxed);
        completed = completed.saturating_add(value);
        completed = completed.min(self.count);
        self.completed.store(completed, Ordering::Relaxed);
        self.breath_in();
    }
    pub fn completed_sub(&self, value: u64) {
        let mut completed = self.completed.load(Ordering::Relaxed);
        completed = completed.saturating_sub(value);
        self.completed.store(completed, Ordering::Relaxed);
        self.breath_in();
        self.breath_in();
    }
    pub fn breath_in(&self) {
        self.breath.store(true, Ordering::Relaxed);
    }

    pub fn breath_out(&self) -> bool {
        let breath = self.breath.load(Ordering::Relaxed);
        self.breath.store(false, Ordering::Relaxed);
        breath
    }
    pub fn seconds_elapsed(&self) -> u64 {
        self.seconds_elapsed.load(Ordering::Relaxed)
    }
}

pub fn shards_sender(
    peer_id: PeerId,
    socket: Arc<UdpSocket>,
    multicast: SocketAddrV4,
    ctx: &impl Repaintable,
    rx: Receiver<ShardsInfo>,
) {
    for shards_info in rx.iter() {
        if send_shards(peer_id, shards_info, socket.clone(), multicast, ctx.clone()).is_err() {
            return;
        }
    }
}

pub fn rename_file(path: &Path) -> Result<(), ErrorBoxed> {
    let correct_path = PathBuf::from(
        path.to_str()
            .and_then(|s| s.strip_suffix("_WIP"))
            .ok_or("can't rename")?,
    );

    std::fs::rename(path, correct_path)?;
    Ok(())
}

pub fn path_wip(path: &Path) -> Option<PathBuf> {
    let file_name = &path.file_name().and_then(|f| f.to_str())?;
    let mut path_wip = path.to_owned();
    path_wip.set_file_name(format!("{file_name}_WIP"));
    Some(path_wip)
}

fn increment_path(path: &Path) -> Option<PathBuf> {
    let mut path = path.to_path_buf();
    let mut postfix_changed = false;
    let file_ext = path.extension()?.to_str()?;
    let mut file_stem = path.file_stem()?.to_str()?.to_string();
    if let Some((n, p)) = file_stem.rsplit_once('_') {
        let post = if let Ok(num) = p.parse::<u64>() {
            postfix_changed = true;
            (num + 1).to_string()
        } else {
            p.to_string()
        };
        file_stem = format!("{n}_{post}");
    }
    if !postfix_changed {
        file_stem.push_str("_1");
    }
    if !file_ext.is_empty() {
        file_stem.push('.');
        file_stem.push_str(file_ext);
    }
    path.set_file_name(file_stem);
    Some(path)
}
