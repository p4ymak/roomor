use rusqlite::{Connection, Result, NO_PARAMS};
use std::collections::HashSet;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;

fn string_from_be_u8(bytes: &[u8]) -> String {
    // std::str::from_utf8(&bytes.iter().map(|b| u8::from_be(*b)).collect::<Vec<u8>>())
    std::str::from_utf8(bytes).unwrap_or("UNKNOWN").to_string()
}

fn be_u8_from_str(text: &str) -> Vec<u8> {
    text.trim().as_bytes().to_owned()
    // .iter()
    // .map(|c| u8::to_be(*c))
    // .collect::<Vec<u8>>()
}

pub enum Recepients {
    One(Ipv4Addr),
    Peers,
    All,
}

#[derive(Debug)]
pub enum Command {
    Text(String),
    Enter(String),
    Empty,
    Exit,
}
impl Command {
    pub fn to_be_bytes(&self) -> Vec<u8> {
        match self {
            Command::Text(text) => {
                let mut v = vec![0];
                v.extend(be_u8_from_str(text));
                v
            }
            Command::Enter(name) => {
                let mut v = vec![1];
                v.extend(be_u8_from_str(name));
                v
            }
            Command::Exit => vec![4],
            Command::Empty => vec![],
        }
    }
    pub fn from_be_bytes(bytes: &[u8]) -> Self {
        match bytes.get(0) {
            Some(0) => Command::Text(string_from_be_u8(&bytes[1..])),
            Some(1) => Command::Enter(string_from_be_u8(&bytes[1..])),
            Some(4) => Command::Exit,
            _ => Command::Empty,
        }
    }
}

pub struct UdpChat {
    socket: Option<Arc<UdpSocket>>,
    pub ip: Ipv4Addr,
    pub port: usize,
    pub name: String,
    sync_sender: mpsc::SyncSender<(Ipv4Addr, Command)>,
    sync_receiver: mpsc::Receiver<(Ipv4Addr, Command)>,
    pub message: Command,
    pub history: Vec<(Ipv4Addr, String)>,
    pub peers: HashSet<Ipv4Addr>,
    db: Option<Connection>,
}
impl UdpChat {
    pub fn new(name: String, port: usize, db_path: Option<PathBuf>) -> Self {
        let (tx, rx) = mpsc::sync_channel::<(Ipv4Addr, Command)>(0);
        let db = match db_path {
            Some(path) => Connection::open(path).ok(),
            None => None,
        };
        UdpChat {
            socket: None,
            ip: Ipv4Addr::UNSPECIFIED,
            port,
            name,
            sync_sender: tx,
            sync_receiver: rx,
            message: Command::Text("".to_string()),
            history: Vec::<(Ipv4Addr, String)>::new(),
            peers: HashSet::<Ipv4Addr>::new(),
            db,
        }
    }

    pub fn connect(&mut self) {
        if let Some(local_ip) = local_ipaddress::get() {
            if let Ok(my_ip) = local_ip.parse::<Ipv4Addr>() {
                self.ip = my_ip;
                self.socket = match UdpSocket::bind(format!("{}:{}", self.ip, self.port)) {
                    Ok(socket) => {
                        socket.set_broadcast(true).unwrap();
                        socket.set_multicast_loop_v4(false).unwrap();
                        Some(Arc::new(socket))
                    }
                    _ => None,
                };
            }
        }

        self.listen();
        self.message = Command::Enter(self.name.to_owned());
        self.send(Recepients::All);
    }

    fn listen(&self) {
        let reader = self.socket.clone().unwrap();
        let receiver = self.sync_sender.clone();
        thread::spawn(move || {
            let mut buf = [0; 2048];
            loop {
                if let Ok((number_of_bytes, SocketAddr::V4(src_addr_v4))) =
                    reader.recv_from(&mut buf)
                {
                    let ip = *src_addr_v4.ip();
                    let message = Command::from_be_bytes(&buf[..number_of_bytes.min(512)]);
                    receiver.send((ip, message)).ok();
                }
            }
        });
    }

    pub fn send(&mut self, mut addrs: Recepients) {
        if let Command::Empty = self.message {
            return;
        }
        let bytes = self.message.to_be_bytes();
        if let Some(socket) = &self.socket {
            if self.peers.len() == 1 {
                addrs = Recepients::All;
            }
            let recepients: Vec<String> = match addrs {
                Recepients::All => (0..=254)
                    .map(|i| {
                        format!(
                            "{}.{}.{}.{}:{}",
                            self.ip.octets()[0],
                            self.ip.octets()[1],
                            self.ip.octets()[2],
                            i,
                            self.port
                        )
                    })
                    .collect(),
                Recepients::Peers => self
                    .peers
                    .iter()
                    .map(|ip| format!("{}:{}", ip, self.port))
                    .collect(),
                Recepients::One(ip) => vec![format!("{}:{}", ip, self.port)],
            };
            // println!("Send to: {}", recepients.len());
            for recepient in recepients {
                socket.send_to(&bytes, recepient).ok();
            }
        }
        self.message = Command::Empty;
    }

    pub fn receive(&mut self) {
        if let Ok(message) = self.sync_receiver.try_recv() {
            match message.1 {
                Command::Enter(_name) => {
                    if !self.peers.contains(&message.0) {
                        self.peers.insert(message.0);
                        if message.0 != self.ip {
                            self.message = Command::Enter(self.name.to_owned());
                            self.send(Recepients::One(message.0));
                        }
                    }
                }
                Command::Text(text) => {
                    self.history.push((message.0, text));
                    if !self.peers.contains(&message.0) {
                        self.peers.insert(message.0);
                        if message.0 != self.ip {
                            self.message = Command::Enter(self.name.to_owned());
                            self.send(Recepients::One(message.0));
                        }
                    }
                }
                Command::Exit => {
                    self.peers.remove(&message.0);
                }
                _ => (),
            }
        }
    }
}
