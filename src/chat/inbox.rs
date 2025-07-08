use crate::chat::{networker::TIMEOUT_ALIVE, Destination};

use super::{
    file::FileLink,
    message::{Command, Id, Part, ShardCount, UdpMessage, CRC},
    networker::{NetWorker, TIMEOUT_SECOND},
    notifier::Repaintable,
    peers::PeerId,
    BackEvent, Content, ErrorBoxed, Presence, Seen, TextMessage,
};
use log::{debug, error};
use range_rover::RangeTree;
use std::{
    collections::BTreeMap,
    error::Error,
    fs::{self, OpenOptions},
    io::Write,
    net::Ipv4Addr,
    ops::RangeInclusive,
    path::Path,
    sync::Arc,
    time::SystemTime,
};

pub type Shard = Vec<u8>;
// pub const MAX_ATTEMPTS: u8 = 10;

#[derive(Default)]
pub struct Inbox(BTreeMap<Id, InMessage>);
impl Inbox {
    pub fn wake_for_missed_all(&mut self, networker: &mut NetWorker, ctx: &impl Repaintable) {
        let messages = self
            .0
            .values_mut()
            .filter_map(|m| {
                (
                    networker.peers.online_status(m.from_peer_id) != Presence::Offline
                        && !(m.link.is_aborted() || m.link.is_ready())
                        && m.is_old_enough()
                    // * m.attempt.max(1) as u32)
                )
                .then_some(m)
            })
            .collect::<Vec<_>>();

        messages.into_iter().for_each(|m| {
            debug!("Wake for missed");
            m.combine(networker, ctx).ok();
        });
    }
    pub fn wake_for_missed_one(
        &mut self,
        networker: &mut NetWorker,
        ctx: &impl Repaintable,
        peer_id: PeerId,
    ) {
        self.0
            .values_mut()
            .filter(|m| {
                m.from_peer_id == peer_id
                    && !(m.link.is_aborted() || m.link.is_ready())
                    && m.is_old_enough()
                // * m.attempt.max(1) as u32)
            })
            .for_each(|m| {
                debug!("Wake for missed");
                m.combine(networker, ctx).ok();
            });
    }
    pub fn peer_left(&mut self, peer_id: PeerId) {
        self.0.retain(|_, msg| {
            if msg.from_peer_id == peer_id {
                msg.link.abort();
                false
            } else {
                true
            }
        });
    }
    pub fn insert(&mut self, id: Id, msg: InMessage) {
        self.0.insert(id, msg);
    }
    pub fn get_mut(&mut self, id: &Id) -> Option<&mut InMessage> {
        self.0.get_mut(id)
    }
}

#[derive(Default, Debug)]
pub struct CompletedCounter {
    pub ranges: Option<RangeTree<ShardCount>>,
}
impl CompletedCounter {
    fn clear(&mut self) {
        self.ranges = None;
    }
    pub fn insert(&mut self, position: ShardCount) {
        self.ranges
            .get_or_insert(RangeTree::new(position))
            .insert(position);
    }
}
pub struct Shards {
    pub shards: Vec<Option<Shard>>,
    pub buffer_size: ShardCount,
    pub completed: CompletedCounter,
    pub current_part: ShardCount,
    pub offset: ShardCount,
    pub end: ShardCount,
    pub terminal: ShardCount,
    pub attempt: u8,
}
impl Shards {
    pub fn new(end: ShardCount, buffer_size: ShardCount) -> Self {
        let size = buffer_size.min(end + 1);
        Shards {
            shards: vec![None; size as usize],
            buffer_size: size,
            completed: CompletedCounter::default(),
            current_part: 0,
            offset: 0,
            end,
            terminal: end,
            attempt: 0,
        }
    }
    pub fn next_clear(&mut self) {
        self.current_part += 1;
        self.offset += self.buffer_size;
        let size = (self.end - self.offset + 1).min(self.buffer_size);
        self.shards = vec![None; size as usize];
        self.completed.clear();
        self.attempt = 0;
    }
    pub fn clear(&mut self) {
        self.shards.clear();
        self.completed.clear();
    }
    pub fn insert(&mut self, position: ShardCount, msg: UdpMessage) -> Result<(), ErrorBoxed> {
        if let Some(part_position) = position.checked_sub(self.offset) {
            if let Some(shard) = self.shards.get_mut(part_position as usize) {
                if shard.is_some() {
                    return Ok(());
                }

                (msg.checksum() == CRC.checksum(&msg.data))
                    .then_some(())
                    .ok_or("Checksum doesn't match")?;

                *shard = Some(msg.data);

                self.completed.insert(position);
            }
        }
        Ok(())
    }

    pub fn missed(&self) -> Vec<RangeInclusive<ShardCount>> {
        let last = self.shards.len().saturating_sub(1) as ShardCount;
        if let Some(ranges) = &self.completed.ranges {
            ranges.missed_in_range(self.offset..=(self.offset + last))
        } else {
            vec![self.offset..=(self.offset + last)]
        }
    }
}
pub struct InMessage {
    pub ts: SystemTime,
    pub id: Id,
    pub _ip: Ipv4Addr,
    pub from_peer_id: PeerId,
    pub public: bool,
    pub command: Command,
    pub link: Arc<FileLink>,
    pub parts_count: ShardCount,
    pub shards: Shards,
}
impl InMessage {
    pub fn new(
        ip: Ipv4Addr,
        msg: UdpMessage,
        downloads_path: &Path,
        buffer_size: ShardCount,
    ) -> Option<Self> {
        debug!("New Multipart {:?}", msg.command);
        if let Part::Init(init) = msg.part {
            let file_name = if let Command::File = msg.command {
                String::from_utf8(msg.data).unwrap_or(format!("{:?}", SystemTime::now()))
            //FIXME
            } else {
                String::new()
            };
            let size = match msg.command {
                Command::File => buffer_size,
                _ => init.count(),
            };
            let link = FileLink::inbox(msg.id, &file_name, downloads_path, init.count());
            let parts_count = init.count().div_ceil(buffer_size);
            Some(InMessage {
                ts: SystemTime::now(),
                id: msg.id,
                from_peer_id: msg.from_peer_id,
                _ip: ip,
                public: msg.public,
                command: msg.command,
                parts_count,
                link: Arc::new(link),
                shards: Shards::new(init.count().saturating_sub(1), size),
            })
        } else {
            None
        }
    }
    pub fn insert(
        &mut self,
        position: ShardCount,
        msg: UdpMessage,
        networker: &mut NetWorker,
        ctx: &impl Repaintable,
    ) {
        if self.link.is_ready() {
            self.send_seen(networker);
            return;
        }
        if self.link.is_aborted() {
            self.send_abort(networker);
            self.shards.clear();
            return;
        }

        if self
            .shards
            .insert(position, msg)
            .inspect_err(|e| error!("{e}"))
            .is_ok()
        {
            self.ts = SystemTime::now();

            self.link.completed_add(1);
            ctx.request_repaint();

            if self.shards.terminal == position {
                self.combine(networker, ctx).ok();
            }
        }
    }

    pub fn combine(
        &mut self,
        networker: &mut NetWorker,
        ctx: &impl Repaintable,
    ) -> Result<(), Box<dyn Error + 'static>> {
        if self.link.is_ready() {
            self.send_seen(networker);
            return Ok(());
        }
        if self.link.is_aborted() {
            self.send_abort(networker);
            return Ok(());
        }
        debug!(
            "Combining! Current part: {} / {}",
            self.shards.current_part, self.parts_count
        );

        let missed = self.shards.missed();
        debug!("Shards count: {}", self.shards.shards.len());
        if missed.is_empty() {
            let data = std::mem::take(&mut self.shards.shards)
                .into_iter()
                .flatten()
                .flatten()
                .collect::<Vec<u8>>();

            match self.command {
                Command::Text => {
                    let text = String::from_utf8(data)?;
                    let txt_msg = TextMessage {
                        dest: Destination::From(self.from_peer_id),
                        timestamp: self.ts,
                        public: self.public,
                        id: self.id,
                        content: Content::Text(text),
                        seen: Some(Seen::One),
                    };
                    networker
                        .send(
                            UdpMessage::seen_msg(networker.id(), &txt_msg),
                            self.from_peer_id,
                        )
                        .inspect_err(|e| error!("{e}"))
                        .ok();
                    networker.handle_back_event(BackEvent::Message(txt_msg), ctx);
                    Ok(())
                }
                Command::File => {
                    let path = &self.link.path;
                    if let Ok(mut file) = OpenOptions::new().create(false).append(true).open(path) {
                        let written = file.write(&data).inspect_err(|e| error!("{e}")).is_ok();
                        if !written {
                            self.send_abort(networker);
                            self.link.abort();
                        }
                    } else {
                        self.send_abort(networker);
                        self.link.abort();
                    }

                    if self.shards.current_part + 1 < self.parts_count {
                        self.shards.next_clear();
                        let last = self.shards.shards.len().saturating_sub(1) as ShardCount;
                        self.ask_for_missed(
                            networker,
                            vec![self.shards.offset..=(self.shards.offset + last)],
                            false,
                        );
                    } else if rename_file(path).is_ok() {
                        self.send_seen(networker);
                        self.link.set_ready();
                        if self.link.seconds_elapsed() > TIMEOUT_ALIVE.as_secs() {
                            ctx.notify(&self.link.name);
                        }
                        ctx.request_repaint();
                    } else {
                        self.send_abort(networker);
                        self.link.abort();
                    }

                    Ok(())
                    // }
                }
                _ => Ok(()),
            }
        } else {
            debug!("Shards missing!");
            self.ask_for_missed(networker, missed, true);
            Err("Missing Shards".into())
        }
    }

    pub fn is_old_enough(&self) -> bool {
        SystemTime::now()
            .duration_since(self.ts)
            .is_ok_and(|d| d > TIMEOUT_SECOND) // * self.attempt.max(1) as u32)
    }

    pub fn send_seen(&self, networker: &mut NetWorker) {
        networker
            .send(
                UdpMessage::seen_id(networker.id(), self.id, false),
                self.from_peer_id,
            )
            .inspect_err(|e| error!("{e}"))
            .ok();
    }

    pub fn send_abort(&self, networker: &mut NetWorker) {
        networker
            .send(
                UdpMessage::abort(networker.id(), self.id),
                self.from_peer_id,
            )
            .inspect_err(|e| error!("{e}"))
            .ok();
    }

    pub fn ask_for_missed(
        &mut self,
        networker: &mut NetWorker,
        missed: Vec<RangeInclusive<ShardCount>>,
        repeat: bool,
    ) {
        if missed.is_empty() {
            return;
        }
        let terminal = missed
            .last()
            .map(|l| *l.end())
            .unwrap_or(self.link.count.saturating_sub(1));
        if self.shards.terminal == terminal {
            self.shards.attempt = self.shards.attempt.saturating_add(1);
        } else {
            self.shards.terminal = terminal;
        }

        for range in missed {
            if self.link.is_aborted()
                || self.link.is_ready()
                || matches!(
                    networker.peers.online_status(self.from_peer_id),
                    Presence::Offline
                )
            {
                break;
            }
            debug!("Asked to repeat shards #{range:?}");

            self.ts = SystemTime::now();
            networker
                .send(
                    UdpMessage::ask_to_repeat(
                        networker.id(),
                        self.id,
                        Part::AskRange(range),
                        repeat,
                    ),
                    self.from_peer_id,
                )
                .ok();
        }
    }
}

fn rename_file(path: &Path) -> Result<(), ErrorBoxed> {
    let correct_path = path
        .to_str()
        .and_then(|s| s.strip_suffix("_WIP"))
        .ok_or("can't rename")?;
    fs::rename(path, correct_path)?;
    Ok(())
}
// TODO cleanup
// fn offset_range(
//     range: RangeInclusive<ShardCount>,
//     offset: ShardCount,
// ) -> RangeInclusive<ShardCount> {
//     let (s, e) = range.into_inner();
//     RangeInclusive::new(s + offset, e + offset)
// }
