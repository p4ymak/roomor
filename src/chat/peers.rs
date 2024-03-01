use std::{
    collections::{btree_map::Entry, BTreeMap},
    net::Ipv4Addr,
    time::SystemTime,
};

use super::{networker::TIMEOUT_ALIVE, Recepients};

#[derive(Default)]
pub struct PeersMap(pub BTreeMap<Ipv4Addr, Peer>);
impl PeersMap {
    pub fn new() -> Self {
        PeersMap(BTreeMap::<Ipv4Addr, Peer>::new())
    }
    pub fn peer_joined(&mut self, ip: Ipv4Addr, name: Option<impl Into<String>>) -> bool {
        let mut new_one = false;
        if let Entry::Vacant(vip) = self.0.entry(ip) {
            vip.insert(Peer::new(ip, name));
            new_one = true;
        } else if let Some(peer) = self.0.get_mut(&ip) {
            peer.set_last_time(SystemTime::now());
            if name.is_some() {
                peer.set_name(name);
            }
            new_one = peer.is_offline();
            peer.set_presence(Presence::Online);
        }
        new_one
    }
    pub fn peer_exited(&mut self, ip: Ipv4Addr) {
        self.0.entry(ip).and_modify(|p| {
            p.presence = Presence::Offline;
        });
    }
    pub fn remove(&mut self, ip: &Ipv4Addr) {
        self.0.remove(ip);
    }
    pub fn get_display_name(&self, ip: Ipv4Addr) -> String {
        self.0
            .get(&ip)
            .map(|r| r.display_name())
            .unwrap_or(ip.to_string())
    }
    pub fn check_alive(&mut self, now: SystemTime) {
        self.0.values_mut().for_each(|p| p.check_alive(now))
    }
    pub fn online_status(&self, recepients: Recepients) -> Presence {
        match recepients {
            Recepients::One(ip) => self.0.get(&ip).map(|p| p.status()).unwrap_or_default(),
            _ => {
                if self.0.values().any(|p| p.is_online()) {
                    Presence::Online
                } else if self.0.values().all(|p| p.is_offline()) {
                    Presence::Offline
                } else {
                    Presence::Unknown
                }
            }
        }
    }
}

#[derive(Debug, Default, PartialEq, Copy, Clone)]
pub enum Presence {
    Online,
    #[default]
    Unknown,
    Offline,
}

pub struct Peer {
    ip: Ipv4Addr,
    name: Option<String>,
    presence: Presence,
    last_time: SystemTime,
}
impl Peer {
    pub fn new(ip: Ipv4Addr, name: Option<impl Into<String>>) -> Self {
        Peer {
            ip,
            name: name.map(|n| n.into()),
            presence: Presence::Online,
            last_time: SystemTime::now(),
        }
    }
    pub fn has_name(&self) -> bool {
        self.name.is_some()
    }
    pub fn display_name(&self) -> String {
        match &self.name {
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

    pub fn set_name(&mut self, name: Option<impl Into<String>>) {
        self.name = name.map(|n| n.into());
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
