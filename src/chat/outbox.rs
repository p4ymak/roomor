use std::{collections::BTreeMap, sync::Arc, time::SystemTime};

use super::{
    file::FileLink,
    message::{Id, UdpMessage},
    networker::TIMEOUT_CHECK,
    peers::PeerId,
};

#[derive(Default)]
pub struct Outbox {
    pub texts: BTreeMap<PeerId, Vec<OutMessage>>,
    pub files: BTreeMap<Id, Arc<FileLink>>,
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
}
