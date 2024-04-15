pub mod file;
pub mod message;
pub mod networker;
pub mod notifier;
pub mod peers;

use self::{
    file::{FileEnding, FileLink},
    message::{new_id, CheckSum, Part, RemainsCount, MAX_PREVIEW_CHARS},
    networker::{NetWorker, TIMEOUT_CHECK},
    notifier::Repaintable,
};
use crate::app::UserSetup;
use directories::UserDirs;
use flume::{Receiver, Sender};
use log::{debug, error};
use message::{Command, Id, UdpMessage};
use std::{
    collections::BTreeMap,
    error::Error,
    fs,
    net::{Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    sync::Arc,
    thread::{self, JoinHandle},
    time::SystemTime,
};

#[derive(Debug, Default, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
pub enum Recepients {
    One(Ipv4Addr),
    #[default]
    All,
}
impl Recepients {
    pub fn from_ip(ip: Ipv4Addr, public: bool) -> Self {
        if !public {
            Recepients::One(ip)
        } else {
            Recepients::All
        }
    }
}

// FIXME
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum Content {
    Ping(String),
    Text(String),
    Icon(String),
    FileLink(FileLink),
    FileData(PathBuf),
    FileEnding(FileEnding),
    Exit,
    Seen,
    Empty,
}
#[derive(Debug)]
pub enum BackEvent {
    PeerJoined((Ipv4Addr, Option<String>)),
    PeerLeft(Ipv4Addr),
    Message(TextMessage),
}

#[derive(Debug)]
pub enum FrontEvent {
    Ping(Recepients),
    Exit,
    Message(TextMessage),
}

#[derive(Debug)]
pub enum ChatEvent {
    Front(FrontEvent),
    Incoming((Ipv4Addr, UdpMessage)),
}

#[derive(Default)]
pub struct Outbox {
    pub texts: BTreeMap<Ipv4Addr, Vec<OutMessage>>,
    pub files: BTreeMap<Id, FileLink>,
}
#[derive(Default)]
struct Inbox(BTreeMap<Id, InMessage>);

pub struct InMessage {
    ts: SystemTime,
    id: Id,
    sender: Ipv4Addr,
    public: bool,
    command: Command,
    _total_checksum: CheckSum,
    file_name: String,
    count: RemainsCount,
    shards: Vec<Option<Shard>>,
}
impl InMessage {
    pub fn new(ip: Ipv4Addr, msg: UdpMessage) -> Option<Self> {
        debug!("New Multipart {:?}", msg.command);
        if let Part::Init(init) = msg.part {
            let file_name = if let Command::File = msg.command {
                String::from_utf8(msg.data).unwrap_or(format!("{:?}", SystemTime::now()))
            //FIXME
            } else {
                String::new()
            };

            Some(InMessage {
                ts: SystemTime::now(),
                id: msg.id,
                sender: ip,
                public: msg.public,
                command: msg.command,
                file_name,
                _total_checksum: init.checksum(),
                count: init.count(),
                shards: vec![None; init.count() as usize],
            })
        } else {
            None
        }
    }
    pub fn insert(
        &mut self,
        remains: RemainsCount,
        msg: UdpMessage,
        sender: &mut NetWorker,
        ctx: &impl Repaintable,
        downloads_path: &Path,
    ) {
        debug!("Got shard, {remains} remains.");
        let pos = self.count - remains - 1;
        if let Some(block) = self.shards.get_mut(pos as usize) {
            if block.is_none() {
                *block = Some(msg.data);
            }
        }
        if remains == 0 {
            self.combine(sender, downloads_path, ctx);
        }
    }
    pub fn combine(
        &mut self,
        sender: &mut NetWorker,
        downloads_path: &Path,
        ctx: &impl Repaintable,
    ) {
        debug!("Combining");
        let missed = self
            .shards
            .iter()
            .rev()
            .enumerate()
            .filter(|s| s.1.is_none())
            .map(|s| s.0)
            .collect::<Vec<usize>>();
        if missed.is_empty() {
            let data = std::mem::take(&mut self.shards)
                .into_iter()
                .flatten()
                .flatten()
                .collect::<Vec<u8>>();

            match self.command {
                Command::Text => {
                    if let Ok(text) = String::from_utf8(data) {
                        let txt_msg = TextMessage {
                            timestamp: self.ts,
                            incoming: true,
                            public: self.public,
                            ip: self.sender,
                            id: self.id,
                            content: Content::Text(text),
                            seen: Some(Seen::One),
                        };
                        sender
                            .send(UdpMessage::seen(&txt_msg), Recepients::One(self.sender))
                            .inspect_err(|e| error!("{e}"))
                            .ok();
                        sender.handle_event(BackEvent::Message(txt_msg), ctx);
                    }
                }
                Command::File => {
                    let path = downloads_path.join(&self.file_name);
                    debug!("Writing new file to {path:?}");
                    match fs::write(&path, data) {
                        Ok(_) => {
                            if let Some(link) = FileLink::new(&path) {
                                debug!("Creating link");
                                let txt_msg = TextMessage {
                                    timestamp: self.ts,
                                    incoming: true,
                                    public: self.public,
                                    ip: self.sender,
                                    id: self.id,
                                    content: Content::FileLink(link),
                                    seen: Some(Seen::One),
                                };
                                sender
                                    .send(UdpMessage::seen(&txt_msg), Recepients::One(self.sender))
                                    .inspect_err(|e| error!("{e}"))
                                    .ok();
                                sender.handle_event(BackEvent::Message(txt_msg), ctx);
                            }
                        }
                        Err(err) => error!("{err}"),
                    }
                }
                _ => (),
            }
        } else {
            error!("Shards missing!");
            for shard in missed {
                sender
                    .send(
                        UdpMessage::ask_to_repeat(self.id, Part::Shard(shard as u64)),
                        Recepients::One(self.sender),
                    )
                    .ok();
            }
            self.shards.clear(); // FIXME
        }
    }
}

pub type Shard = Vec<u8>;

pub struct OutMessage {
    ts: SystemTime,
    msg: UdpMessage,
}
impl OutMessage {
    pub fn new(msg: UdpMessage) -> Self {
        OutMessage {
            ts: SystemTime::UNIX_EPOCH,
            msg,
        }
    }
    pub fn id(&self) -> Id {
        self.msg.id
    }
}
impl Outbox {
    fn add(&mut self, ip: Ipv4Addr, msg: UdpMessage) {
        self.texts
            .entry(ip)
            .and_modify(|h| h.push(OutMessage::new(msg.clone())))
            .or_insert(vec![OutMessage::new(msg)]);
    }
    fn remove(&mut self, ip: Ipv4Addr, id: Id) {
        self.texts
            .entry(ip)
            .and_modify(|h| h.retain(|m| m.id() != id));
    }
    fn get(&self, ip: Ipv4Addr, id: Id) -> Option<&UdpMessage> {
        self.texts
            .get(&ip)
            .and_then(|h| h.iter().find(|m| m.id() == id))
            .map(|m| &m.msg)
    }
    fn undelivered(&mut self, ip: Ipv4Addr) -> Vec<&UdpMessage> {
        let now = SystemTime::now();
        if let Some(history) = self.texts.get_mut(&ip) {
            history
                .iter_mut()
                .filter_map(|msg| {
                    now.duration_since(msg.ts)
                        .is_ok_and(|t| t > TIMEOUT_CHECK)
                        .then_some({
                            msg.ts = now;
                            &msg.msg
                        })
                })
                .collect()
        } else {
            vec![]
        }
    }
}

pub struct UdpChat {
    pub name: String,
    tx: Sender<ChatEvent>,
    rx: Receiver<ChatEvent>,
    sender: NetWorker,
    outbox: Outbox,
    inbox: Inbox,
    downloads_path: PathBuf,
    thread_handle: Option<JoinHandle<()>>,
}
impl Drop for UdpChat {
    fn drop(&mut self) {
        if let Some(handle) = self.thread_handle.take() {
            handle.join().ok();
        }
    }
}
#[derive(Debug, Clone)]
pub enum Seen {
    One,
    Many(Vec<Ipv4Addr>),
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct TextMessage {
    timestamp: SystemTime,
    incoming: bool,
    public: bool,
    ip: Ipv4Addr,
    id: Id,
    content: Content,
    seen: Option<Seen>,
}
impl TextMessage {
    pub fn logo() -> Self {
        TextMessage {
            timestamp: SystemTime::now(),
            incoming: true,
            public: true,
            ip: Ipv4Addr::UNSPECIFIED,
            id: 0,
            content: Content::Icon(String::from("RMЯ")),
            seen: Some(Seen::One),
        }
    }

    pub fn from_message(ip: Ipv4Addr, msg: &UdpMessage, incoming: bool) -> Self {
        TextMessage {
            timestamp: SystemTime::now(),
            incoming,
            public: msg.public,
            ip,
            id: msg.id,
            content: match msg.command {
                Command::Enter => Content::Ping(msg.read_text()),
                Command::Text => {
                    let mut text = msg.read_text();
                    let is_icon = text.starts_with(' ');
                    text = text.trim().to_string();
                    if is_icon {
                        Content::Icon(text)
                    } else {
                        Content::Text(text)
                    }
                }
                Command::Exit => Content::Exit,
                Command::Seen => Content::Seen,
                _ => Content::Empty,
            },
            seen: incoming.then_some(Seen::One),
        }
    }
    pub fn is_public(&self) -> bool {
        self.public
    }
    pub fn is_incoming(&self) -> bool {
        self.incoming
    }

    pub fn in_enter(ip: Ipv4Addr, name: String) -> Self {
        TextMessage {
            timestamp: SystemTime::now(),
            incoming: true,
            public: true,
            ip,
            id: 0,
            content: Content::Ping(name),
            seen: Some(Seen::One),
        }
    }

    pub fn out_message(content: Content, recipients: Recepients) -> Self {
        let (public, ip) = match recipients {
            Recepients::One(ip) => (false, ip),
            _ => (true, Ipv4Addr::BROADCAST), //FIXME ??
        };

        TextMessage {
            timestamp: SystemTime::now(),
            incoming: false,
            public,
            ip,
            id: new_id(),
            content,
            seen: None,
        }
    }

    pub fn in_exit(ip: Ipv4Addr) -> Self {
        TextMessage {
            timestamp: SystemTime::now(),
            incoming: true,
            public: true,
            ip,
            id: 0,
            content: Content::Exit,
            seen: Some(Seen::One),
        }
    }

    pub fn ip(&self) -> Ipv4Addr {
        self.ip
    }
    pub fn id(&self) -> Id {
        self.id
    }
    pub fn seen_private(&mut self) {
        self.seen = Some(Seen::One);
    }
    pub fn seen_public_by(&mut self, ip: Ipv4Addr) {
        if let Some(Seen::Many(peers)) = &mut self.seen {
            peers.push(ip);
        } else {
            self.seen = Some(Seen::Many(vec![ip]));
        }
    }
    pub fn is_seen(&self) -> bool {
        self.seen.is_some()
    }
    pub fn is_seen_by(&self) -> &[Ipv4Addr] {
        if let Some(Seen::Many(peers)) = &self.seen {
            peers
        } else {
            &[]
        }
    }
    pub fn content(&self) -> &Content {
        &self.content
    }
    pub fn get_text(&self) -> String {
        match &self.content {
            Content::Text(text) => {
                if text.chars().count() <= MAX_PREVIEW_CHARS {
                    text.to_owned()
                } else {
                    let mut preview = text
                        .as_str()
                        .chars()
                        .take(MAX_PREVIEW_CHARS)
                        .collect::<String>();
                    preview.push_str("..");
                    preview
                }
            }
            Content::Icon(icon) => icon.to_owned(),
            _ => String::new(),
        }
    }
    pub fn time(&self) -> SystemTime {
        self.timestamp
    }
}

impl UdpChat {
    pub fn new(ip: Ipv4Addr, front_tx: Sender<BackEvent>) -> Self {
        let (tx, rx) = flume::unbounded::<ChatEvent>();
        let sender = NetWorker::new(ip, front_tx);
        let downloads_path = UserDirs::new()
            .unwrap()
            .download_dir()
            .unwrap()
            .join("Roomor");
        fs::create_dir_all(&downloads_path).ok(); // FIXME

        UdpChat {
            sender,
            name: String::new(),
            tx,
            rx,
            outbox: Outbox::default(),
            inbox: Inbox::default(),
            thread_handle: None,
            downloads_path,
        }
    }
    pub fn tx(&self) -> Sender<ChatEvent> {
        self.tx.clone()
    }
    pub fn prelude(&mut self, user: &UserSetup) -> Result<(), Box<dyn Error + 'static>> {
        self.name = user.name().to_string();
        self.sender.port = user.port();
        self.sender.name = user.name().to_string();
        self.sender.connect(user.multicast())?;
        self.listen();
        Ok(())
    }

    pub fn run(&mut self, ctx: &impl Repaintable) {
        self.sender
            .send(UdpMessage::enter(&self.name), Recepients::All)
            .inspect_err(|e| error!("{e}"))
            .ok();
        self.receive(ctx);
    }

    fn listen(&mut self) {
        self.thread_handle = self.sender.socket.as_ref().map(|socket| {
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
                        if let Some(message) =
                            UdpMessage::from_be_bytes(&buf[..number_of_bytes.min(128)])
                        {
                            if ip != local_ip {
                                receiver.send(ChatEvent::Incoming((ip, message))).ok();
                            } else if message.command == Command::Exit {
                                break;
                            }
                        }
                    }
                }
            })
        });
    }

    pub fn receive(&mut self, ctx: &impl Repaintable) {
        for event in self.rx.iter() {
            match event {
                ChatEvent::Front(front) => match front {
                    FrontEvent::Message(msg) => {
                        debug!("Sending: {}", msg.get_text());

                        UdpMessage::send_message(&msg, &mut self.sender, &mut self.outbox)
                            .inspect_err(|e| error!("{e}"))
                            .ok();

                        self.sender.front_tx.send(BackEvent::Message(msg)).ok();
                        ctx.request_repaint();
                    }
                    FrontEvent::Ping(recepients) => {
                        debug!("Ping {recepients:?}");
                        self.sender
                            .send(UdpMessage::enter(&self.name), recepients)
                            .inspect_err(|e| error!("{e}"))
                            .ok();
                    }
                    FrontEvent::Exit => {
                        debug!("I'm Exit");
                        self.sender
                            .send(UdpMessage::exit(), Recepients::All)
                            .inspect_err(|e| error!("{e}"))
                            .ok();
                        // self.sender.send(UdpMessage::exit(), Recepients::Myself);
                        break;
                    }
                },

                ChatEvent::Incoming((r_ip, r_msg)) => {
                    if r_ip == self.sender.ip {
                        continue;
                    }

                    debug!("Received {:?} from {r_ip}", r_msg.command);

                    match r_msg.command {
                        Command::Enter | Command::Greating => {
                            let user_name = String::from_utf8_lossy(&r_msg.data);

                            self.sender.handle_event(
                                BackEvent::PeerJoined((r_ip, Some(user_name.to_string()))),
                                ctx,
                            );
                            if r_msg.command == Command::Enter {
                                self.sender
                                    .send(UdpMessage::greating(&self.name), Recepients::One(r_ip))
                                    .inspect_err(|e| error!("{e}"))
                                    .ok();
                            }
                            for undelivered in self.outbox.undelivered(r_ip) {
                                self.sender
                                    .send(undelivered.clone(), Recepients::One(r_ip))
                                    .inspect_err(|e| error!("{e}"))
                                    .ok();
                            }
                        }

                        Command::Exit => {
                            self.sender.handle_event(BackEvent::PeerLeft(r_ip), ctx);
                        }
                        Command::Text | Command::File | Command::Repeat => match r_msg.part {
                            message::Part::Single => {
                                let txt_msg = TextMessage::from_message(r_ip, &r_msg, true);
                                self.sender.incoming(r_ip, &self.name); // FIXME
                                self.sender
                                    .send(UdpMessage::seen(&txt_msg), Recepients::One(r_ip))
                                    .inspect_err(|e| error!("{e}"))
                                    .ok();
                                self.sender.handle_event(BackEvent::Message(txt_msg), ctx);
                            }
                            message::Part::Init(_) => {
                                debug!("incomint PartInit");
                                let r_id = r_msg.id;
                                if let Some(inmsg) = InMessage::new(r_ip, r_msg) {
                                    self.inbox.0.insert(r_id, inmsg);
                                }
                            }
                            message::Part::Shard(count) => {
                                if let Some(inmsg) = self.inbox.0.get_mut(&r_msg.id) {
                                    inmsg.insert(
                                        count,
                                        r_msg,
                                        &mut self.sender,
                                        ctx,
                                        &self.downloads_path,
                                    );
                                }
                            }
                        },
                        Command::Seen => {
                            let txt_msg = TextMessage::from_message(r_ip, &r_msg, true);
                            self.sender.incoming(r_ip, &self.name);
                            self.outbox.remove(r_ip, txt_msg.id());
                            self.sender.handle_event(BackEvent::Message(txt_msg), ctx);
                        }
                        Command::Error => {
                            self.sender.incoming(r_ip, &self.name);
                            self.sender
                                .send(
                                    UdpMessage::new_single(
                                        Command::AskToRepeat,
                                        r_msg.id.to_be_bytes().to_vec(),
                                        r_msg.public,
                                    ),
                                    Recepients::One(r_ip),
                                )
                                .inspect_err(|e| error!("{e}"))
                                .ok();
                        }

                        Command::AskToRepeat => {
                            self.sender.incoming(r_ip, &self.name);
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
                                    .send(UdpMessage::greating(&self.name), Recepients::One(r_ip))
                                    .inspect_err(|e| error!("{e}"))
                                    .ok();
                            } else if let Some(message) = self.outbox.get(r_ip, id) {
                                let mut message = message.clone();
                                message.command = Command::Repeat;
                                self.sender
                                    .send(message, Recepients::One(r_ip))
                                    .inspect_err(|e| error!("{e}"))
                                    .ok();
                            }
                        } // Command::File => todo!(),
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

pub fn limit_text(text: &mut String, limit: usize) {
    *text = text.trim_end_matches('\n').to_string();
    utf8_truncate(text, limit);
}
