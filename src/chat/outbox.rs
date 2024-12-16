use std::{collections::BTreeMap, sync::Arc, thread, time::SystemTime};

use flume::Sender;

use super::{
    file::{shards_sender, FileLink, ShardsInfo},
    message::{Id, UdpMessage},
    networker::{NetWorker, TIMEOUT_CHECK},
    notifier::Repaintable,
    peers::PeerId,
    ErrorBoxed,
};

#[derive(Default)]
pub struct Outbox {
    pub texts: BTreeMap<PeerId, Vec<OutMessage>>,
    pub files: BTreeMap<Id, (Arc<FileLink>, Sender<ShardsInfo>)>,
}

pub struct OutMessage {
    ts: SystemTime,
    msg: UdpMessage,
}
impl OutMessage {
    pub fn new(msg: UdpMessage) -> Self {
        OutMessage {
            ts: SystemTime::UNIX_EPOCH,
            msg,
        }
    }
    pub fn id(&self) -> Id {
        self.msg.id
    }
}
impl Outbox {
    pub fn add(&mut self, peer_id: PeerId, msg: UdpMessage) {
        self.texts
            .entry(peer_id)
            .and_modify(|h| h.push(OutMessage::new(msg.clone())))
            .or_insert(vec![OutMessage::new(msg)]);
    }
    pub fn remove(&mut self, peer_id: PeerId, id: Id) {
        self.texts
            .entry(peer_id)
            .and_modify(|h| h.retain(|m| m.id() != id));
    }
    pub fn get(&self, peer_id: PeerId, id: Id) -> Option<&UdpMessage> {
        self.texts
            .get(&peer_id)
            .and_then(|h| h.iter().find(|m| m.id() == id))
            .map(|m| &m.msg)
    }
    pub fn undelivered(&mut self, id: PeerId) -> Vec<&UdpMessage> {
        let now = SystemTime::now();
        if let Some(history) = self.texts.get_mut(&id) {
            history
                .iter_mut()
                .filter_map(|msg| {
                    now.duration_since(msg.ts)
                        .is_ok_and(|t| t > TIMEOUT_CHECK)
                        .then_some({
                            msg.ts = now;
                            &msg.msg
                        })
                })
                .collect()
        } else {
            vec![]
        }
    }
    pub fn new_file(
        &mut self,
        networker: &NetWorker,
        ctx: &impl Repaintable,
        msg_id: Id,
        link: Arc<FileLink>,
    ) -> Result<(), ErrorBoxed> {
        let (tx, rx) = flume::unbounded::<ShardsInfo>();
        let socket = networker.socket.as_ref().ok_or("No Socket")?.clone();
        let multicast_port = networker.multicast;
        let ctx = ctx.clone();
        let peer_id = networker.id();
        thread::Builder::new()
            .name(format!("shards_sender_{msg_id}"))
            .spawn(move || shards_sender(peer_id, socket, multicast_port, &ctx, rx))?;
        self.files.insert(msg_id, (link, tx));
        Ok(())
    }
}
