pub mod message;
pub mod networker;
pub mod notifier;

use self::{networker::NetWorker, notifier::Repaintable};

use flume::{Receiver, Sender};
use message::{Command, Id, Message};
use std::{
    collections::BTreeMap,
    net::{Ipv4Addr, SocketAddr},
    sync::Arc,
    thread,
};
pub enum Recepients {
    One(Ipv4Addr),
    Peers,
    All,
}

#[derive(Debug)]
pub enum FrontEvent {
    Enter(String),
    Text(String),
    Icon(String),
    Exit,
    Empty,
}
#[derive(Debug)]
pub enum BackEvent {
    PeerJoined((Ipv4Addr, Option<String>)),
    PeerLeft(Ipv4Addr),
    Message(TextMessage),
    MyIp(Ipv4Addr),
}

#[derive(Debug)]
pub enum ChatEvent {
    Front(FrontEvent),
    Incoming((Ipv4Addr, Message)),
}

pub struct UdpChat {
    pub name: String,
    tx: Sender<ChatEvent>,
    rx: Receiver<ChatEvent>,
    sender: NetWorker,
    history: BTreeMap<Id, Message>,
}

#[allow(dead_code)]
#[derive(Debug)]
pub struct TextMessage {
    ip: Ipv4Addr,
    id: Id,
    content: FrontEvent,
}
impl TextMessage {
    pub fn from_message(ip: Ipv4Addr, msg: &Message) -> Self {
        TextMessage {
            ip,
            id: msg.id,
            content: match msg.command {
                Command::Enter => FrontEvent::Enter(msg.read_text()),
                Command::Text => FrontEvent::Text(msg.read_text()),
                Command::Icon => FrontEvent::Icon(msg.read_text()),
                Command::Exit => FrontEvent::Exit,
                _ => FrontEvent::Empty,
            },
        }
    }
    pub fn enter(ip: Ipv4Addr, name: String) -> Self {
        TextMessage {
            ip,
            id: 0,
            content: FrontEvent::Enter(name),
        }
    }
    pub fn exit(ip: Ipv4Addr) -> Self {
        TextMessage {
            ip,
            id: 0,
            content: FrontEvent::Exit,
        }
    }

    pub fn ip(&self) -> Ipv4Addr {
        self.ip
    }
    pub fn _id(&self) -> Id {
        self.id
    }
    pub fn content(&self) -> &FrontEvent {
        &self.content
    }
    pub fn text(&self) -> &str {
        match &self.content {
            FrontEvent::Text(text) | FrontEvent::Icon(text) => text,
            _ => "",
        }
    }
}

impl UdpChat {
    pub fn new(name: String, port: u16, front_tx: Sender<BackEvent>) -> Self {
        let (tx, rx) = flume::unbounded::<ChatEvent>();
        UdpChat {
            sender: NetWorker::new(port, front_tx),
            name,
            tx,
            rx,
            history: BTreeMap::<Id, Message>::new(),
        }
    }
    pub fn tx(&self) -> Sender<ChatEvent> {
        self.tx.clone()
    }
    pub fn prelude(&mut self, name: &str, port: u16) {
        self.name = name.to_string();
        self.sender.port = port;
        self.sender.connect().ok(); // FIXME Handle Error
        self.listen();
    }

    pub fn run(&mut self, ctx: &impl Repaintable) {
        self.sender
            .send(Message::enter(&self.name), Recepients::All);
        self.receive(ctx);
    }

    fn listen(&self) {
        if let Some(socket) = &self.sender.socket {
            let local_ip = self.sender.ip;
            let socket = Arc::clone(socket);
            let receiver = self.tx.clone();
            thread::spawn(move || {
                let mut buf = [0; 2048];
                loop {
                    if let Ok((number_of_bytes, SocketAddr::V4(src_addr_v4))) =
                        socket.recv_from(&mut buf)
                    {
                        let ip = *src_addr_v4.ip();
                        if ip != local_ip {
                            if let Some(message) =
                                Message::from_be_bytes(&buf[..number_of_bytes.min(128)])
                            {
                                receiver.send(ChatEvent::Incoming((ip, message))).ok();
                            }
                        }
                    }
                }
            });
        }
    }

    pub fn receive(&mut self, ctx: &impl Repaintable) {
        for event in self.rx.iter() {
            match event {
                ChatEvent::Front(front) => match front {
                    FrontEvent::Enter(name) => {
                        self.sender.send(Message::enter(&name), Recepients::All);
                    }
                    FrontEvent::Text(text) => {
                        let message = Message::text(&text);
                        self.history.insert(message.id, message.clone());
                        self.sender
                            .front_tx
                            .send(BackEvent::Message(TextMessage::from_message(
                                self.sender.ip,
                                &message,
                            )))
                            .ok();
                        self.sender.send(message, Recepients::Peers);
                        ctx.request_repaint();
                    }
                    FrontEvent::Icon(text) => {
                        let message = Message::icon(&text);
                        self.history.insert(message.id, message.clone());
                        self.sender
                            .front_tx
                            .send(BackEvent::Message(TextMessage::from_message(
                                self.sender.ip,
                                &message,
                            )))
                            .ok();
                        self.sender.send(message, Recepients::Peers);
                        ctx.request_repaint();
                    }

                    FrontEvent::Exit => {
                        self.sender.send(Message::exit(), Recepients::Peers);
                    }
                    FrontEvent::Empty => (),
                },

                ChatEvent::Incoming((r_ip, r_msg)) => {
                    if r_ip == self.sender.ip {
                        continue;
                    }
                    let txt_msg = TextMessage::from_message(r_ip, &r_msg);
                    self.sender.incoming(r_ip, &self.name);
                    match r_msg.command {
                        Command::Enter => {
                            let user_name = String::from_utf8_lossy(&r_msg.data);
                            self.sender.event(
                                BackEvent::PeerJoined((r_ip, Some(user_name.to_string()))),
                                ctx,
                            );
                        }
                        Command::Greating => (),
                        Command::Exit => {
                            self.sender.event(BackEvent::PeerLeft(r_ip), ctx);
                        }
                        Command::Text | Command::Icon | Command::Repeat => {
                            self.sender.event(BackEvent::Message(txt_msg), ctx);
                        }
                        Command::Error => {
                            self.sender.send(
                                Message::new(Command::AskToRepeat, r_msg.id.to_be_bytes().to_vec()),
                                Recepients::One(r_ip),
                            );
                        }
                        Command::AskToRepeat => {
                            let id: u32 = u32::from_be_bytes(
                                (0..4)
                                    .map(|i| *r_msg.data.get(i).unwrap_or(&0))
                                    .collect::<Vec<u8>>()
                                    .try_into()
                                    .unwrap_or_default(),
                            );
                            // Resend my Name
                            if id == 0 {
                                self.sender
                                    .send(Message::greating(&self.name), Recepients::One(r_ip));
                            } else if let Some(message) = self.history.get(&id) {
                                let mut message = message.clone();
                                message.command = Command::Repeat;
                                self.sender.send(message, Recepients::One(r_ip));
                            }
                        }
                    }
                }
            }
        }
    }
}

pub fn utf8_truncate(input: &mut String, maxsize: usize) {
    let mut utf8_maxsize = input.len();
    if utf8_maxsize >= maxsize {
        {
            let mut char_iter = input.char_indices();
            while utf8_maxsize >= maxsize {
                utf8_maxsize = match char_iter.next_back() {
                    Some((index, _)) => index,
                    _ => 0,
                };
            }
        } // Extra {} wrap to limit the immutable borrow of char_indices()
        input.truncate(utf8_maxsize);
    }
}
