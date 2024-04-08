use super::{
    message::UdpMessage, notifier::Repaintable, peers::PeersMap, BackEvent, Content, Recepients,
};
use flume::Sender;
use log::{debug, error};
use std::{
    error::Error,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket},
    sync::Arc,
    time::{Duration, SystemTime},
};

pub const TIMEOUT_ALIVE: Duration = Duration::from_secs(60);
pub const TIMEOUT_CHECK: Duration = Duration::from_secs(10);
pub const IP_MULTICAST_DEFAULT: Ipv4Addr = Ipv4Addr::new(225, 225, 225, 225);
pub const IP_UNSPECIFIED: Ipv4Addr = Ipv4Addr::UNSPECIFIED;
// pub const TIMEOUT_SECOND: Duration = Duration::from_secs(1);

pub struct NetWorker {
    pub name: String,
    pub socket: Option<Arc<UdpSocket>>,
    pub ip: Ipv4Addr,
    pub port: u16,
    pub peers: PeersMap,
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
            front_tx,
        }
    }
    pub fn connect(&mut self, multicast: Ipv4Addr) -> Result<(), Box<dyn Error + 'static>> {
        let socket = UdpSocket::bind(SocketAddrV4::new(IP_UNSPECIFIED, self.port))?;
        socket.set_broadcast(true)?;
        socket.set_multicast_loop_v4(true)?;
        socket.join_multicast_v4(&multicast, &IP_UNSPECIFIED)?;
        socket.set_nonblocking(false)?;
        self.socket = Some(Arc::new(socket));
        Ok(())
    }

    pub fn send(&mut self, message: UdpMessage, addrs: Recepients) -> std::io::Result<usize> {
        let bytes = message.to_be_bytes();
        if let Some(socket) = &self.socket {
            let result = match addrs {
                Recepients::All => {
                    socket.send_to(&bytes, SocketAddrV4::new(IP_MULTICAST_DEFAULT, self.port))
                }
                Recepients::One(ip) => socket.send_to(&bytes, SocketAddrV4::new(ip, self.port)),
            };
            match &result {
                Ok(num) => debug!("Sent {num} bytes of '{:?}' to {addrs:?}", message.command),
                Err(err) => error!("Could't send '{:?}' to {addrs:?}: {err}", message.command),
            };
            result
        } else {
            Ok(0)
        }
    }

    pub fn handle_event(&mut self, event: BackEvent, ctx: &impl Repaintable) {
        match event {
            BackEvent::PeerJoined((ip, ref user_name)) => {
                let new_comer = self.peers.peer_joined(ip, user_name.clone());
                self.front_tx.send(event).ok();
                if new_comer {
                    self.send(UdpMessage::greating(&self.name), Recepients::One(ip))
                        .inspect_err(|e| error!("{e}"))
                        .ok();
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
                if matches!(msg.content, Content::Seen) {
                    self.front_tx.send(BackEvent::Message(msg)).ok();
                    ctx.request_repaint();
                } else {
                    let text = msg.get_text();
                    let name = self.peers.get_display_name(msg.ip());
                    let notification_text = format!("{name}: {text}");
                    self.front_tx.send(BackEvent::Message(msg)).ok();
                    ctx.notify(&notification_text);
                }
            }
        }
    }

    pub fn incoming(&mut self, ip: Ipv4Addr, my_name: &str) {
        self.front_tx.send(BackEvent::PeerJoined((ip, None))).ok();
        match self.peers.0.get_mut(&ip) {
            None => {
                let noname: Option<&str> = None;
                self.peers.peer_joined(ip, noname);
                self.send(UdpMessage::enter(my_name), Recepients::One(ip))
                    .inspect_err(|e| error!("{e}"))
                    .ok();
            }
            Some(peer) => {
                peer.set_last_time(SystemTime::now());
                if !peer.has_name() {
                    self.send(UdpMessage::enter(my_name), Recepients::One(ip))
                        .inspect_err(|e| error!("{e}"))
                        .ok();
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
