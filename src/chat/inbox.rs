use super::{
    file::FileLink,
    message::{CheckSum, Command, Id, Part, ShardCount, UdpMessage, CRC},
    networker::{NetWorker, TIMEOUT_SECOND},
    notifier::Repaintable,
    BackEvent, Content, Presence, Recepients, Seen, TextMessage,
};
use log::{debug, error, warn};
use range_rover::range_rover;
use std::{
    collections::BTreeMap,
    error::Error,
    fs,
    net::Ipv4Addr,
    ops::RangeInclusive,
    path::Path,
    sync::Arc,
    time::{Duration, SystemTime},
};

pub type Shard = Vec<u8>;
// pub const MAX_ATTEMPTS: u8 = 10;

#[derive(Default)]
pub struct Inbox(BTreeMap<Id, InMessage>);
impl Inbox {
    pub fn wake_for_missed(&mut self, networker: &mut NetWorker, ip: Ipv4Addr) {
        self.0
            .values_mut()
            .filter(|m| m.sender == ip)
            .filter(|m| {
                SystemTime::now()
                    .duration_since(m.ts)
                    .is_ok_and(|d| d > TIMEOUT_SECOND * m.attempt.max(1) as u32)
            })
            .for_each(|m| {
                m.ask_for_missed(networker);
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
    pub fn remove(&mut self, id: &Id) {
        self.0.remove(id);
    }
}

pub struct InMessage {
    pub ts: SystemTime,
    pub id: Id,
    pub sender: Ipv4Addr,
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
            let link = FileLink::new(&file_name, downloads_path, init.count());
            Some(InMessage {
                ts: SystemTime::now(),
                id: msg.id,
                sender: ip,
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
    ) -> bool {
        if self.link.is_ready() {
            return true;
        }
        if self.link.is_aborted() {
            return false;
        }
        self.ts = SystemTime::now();
        if let Some(block) = self.shards.get_mut(position as usize) {
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
            return self.combine(networker, ctx).is_ok();
        }
        false
    }

    pub fn missed_shards(&self) -> Vec<RangeInclusive<ShardCount>> {
        range_rover(
            self.shards
                .iter()
                .enumerate()
                .filter(|s| s.1.is_none())
                .map(|s| s.0 as ShardCount),
        )
    }

    pub fn combine(
        &mut self,
        networker: &mut NetWorker,
        ctx: &impl Repaintable,
    ) -> Result<(), Box<dyn Error + 'static>> {
        if self.link.is_ready() {
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
                        timestamp: self.ts,
                        incoming: true,
                        public: self.public,
                        ip: self.sender,
                        id: self.id,
                        content: Content::Text(text),
                        seen: Some(Seen::One),
                    };
                    networker
                        .send(UdpMessage::seen_msg(&txt_msg), Recepients::One(self.sender))
                        .inspect_err(|e| error!("{e}"))
                        .ok();
                    networker.handle_back_event(BackEvent::Message(txt_msg), ctx);
                    Ok(())
                }
                Command::File => {
                    let path = &self.link.path;
                    debug!("Data lenght: {}", data.len());
                    debug!("Writing new file to {path:?}");
                    fs::write(path, data).inspect_err(|e| error!("{e}"))?;

                    self.link.set_ready();
                    ctx.request_repaint();
                    Ok(())
                }
                _ => Ok(()),
            }
        } else {
            error!("Shards missing!");
            self.ask_for_missed(networker);

            Err("Missing Shards".into())
        }
    }
    pub fn ask_for_missed(&mut self, networker: &mut NetWorker) {
        let missed = self.missed_shards();
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
        // TODO save outbox
        if !matches!(
            networker.peers.online_status(Recepients::One(self.sender)),
            Presence::Offline
        ) {
            for range in missed {
                if self.link.is_aborted() {
                    break;
                }
                debug!("Asked to repeat shards #{range:?}");
                if matches!(
                    networker.peers.online_status(Recepients::One(self.sender)),
                    Presence::Online
                ) {
                    self.ts = SystemTime::now();
                    networker
                        .send(
                            UdpMessage::ask_to_repeat(self.id, Part::AskRange(range)),
                            Recepients::One(self.sender),
                        )
                        .ok();
                }
            }
        }
    }
}
