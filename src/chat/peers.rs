use super::networker::TIMEOUT_ALIVE;
use crate::app::PUBLIC;
use eframe::egui;
use std::{
    collections::{btree_map::Entry, BTreeMap},
    net::Ipv4Addr,
    time::SystemTime,
};

pub const IDHASH: crc::Crc<u32> = crc::Crc::<u32>::new(&crc::CRC_32_AIXM);

#[derive(Debug, Default, PartialEq, Copy, Clone)]
pub enum Presence {
    Online,
    #[default]
    Unknown,
    Offline,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PeerId(pub u32);
impl PeerId {
    pub fn new(user_name: &str, device_name: &str) -> Self {
        let data = [user_name.as_bytes(), device_name.as_bytes()].concat();
        PeerId(IDHASH.checksum(&data))
    }
    pub fn is_public(&self) -> bool {
        self.0 == 0
    }
    pub const PUBLIC: PeerId = PeerId(0);
}

pub struct Peer {
    ip: Ipv4Addr,
    _id: PeerId,
    name: Option<String>,
    presence: Presence,
    last_time: SystemTime,
}
impl Peer {
    pub fn new(ip: Ipv4Addr, id: PeerId, name: Option<impl Into<String>>) -> Self {
        Peer {
            ip,
            _id: id,
            name: name.map(|n| n.into()),
            presence: Presence::Online,
            last_time: SystemTime::now(),
        }
    }

    pub fn _id(&self) -> PeerId {
        self._id
    }
    pub fn has_name(&self) -> bool {
        self.name.is_some()
    }
    pub fn display_name(&self) -> String {
        match self.name() {
            Some(name) => name.to_string(),
            None => format!("{}", self.ip),
        }
    }
    pub fn name(&self) -> Option<&String> {
        self.name.as_ref()
    }
    pub fn status(&self) -> Presence {
        self.presence
    }
    pub fn is_online(&self) -> bool {
        self.presence == Presence::Online
    }
    pub fn is_offline(&self) -> bool {
        self.presence == Presence::Offline
    }

    pub fn set_name(&mut self, name: impl Into<String>) {
        self.name = Some(name.into());
    }
    pub fn set_presence(&mut self, presence: Presence) {
        self.presence = presence;
    }
    pub fn last_time(&self) -> SystemTime {
        self.last_time
    }
    pub fn set_last_time(&mut self, time: SystemTime) {
        self.last_time = time;
    }
    pub fn ip(&self) -> Ipv4Addr {
        self.ip
    }
    pub fn check_alive(&mut self, now: SystemTime) {
        if self.presence == Presence::Offline {
            return;
        }
        self.presence = if now
            .duration_since(self.last_time)
            .is_ok_and(|t| t < TIMEOUT_ALIVE)
        {
            Presence::Online
        } else {
            Presence::Unknown
        }
    }
}

#[derive(Default)]
pub struct PeersMap {
    pub ids: BTreeMap<PeerId, Peer>,
}
impl PeersMap {
    pub fn new() -> Self {
        PeersMap {
            ids: BTreeMap::<PeerId, Peer>::new(),
        }
    }
    pub fn peer_joined(&mut self, ip: Ipv4Addr, id: PeerId, name: Option<&String>) -> bool {
        let mut new_one = false;
        if let Entry::Vacant(vip) = self.ids.entry(id) {
            let peer = Peer::new(ip, id, name);
            vip.insert(peer);
            new_one = true;
        } else if let Some(peer) = self.ids.get_mut(&id) {
            peer.set_last_time(SystemTime::now());
            peer.ip = ip;
            if let Some(name) = name {
                peer.set_name(name);
            }
            new_one = peer.is_offline();
            peer.set_presence(Presence::Online);
        }
        new_one
    }
    pub fn peer_exited(&mut self, id: PeerId) {
        self.ids.entry(id).and_modify(|p| {
            p.presence = Presence::Offline;
        });
    }

    pub fn remove(&mut self, id: &PeerId) {
        self.ids.remove(id);
    }
    pub fn get_display_name(&self, id: PeerId) -> String {
        self.ids
            .get(&id)
            .map(|r| r.display_name())
            .unwrap_or("Unknown".to_string())
    }
    pub fn check_alive(&mut self, now: SystemTime) {
        self.ids.values_mut().for_each(|p| p.check_alive(now))
    }
    pub fn any_online(&self) -> bool {
        self.ids.values().any(|p| p.is_online())
    }
    pub fn all_offline(&self) -> bool {
        self.ids.values().all(|p| p.is_offline())
    }
    pub fn online_status(&self, peer_id: PeerId) -> Presence {
        if peer_id.is_public() {
            if self.ids.values().any(|p| p.is_online()) {
                Presence::Online
            } else if self.ids.values().all(|p| p.is_offline()) {
                Presence::Offline
            } else {
                Presence::Unknown
            }
        } else {
            self.ids
                .get(&peer_id)
                .map(|p| p.status())
                .unwrap_or_default()
        }
    }

    pub fn rich_public(&self) -> egui::RichText {
        let mut label = egui::RichText::new(PUBLIC);
        if self.ids.values().any(|p| p.is_online()) {
            label = label.strong()
        } else if self.ids.values().all(|p| p.is_online()) {
            label = label.weak();
        }
        label
    }
}
