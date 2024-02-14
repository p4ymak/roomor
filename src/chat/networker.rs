use super::{message::Message, BackEvent, Recepients};
use flume::Sender;
use std::{
    collections::BTreeMap,
    error::Error,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket},
    sync::Arc,
};
type HasName = bool;

pub struct NetWorker {
    pub socket: Option<Arc<UdpSocket>>,
    pub ip: Ipv4Addr,
    pub port: u16,
    pub peers: BTreeMap<Ipv4Addr, HasName>,
    all_recepients: Vec<Ipv4Addr>,
    pub front_tx: Sender<BackEvent>,
}
impl NetWorker {
    pub fn new(port: u16, front_tx: Sender<BackEvent>) -> Self {
        NetWorker {
            socket: None,
            ip: Ipv4Addr::UNSPECIFIED,
            port,
            peers: BTreeMap::<Ipv4Addr, HasName>::new(),
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
