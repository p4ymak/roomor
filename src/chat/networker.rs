use super::{message::UdpMessage, notifier::Repaintable, peers::PeersMap, BackEvent, Recepients};
use flume::Sender;
use ipnet::{ipv4_mask_to_prefix, Ipv4Net};
use log::debug;
use std::{
    error::Error,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket},
    str::FromStr,
    sync::Arc,
    time::{Duration, SystemTime},
};

pub const TIMEOUT_ALIVE: Duration = Duration::from_secs(60);
pub const TIMEOUT_CHECK: Duration = Duration::from_secs(10);

pub struct NetWorker {
    pub name: String,
    pub socket: Option<Arc<UdpSocket>>,
    pub ip: Ipv4Addr,
    pub port: u16,
    pub peers: PeersMap,
    ipnet: Ipv4Net,
    pub front_tx: Sender<BackEvent>,
}

impl NetWorker {
    pub fn new(ip: Ipv4Addr, front_tx: Sender<BackEvent>) -> Self {
        NetWorker {
            name: String::new(),
            socket: None,
            ip,
            port: 4444,
            peers: PeersMap::new(),
            ipnet: Ipv4Net::default(),
            front_tx,
        }
    }
    pub fn connect(&mut self, mask: u8) -> Result<(), Box<dyn Error + 'static>> {
        self.ipnet = Ipv4Net::new(self.ip, mask)?;
        let socket = UdpSocket::bind(SocketAddrV4::new(self.ip, self.port))?;
        socket.set_broadcast(true)?;
        socket.set_multicast_loop_v4(false)?;
        socket.set_nonblocking(false)?;
        self.socket = Some(Arc::new(socket));

        Ok(())
    }

    pub fn send(&mut self, message: UdpMessage, mut addrs: Recepients) {
        let bytes = message.to_be_bytes();
        if let Some(socket) = &self.socket {
            if self.peers.0.is_empty() {
                addrs = Recepients::All;
            }
            match addrs {
                Recepients::All => self
                    .ipnet
                    .hosts()
                    .map(|r| {
                        socket
                            .send_to(&bytes, SocketAddrV4::new(r, self.port))
                            .is_ok()
                    })
                    .all(|r| r),
                Recepients::Peers => self
                    .peers
                    .0
                    .keys()
                    .map(|ip| {
                        socket
                            .send_to(&bytes, SocketAddrV4::new(*ip, self.port))
                            .is_ok()
                    })
                    .all(|r| r),
                Recepients::One(ip) => socket
                    .send_to(&bytes, SocketAddrV4::new(ip, self.port))
                    .is_ok(),
                Recepients::Myself => socket
                    .send_to(&bytes, SocketAddrV4::new(self.ip, self.port))
                    .is_ok(),
            };
            debug!("Sent '{:?}' to {addrs:?}", message.command);
        }
    }

    pub fn handle_event(&mut self, event: BackEvent, ctx: &impl Repaintable) {
        match event {
            BackEvent::PeerJoined((ip, ref user_name)) => {
                let new_comer = self.peers.peer_joined(ip, user_name.clone());
                self.front_tx.send(event).ok();
                if new_comer {
                    self.send(UdpMessage::greating(&self.name), Recepients::One(ip));
                    // let notification_text = format!("{} joined..", self.peers.get_display_name(ip));
                    // ctx.notify(&notification_text);
                    // } else {
                    // ctx.request_repaint();
                }
                ctx.request_repaint();
            }
            BackEvent::PeerLeft(ip) => {
                // let notification_text = format!("{} left..", self.peers.get_display_name(ip));
                self.peers.remove(&ip);
                self.front_tx.send(BackEvent::PeerLeft(ip)).ok();
                // ctx.notify(&notification_text);
                ctx.request_repaint();
            }
            BackEvent::Message(msg) => {
                let text = msg.get_text();
                let name = self.peers.get_display_name(msg.ip());
                let notification_text = format!("{name}: {text}");
                self.front_tx.send(BackEvent::Message(msg)).ok();
                ctx.notify(&notification_text);
            }
        }
    }

    pub fn incoming(&mut self, ip: Ipv4Addr, my_name: &str) {
        self.front_tx.send(BackEvent::PeerJoined((ip, None))).ok();
        match self.peers.0.get_mut(&ip) {
            None => {
                let noname: Option<&str> = None;
                self.peers.peer_joined(ip, noname);
                self.send(UdpMessage::ask_name(), Recepients::One(ip));
                self.send(UdpMessage::greating(my_name), Recepients::One(ip));
            }
            Some(peer) => {
                peer.set_last_time(SystemTime::now());
                if !peer.has_name() {
                    self.send(UdpMessage::ask_name(), Recepients::One(ip));
                }
            }
        };
    }
}

pub fn get_my_ipv4() -> Option<Ipv4Addr> {
    let socket = match UdpSocket::bind("0.0.0.0:0") {
        Ok(s) => s,
        Err(_) => return None,
    };

    match socket.connect("8.8.8.8:80") {
        Ok(()) => (),
        Err(_) => return None,
    };

    if let Ok(SocketAddr::V4(addr)) = socket.local_addr() {
        return Some(addr.ip().to_owned());
    }
    None
}

pub fn parse_netmask(s: &str) -> Option<u8> {
    if let Ok(mask) = s.parse::<u8>() {
        return Some(mask);
    }
    let ip = Ipv4Addr::from_str(s).ok()?;
    ipv4_mask_to_prefix(ip).ok()
}
