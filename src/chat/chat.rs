use super::message::{Command, Message};
use log::info;
use rusqlite::Connection;
use std::collections::HashSet;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;

pub enum Recepients {
    One(Ipv4Addr),
    Peers,
    All,
}

pub struct UdpChat {
    socket: Option<Arc<UdpSocket>>,
    pub ip: Ipv4Addr,
    pub port: usize,
    pub name: String,
    sync_sender: mpsc::SyncSender<(Ipv4Addr, Message)>,
    sync_receiver: mpsc::Receiver<(Ipv4Addr, Message)>,
    pub message: Message,
    pub history: Vec<(Ipv4Addr, String)>,
    pub peers: HashSet<Ipv4Addr>,
    db: Option<Connection>,
    pub db_status: String,
}
impl UdpChat {
    pub fn new(name: String, port: usize, db_path: Option<PathBuf>) -> Self {
        let (tx, rx) = mpsc::sync_channel::<(Ipv4Addr, Message)>(0);
        let (db, db_status) = match db_path {
            Some(path) => (Connection::open(path).ok(), "DB: ready.".to_string()),
            None => (None, "DB! offline".to_string()),
        };
        info!("{}", db_status);
        UdpChat {
            socket: None,
            ip: Ipv4Addr::UNSPECIFIED,
            port,
            name,
            sync_sender: tx,
            sync_receiver: rx,
            message: Message::empty(),
            history: Vec::<(Ipv4Addr, String)>::new(),
            peers: HashSet::<Ipv4Addr>::new(),
            db,
            db_status,
        }
    }

    pub fn prelude(&mut self) {
        self.db_create();
        if let Ok(history) = self.db_get_all() {
            self.history = history;
        };
        self.connect();
    }

    fn connect(&mut self) {
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
        self.message = Message::enter(&self.name);
        self.send(Recepients::All);
    }

    fn listen(&self) {
        if let Some(socket) = &self.socket {
            let reader = Arc::clone(socket);
            let receiver = self.sync_sender.clone();
            thread::spawn(move || {
                let mut buf = [0; 2048];
                loop {
                    if let Ok((number_of_bytes, SocketAddr::V4(src_addr_v4))) =
                        reader.recv_from(&mut buf)
                    {
                        let ip = *src_addr_v4.ip();
                        if let Some(message) =
                            Message::from_be_bytes(&buf[..number_of_bytes.min(512)])
                        {
                            println!("{}: {}", ip, message);
                            receiver.send((ip, message)).ok();
                        }
                    }
                }
            });
        }
    }

    pub fn send(&mut self, mut addrs: Recepients) {
        if let Command::Empty = self.message.command {
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
            for recepient in recepients {
                socket.send_to(&bytes, recepient).ok();
            }
        }
        self.message = Message::empty();
    }

    pub fn receive(&mut self) {
        if let Ok(message) = self.sync_receiver.try_recv() {
            match message.1.command {
                Command::Enter => {
                    info!("{} entered chat.", message.0);
                    if !self.peers.contains(&message.0) {
                        self.peers.insert(message.0);
                        if message.0 != self.ip {
                            self.message = Message::enter(&self.name);
                            self.send(Recepients::One(message.0));
                        }
                    }
                }
                Command::Text => {
                    let text = message.1.read_text();
                    self.db_save(message.0, &message.1);
                    self.history.push((message.0, text));
                    if !self.peers.contains(&message.0) {
                        self.peers.insert(message.0);
                        if message.0 != self.ip {
                            self.message = Message::enter(&self.name);
                            self.send(Recepients::One(message.0));
                        }
                    }
                }
                Command::Damaged => {
                    if message.0 != self.ip {
                        self.message = Message::new(Command::Retry, message.1.data);
                        self.send(Recepients::One(message.0));
                    }
                }
                Command::Retry => {
                    if message.0 != self.ip {
                        self.message = Message::retry_text(
                            message.1.id,
                            &self
                                .db_get_by_id(message.1.id)
                                .unwrap_or_else(|| String::from("NO SUCH MESSAGE! = (")),
                        );
                        self.send(Recepients::One(message.0));
                    }
                }
                Command::Exit => {
                    info!("{} left chat.", message.0);
                    self.peers.remove(&message.0);
                }
                _ => (),
            }
        }
    }

    fn db_create(&mut self) {
        if let Some(db) = &self.db {
            self.db_status = match db.execute(
                "create table if not exists chat_history (
                id integer primary key,
                ip text not null,
                message_text text not null
                )",
                [],
            ) {
                Ok(_) => "DB is ready.".to_string(),
                Err(err) => format!("DB Err: {}", err),
            };
        }
    }
    fn db_save(&mut self, ip: Ipv4Addr, message: &Message) {
        if let Some(db) = &self.db {
            self.db_status = match db.execute(
                "INSERT INTO chat_history (id, ip, message_text) values (?1, ?2, ?3)",
                [message.id.to_string(), ip.to_string(), message.read_text()],
            ) {
                Ok(_) => "DB: appended.".to_string(),
                Err(err) => format!("DB! {}", err),
            };
            info!("{}", self.db_status);
        }
    }
    fn db_get_all(&mut self) -> rusqlite::Result<Vec<(Ipv4Addr, String)>> {
        if let Some(db) = &self.db {
            let mut stmt = db.prepare("SELECT ip, message_text FROM chat_history")?;
            let mut rows = stmt.query([])?;
            let mut story = Vec::<(String, String)>::new();
            while let Some(row) = rows.next()? {
                story.push((row.get(0)?, row.get(1)?));
            }

            Ok(story
                .iter()
                .map(|row| (row.0.parse::<Ipv4Addr>().unwrap(), row.1.to_owned()))
                .collect())
        } else {
            Ok(Vec::<(Ipv4Addr, String)>::new())
        }
    }
    fn db_get_by_id(&mut self, id: u32) -> Option<String> {
        if let Some(db) = &self.db {
            match db.query_row(
                "SELECT message_text FROM chat_history WHERE id = ?",
                [id],
                |row| row.get(0),
            ) {
                Ok(message_text) => message_text,
                Err(_) => None,
            }
        } else {
            None
        }
    }
    pub fn clear_history(&mut self) {
        if let Some(db) = &self.db {
            if let Some(db_path) = db.path() {
                self.db_status = match std::fs::remove_file(db_path) {
                    Ok(_) => "DB: Cleared".to_string(),
                    Err(err) => format!("DB! {}", err),
                };
                info!("{}", self.db_status);
                self.db_create();
            }
        }
        self.history = Vec::<(Ipv4Addr, String)>::new();
    }
}
