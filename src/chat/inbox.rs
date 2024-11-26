use crate::chat::Destination;

use super::{
    file::FileLink,
    message::{CheckSum, Command, Id, Part, ShardCount, UdpMessage, CRC},
    networker::{NetWorker, TIMEOUT_SECOND},
    notifier::Repaintable,
    peers::PeerId,
    BackEvent, Content, Presence, Seen, TextMessage,
};
use log::{debug, error, warn};
use range_rover::range_rover;
use std::{
    collections::BTreeMap, error::Error, fs, net::Ipv4Addr, ops::RangeInclusive, path::Path,
    sync::Arc, time::SystemTime,
};

pub type Shard = Vec<u8>;
// pub const MAX_ATTEMPTS: u8 = 10;

#[derive(Default)]
pub struct Inbox(BTreeMap<Id, InMessage>);
impl Inbox {
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
    // pub fn retain(&mut self, networker: &mut NetWorker, ctx: &impl Repaintable, delta: Duration) {
    //     self.0.retain(|_, msg| {
    //         !(SystemTime::now()
    //             .duration_since(msg.ts)
    //             .is_ok_and(|d| d > delta * msg.attempt.max(1) as u32)
    //             && (msg.combine(networker, ctx).is_ok()))
    //     });
    // }
    pub fn insert(&mut self, id: Id, msg: InMessage) {
        self.0.insert(id, msg);
    }
    pub fn get_mut(&mut self, id: &Id) -> Option<&mut InMessage> {
        self.0.get_mut(id)
    }
    // pub fn remove(&mut self, id: &Id) {
    //     self.0.remove(id);
    // }
}

pub struct InMessage {
    pub ts: SystemTime,
    pub id: Id,
    pub _ip: Ipv4Addr,
    pub from_peer_id: PeerId,
    pub public: bool,
    pub command: Command,
    pub _total_checksum: CheckSum,
    pub link: Arc<FileLink>,
    pub terminal: ShardCount,
    pub shards: Vec<Option<Shard>>,
    pub attempt: u8,
}
impl InMessage {
    pub fn new(ip: Ipv4Addr, msg: UdpMessage, downloads_path: &Path) -> Option<Self> {
        debug!("New Multipart {:?}", msg.command);
        if let Part::Init(init) = msg.part {
            let file_name = if let Command::File = msg.command {
                String::from_utf8(msg.data).unwrap_or(format!("{:?}", SystemTime::now()))
            //FIXME
            } else {
                String::new()
            };
            let link = FileLink::new(msg.id, &file_name, downloads_path, init.count());
            Some(InMessage {
                ts: SystemTime::now(),
                id: msg.id,
                from_peer_id: msg.from_peer_id,
                _ip: ip,
                public: msg.public,
                command: msg.command,
                _total_checksum: init.checksum(),
                link: Arc::new(link),
                terminal: init.count().saturating_sub(1),
                shards: vec![None; init.count() as usize],
                attempt: 0,
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
        if let Some(block) = self.shards.get_mut(position as usize) {
            self.ts = SystemTime::now();
            if block.is_none() && msg.checksum() == CRC.checksum(&msg.data) {
                *block = Some(msg.data);
                self.link
                    .completed
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                ctx.request_repaint();
            }
        }
        if position == self.terminal {
            warn!("Received terminal {position}");
            self.combine(networker, ctx).ok();
        }
    }

    pub fn missed_shards(&self) -> Vec<RangeInclusive<ShardCount>> {
        let missed = range_rover(
            self.shards
                .iter()
                .enumerate()
                .filter(|s| s.1.is_none())
                .map(|s| s.0 as ShardCount),
        );
        let missed = missed.into_iter().fold(
            vec![],
            |mut r: Vec<RangeInclusive<ShardCount>>, m: RangeInclusive<ShardCount>| {
                if let Some(last) = r.last_mut() {
                    let empty_len = m.start().saturating_sub(*last.end());
                    if empty_len <= m.clone().count() as ShardCount
                    // || empty_len <= last.clone().count() as ShardCount
                    {
                        *last = *last.start()..=*m.end();
                    } else {
                        r.push(m);
                    }
                } else {
                    r.push(m);
                }
                r
            },
        );

        missed
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
        debug!("Combining");
        let missed = self.missed_shards();
        debug!("Shards count: {}", self.shards.len());
        if missed.is_empty() {
            let data = std::mem::take(&mut self.shards)
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
                    debug!("Data lenght: {}", data.len());
                    debug!("Writing new file to {path:?}");
                    let written = fs::write(path, data).inspect_err(|e| error!("{e}")).is_ok();
                    if written {
                        self.send_seen(networker);
                        self.link.set_ready();
                    } else {
                        self.send_abort(networker);
                        self.link.abort();
                    }
                    ctx.request_repaint();
                    Ok(())
                }
                _ => Ok(()),
            }
        } else {
            debug!("Shards missing!");
            self.ask_for_missed(networker, missed);
            Err("Missing Shards".into())
        }
    }

    pub fn is_old_enough(&self) -> bool {
        SystemTime::now()
            .duration_since(self.ts)
            .is_ok_and(|d| d > TIMEOUT_SECOND * self.attempt.max(1) as u32)
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
    ) {
        let terminal = missed
            .last()
            .map(|l| *l.end())
            .unwrap_or(self.link.count.saturating_sub(1));
        if terminal == self.terminal {
            self.attempt = self.attempt.saturating_add(1);
            warn!("New attempt: {}", self.attempt);
        } else {
            self.terminal = terminal;
            warn!("New terminal: {}", self.terminal);
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
                    UdpMessage::ask_to_repeat(networker.id(), self.id, Part::AskRange(range)),
                    self.from_peer_id,
                )
                .ok();
        }
        // networker
        //     .send(
        //         UdpMessage::ask_to_repeat(self.id, Part::AskRange(terminal..=terminal)),
        //         Recepients::One(self.sender),
        //     )
        //     .ok();
    }
}
