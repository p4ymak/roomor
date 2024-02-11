pub mod message;

use log::info;
use message::{Command, Message};
use std::collections::btree_map::Entry;
// use rusqlite::Connection;
use std::collections::BTreeMap;
use std::error::Error;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
// use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;

use self::message::Id;

pub enum Recepients {
    One(Ipv4Addr),
    Peers,
    All,
}

// pub struct User {
//     name: String,
//     ip: Ipv4Addr,
// }

pub trait Repaintable
where
    Self: Clone + Sync + Send + 'static,
{
    fn request_repaint(&self);
}
#[derive(Clone)]
pub struct RepaintDummy;
impl Repaintable for RepaintDummy {
    fn request_repaint(&self) {}
}

pub struct UdpChat {
    socket: Option<Arc<UdpSocket>>,
    pub ip: Ipv4Addr,
    pub port: usize,
    pub name: String,
    sync_sender: mpsc::SyncSender<(Ipv4Addr, Message)>,
    sync_receiver: mpsc::Receiver<(Ipv4Addr, Message)>,
    pub message: Message,
    pub history: Vec<(Ipv4Addr, (Id, String))>,
    pub peers: BTreeMap<Ipv4Addr, String>,
    all_recepients: Vec<String>,
    // db: Option<Connection>,
    // pub db_status: String,
}

impl UdpChat {
    pub fn new(name: String, port: usize) -> Self {
        let (tx, rx) = mpsc::sync_channel::<(Ipv4Addr, Message)>(0);
        // let (db, db_status) = match db_path {
        //     Some(path) => (Connection::open(path).ok(), "DB: ready.".to_string()),
        //     None => (None, "DB! offline".to_string()),
        // };
        // warn!("{}", db_status);
        UdpChat {
            socket: None,
            ip: Ipv4Addr::UNSPECIFIED,
            port,
            name,
            sync_sender: tx,
            sync_receiver: rx,
            message: Message::empty(),
            history: Vec::<(Ipv4Addr, (Id, String))>::new(),
            peers: BTreeMap::<Ipv4Addr, String>::new(),
            all_recepients: vec![],
            // db,
            // db_status,
        }
    }

    pub fn prelude(&mut self, ctx: &impl Repaintable) {
        // self.db_create();
        // if let Ok(history) = self.db_get_all() {
        //     self.history = history;
        // };
        self.connect().ok();
        self.listen(ctx);
        self.message = Message::enter(&self.name);
        self.send(Recepients::All);
    }

    fn connect(&mut self) -> Result<(), Box<dyn Error + 'static>> {
        let my_ip = local_ipaddress::get()
            .ok_or("no local")?
            .parse::<Ipv4Addr>()?;
        self.ip = my_ip;
        self.all_recepients = (0..=254)
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
            .collect();
        self.socket = match UdpSocket::bind(format!("{}:{}", self.ip, self.port)) {
            Ok(socket) => {
                socket.set_broadcast(true).unwrap();
                socket.set_multicast_loop_v4(false).unwrap();
                Some(Arc::new(socket))
            }
            _ => None,
        };

        Ok(())
    }

    fn listen(&self, ctx: &impl Repaintable) {
        if let Some(socket) = &self.socket {
            let reader = Arc::clone(socket);
            let receiver = self.sync_sender.clone();
            let ctx = ctx.clone();
            thread::spawn(move || {
                let mut buf = [0; 2048];
                loop {
                    if let Ok((number_of_bytes, SocketAddr::V4(src_addr_v4))) =
                        reader.recv_from(&mut buf)
                    {
                        let ip = *src_addr_v4.ip();
                        if let Some(message) =
                            Message::from_be_bytes(&buf[..number_of_bytes.min(128)])
                        {
                            info!("{}: {}", ip, message);
                            ctx.request_repaint();
                            receiver.send((ip, message)).ok();
                        }
                    }
                }
            });
        }
    }

    pub fn send(&mut self, mut addrs: Recepients) {
        if self.message.command == Command::Empty {
            return;
        }

        let bytes = self.message.to_be_bytes();
        if let Some(socket) = &self.socket {
            if self.peers.len() == 1 {
                addrs = Recepients::All;
            }
            match addrs {
                Recepients::All => self
                    .all_recepients
                    .iter()
                    .map(|r| socket.send_to(&bytes, r).is_ok())
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
        // self.message = Message::empty();
    }

    pub fn receive(&mut self) {
        if let Ok(message) = self.sync_receiver.try_recv() {
            let r_ip = message.0;
            let r_msg = message.1;
            match r_msg.command {
                Command::Enter => {
                    let name = String::from_utf8_lossy(&r_msg.data);
                    info!("{} entered chat.", r_ip);
                    if let Entry::Vacant(ip) = self.peers.entry(r_ip) {
                        ip.insert(name.to_string());
                        if r_ip != self.ip {
                            self.message = Message::enter(&self.name);
                            self.send(Recepients::One(r_ip));
                        }
                    }
                }
                Command::Text | Command::Repeat => {
                    // if message.0 != self.ip {
                    //     self.db_save(message.0, &message.1);
                    // }
                    let text = r_msg.read_text();
                    self.history.push((r_ip, (r_msg.id, text)));
                    if let Entry::Vacant(ip) = self.peers.entry(r_ip) {
                        // if !self.peers.contains_key(&message.0) {
                        ip.insert(r_ip.to_string());
                        if r_ip != self.ip {
                            self.message = Message::enter(&self.name);
                            self.send(Recepients::One(r_ip));
                        }
                    }
                }
                Command::Damaged => {
                    self.message =
                        Message::new(Command::AskToRepeat, r_msg.id.to_be_bytes().to_vec());
                    self.send(Recepients::One(r_ip));
                }
                Command::AskToRepeat => {
                    let id: u32 = u32::from_be_bytes(
                        (0..4)
                            .map(|i| *r_msg.data.get(i).unwrap_or(&0))
                            .collect::<Vec<u8>>()
                            .try_into()
                            .unwrap(),
                    );
                    self.message = Message::retry_text(
                        id,
                        self.history
                            .iter()
                            .find(|m| m.1 .0 == id)
                            .unwrap_or(&(r_ip, (id, String::from("NO SUCH MESSAGE! = ("))))
                            .1
                             .1
                            .as_str(),
                    );
                    self.send(Recepients::One(r_ip));
                }
                Command::Exit => {
                    info!("{} left chat.", r_ip);
                    self.peers.remove(&r_ip);
                }
                _ => (),
            }
        }
    }

    // fn db_create(&mut self) {
    //     if let Some(db) = &self.db {
    //         self.db_status = match db.execute(
    //             "create table if not exists chat_history (
    //             id integer primary key,
    //             ip text not null,
    //             message_text text not null
    //             )",
    //             [],
    //         ) {
    //             Ok(_) => "DB is ready.".to_string(),
    //             Err(err) => format!("DB Err: {}", err),
    //         };
    //         warn!("{}", self.db_status);
    //     }
    // }
    // fn db_save(&mut self, ip: Ipv4Addr, message: &Message) {
    //     if let Some(db) = &self.db {
    //         self.db_status = match db.execute(
    //             "INSERT INTO chat_history (id, ip, message_text) values (?1, ?2, ?3)",
    //             [message.id.to_string(), ip.to_string(), message.read_text()],
    //         ) {
    //             Ok(_) => "DB: appended.".to_string(),
    //             Err(err) => format!("DB! {}", err),
    //         };
    //         info!("{}", self.db_status);
    //     }
    // }
    // fn db_get_all(&mut self) -> rusqlite::Result<Vec<(Ipv4Addr, String)>> {
    //     if let Some(db) = &self.db {
    //         let mut stmt = db.prepare("SELECT ip, message_text FROM chat_history")?;
    //         let mut rows = stmt.query([])?;
    //         let mut story = Vec::<(String, String)>::new();
    //         while let Some(row) = rows.next()? {
    //             story.push((row.get(0)?, row.get(1)?));
    //         }

    //         Ok(story
    //             .iter()
    //             .map(|row| (row.0.parse::<Ipv4Addr>().unwrap(), row.1.to_owned()))
    //             .collect())
    //     } else {
    //         Ok(Vec::<(Ipv4Addr, String)>::new())
    //     }
    // }
    // fn db_get_by_id(&mut self, id: u32) -> Option<String> {
    //     if let Some(db) = &self.db {
    //         match db.query_row(
    //             "SELECT message_text FROM chat_history WHERE id = ?",
    //             [id],
    //             |row| row.get(0),
    //         ) {
    //             Ok(message_text) => message_text,
    //             Err(_) => None,
    //         }
    //     } else {
    //         None
    //     }
    // }
    pub fn clear_history(&mut self) {
        self.history.clear();
    }
    //     if let Some(db) = &self.db {
    //         if let Some(db_path) = db.path() {
    //             self.db_status = match std::fs::remove_file(db_path) {
    //                 Ok(_) => "DB: Cleared".to_string(),
    //                 Err(err) => format!("DB! {}", err),
    //             };
    //             info!("{}", self.db_status);
    //             self.db_create();
    //         }
    //     }
    //     self.history = Vec::<(Ipv4Addr, String)>::new();
    // }
}
