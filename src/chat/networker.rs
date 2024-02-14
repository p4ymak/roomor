use super::{message::Message, notifier::Repaintable, BackEvent, Recepients};
use flume::Sender;
use std::{
    collections::BTreeMap,
    error::Error,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket},
    sync::Arc,
};

pub struct NetWorker {
    pub socket: Option<Arc<UdpSocket>>,
    pub ip: Ipv4Addr,
    pub port: u16,
    pub peers: BTreeMap<Ipv4Addr, Option<String>>,
    all_recepients: Vec<Ipv4Addr>,
    pub front_tx: Sender<BackEvent>,
}
impl NetWorker {
    pub fn new(port: u16, front_tx: Sender<BackEvent>) -> Self {
        NetWorker {
            socket: None,
            ip: Ipv4Addr::UNSPECIFIED,
            port,
            peers: BTreeMap::<Ipv4Addr, Option<String>>::new(),
            all_recepients: vec![],
            front_tx,
        }
    }
    pub fn connect(&mut self) -> Result<(), Box<dyn Error + 'static>> {
        self.ip = get_my_ipv4().ok_or("No local IpV4")?;
        let octets = self.ip.octets();
        self.front_tx.send(BackEvent::MyIp(self.ip)).ok();

        self.all_recepients = (0..=254)
            .map(|i| Ipv4Addr::new(octets[0], octets[1], octets[2], i))
            .collect();
        self.socket = match UdpSocket::bind(SocketAddrV4::new(self.ip, self.port)) {
            Ok(socket) => {
                socket.set_broadcast(true).ok();
                socket.set_multicast_loop_v4(false).ok();
                socket.set_nonblocking(false).ok();
                Some(Arc::new(socket))
            }
            _ => None,
        };

        Ok(())
    }

    pub fn send(&mut self, message: Message, mut addrs: Recepients) {
        let bytes = message.to_be_bytes();
        if let Some(socket) = &self.socket {
            if self.peers.is_empty() {
                addrs = Recepients::All;
            }
            match addrs {
                Recepients::All => self
                    .all_recepients
                    .iter()
                    .map(|r| {
                        socket
                            .send_to(&bytes, SocketAddrV4::new(*r, self.port))
                            .is_ok()
                    })
                    .all(|r| r),
                Recepients::Peers => self
                    .peers
                    .keys()
                    .map(|ip| {
                        socket
                            .send_to(&bytes, format!("{}:{}", ip, self.port))
                            .is_ok()
                    })
                    .all(|r| r),
                Recepients::One(ip) => socket
                    .send_to(&bytes, format!("{}:{}", ip, self.port))
                    .is_ok(),
            };
        }
    }

    pub fn event(&mut self, event: BackEvent, ctx: &impl Repaintable) {
        match event {
            BackEvent::PeerJoined((ref ip, ref user_name)) => {
                let new_comer = self.peers.get(ip).is_none();
                self.peers
                    .entry(*ip)
                    .and_modify(|name| *name = user_name.clone());
                let notification_text = format!("{} joined..", self.get_user_name(*ip));
                self.front_tx.send(event).ok();
                if new_comer {
                    ctx.notify(&notification_text);
                } else {
                    ctx.request_repaint();
                }
            }
            BackEvent::PeerLeft(ip) => {
                let notification_text = format!("{} left..", self.get_user_name(ip));
                self.peers.remove(&ip);
                self.front_tx.send(BackEvent::PeerLeft(ip)).ok();
                ctx.notify(&notification_text);
            }
            BackEvent::Message(msg) => {
                let text = msg.text();
                let name = self.get_user_name(msg.ip());
                let notification_text = format!("{name}: {text}");
                self.front_tx.send(BackEvent::Message(msg)).ok();
                ctx.notify(&notification_text);
            }
            _ => (),
        }
    }

    pub fn incoming(&mut self, ip: Ipv4Addr, my_name: &str) {
        match self.peers.get(&ip) {
            None => {
                self.front_tx.send(BackEvent::PeerJoined((ip, None))).ok();
                self.peers.insert(ip, None);
                self.send(Message::greating(my_name), Recepients::One(ip));
            }
            Some(None) => {
                self.send(Message::ask_name(), Recepients::One(ip));
            }
            _ => (),
        };
    }

    fn get_user_name(&self, ip: Ipv4Addr) -> String {
        self.peers
            .get(&ip)
            .and_then(|r| r.clone())
            .unwrap_or(ip.to_string())
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
