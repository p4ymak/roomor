use std::collections::HashSet;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;

fn ipv4_from_str(string: &str) -> Option<Ipv4Addr> {
    let ip_vec: Vec<u8> = string
        .split('.')
        .filter_map(|o| o.parse::<u8>().ok())
        .collect();

    if ip_vec.len() != 4 {
        return None;
    }
    let a = *ip_vec.get(0)?;
    let b = *ip_vec.get(1)?;
    let c = *ip_vec.get(2)?;
    let d = *ip_vec.get(3)?;
    Some(Ipv4Addr::new(a, b, c, d))
}

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
                let mut v = vec![0];
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
}
impl UdpChat {
    pub fn new(name: String, port: usize) -> Self {
        let (tx, rx) = mpsc::sync_channel::<(Ipv4Addr, Command)>(0);
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
        }
    }

    pub fn connect(&mut self) {
        if let Some(local_ip) = local_ipaddress::get() {
            if let Some(my_ip) = ipv4_from_str(&local_ip) {
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

        self.receive();
        self.message = Command::Enter(self.name.to_owned());
        self.send();
    }

    fn receive(&self) {
        let reader = self.socket.clone().unwrap();
        let receiver = self.sync_sender.clone();
        thread::spawn(move || {
            let mut buf = [0; 512];
            loop {
                if let Ok((number_of_bytes, src_addr)) = reader.recv_from(&mut buf) {
                    if let SocketAddr::V4(src_addr_v4) = src_addr {
                        receiver
                            .send((
                                *src_addr_v4.ip(),
                                Command::from_be_bytes(&buf[..number_of_bytes]),
                            ))
                            .unwrap();
                    }
                }
            }
        });
    }

    pub fn send(&mut self) {
        match self.message {
            Command::Empty => (),
            _ => {
                let bytes = self.message.to_be_bytes();
                if let Some(socket) = &self.socket {
                    let recepients: Vec<String> = match self.peers.len() {
                        0..=1 => (0..=254)
                            .map(|i| format!("192.168.0.{}:{}", i, self.port))
                            .collect(),
                        _ => self
                            .peers
                            .iter()
                            .map(|ip| format!("{}:{}", ip, self.port))
                            .collect(),
                    };
                    println!("Send to: {}", recepients.len());
                    for recepient in recepients {
                        socket.send_to(&bytes, recepient).ok();
                    }
                }
                self.message = Command::Empty;
            }
        }
    }

    pub fn read(&mut self) {
        if let Ok(message) = self.sync_receiver.try_recv() {
            match message.1 {
                Command::Enter(_name) => {
                    self.peers.insert(message.0);
                }
                Command::Text(text) => {
                    self.history.push((message.0, text));
                    self.peers.insert(message.0);
                }
                Command::Exit => {
                    self.peers.remove(&message.0);
                }
                _ => (),
            }
        }
    }
}
