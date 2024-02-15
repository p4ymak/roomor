use std::{
    collections::{btree_map::Entry, BTreeMap},
    net::Ipv4Addr,
    time::SystemTime,
};

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
            if !peer.is_online() {
                new_one = true;
            }
            if name.is_some() {
                peer.set_name(name);
            }
            peer.set_online(true);
        }
        new_one
    }
    pub fn peer_left(&mut self, ip: Ipv4Addr) {
        self.0.entry(ip).and_modify(|p| p.online = false);
    }
    pub fn get_display_name(&self, ip: Ipv4Addr) -> String {
        self.0
            .get(&ip)
            .map(|r| r.display_name())
            .unwrap_or(ip.to_string())
    }
}
pub struct Peer {
    ip: Ipv4Addr,
    name: Option<String>,
    online: bool,
    last_time: SystemTime,
}
impl Peer {
    pub fn new(ip: Ipv4Addr, name: Option<impl Into<String>>) -> Self {
        Peer {
            ip,
            name: name.map(|n| n.into()),
            online: true,
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
    pub fn is_online(&self) -> bool {
        self.online
    }
    pub fn set_name(&mut self, name: Option<impl Into<String>>) {
        self.name = name.map(|n| n.into());
    }
    pub fn set_online(&mut self, online: bool) {
        self.online = online;
    }
    pub fn last_time(&self) -> SystemTime {
        self.last_time
    }
    pub fn set_last_time(&mut self, time: SystemTime) {
        self.last_time = time;
    }
}
