pub mod file;
pub mod message;
pub mod networker;
pub mod notifier;
pub mod peers;

use self::{
    file::{FileEnding, FileLink},
    message::{new_id, CheckSum, Part, ShardCount, CRC, MAX_PREVIEW_CHARS},
    networker::{NetWorker, TIMEOUT_CHECK},
    notifier::Repaintable,
};
use crate::app::UserSetup;
use directories::UserDirs;
use eframe::Result;
use flume::{Receiver, Sender};
use log::{debug, error};
use message::{Command, Id, UdpMessage};
use range_rover::range_rover;
use std::{
    collections::BTreeMap,
    error::Error,
    fs,
    net::{Ipv4Addr, SocketAddr},
    ops::ControlFlow,
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
    pub fn is_public(&self) -> bool {
        matches!(self, Recepients::All)
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
pub struct Inbox(BTreeMap<Id, InMessage>);

pub struct InMessage {
    ts: SystemTime,
    id: Id,
    sender: Ipv4Addr,
    public: bool,
    command: Command,
    _total_checksum: CheckSum,
    file_name: String,
    count: ShardCount,
    terminal: ShardCount,
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
                terminal: init.count().saturating_sub(1),
                shards: vec![None; init.count() as usize],
            })
        } else {
            None
        }
    }
    pub fn insert(
        &mut self,
        position: ShardCount,
        msg: UdpMessage,
        networker: &mut NetWorker,
        ctx: &impl Repaintable,
        downloads_path: &Path,
    ) -> bool {
        self.ts = SystemTime::now();
        if let Some(block) = self.shards.get_mut(position as usize) {
            if block.is_none() && msg.checksum() == CRC.checksum(&msg.data) {
                *block = Some(msg.data);
            }
        }
        if position == self.terminal {
            return self.combine(networker, downloads_path, ctx).is_ok();
        }
        false
    }
    pub fn combine(
        &mut self,
        sender: &mut NetWorker,
        downloads_path: &Path,
        ctx: &impl Repaintable,
    ) -> Result<(), Box<dyn Error + 'static>> {
        debug!("Combining");
        let missed = range_rover(
            self.shards
                .iter()
                .enumerate()
                .filter(|s| s.1.is_none())
                .map(|s| s.0 as ShardCount),
        );

        if missed.is_empty() {
            let data = std::mem::take(&mut self.shards)
                .into_iter()
                .flatten()
                .flatten()
                .collect::<Vec<u8>>();

            match self.command {
                Command::Text => {
                    let text = String::from_utf8(data)?;
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
                    sender.handle_back_event(BackEvent::Message(txt_msg), ctx);
                    Ok(())
                }
                Command::File => {
                    let path = downloads_path.join(&self.file_name);
                    debug!("Writing new file to {path:?}");
                    fs::write(&path, data)?;

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
                        sender.handle_back_event(BackEvent::Message(txt_msg), ctx);
                        Ok(())
                    } else {
                        Err("Missing Link".into())
                    }
                }
                _ => Ok(()),
            }
        } else {
            error!("Shards missing!");
            self.terminal = missed
                .last()
                .map(|l| *l.end())
                .unwrap_or(self.count.saturating_sub(1));
            for range in missed {
                debug!("Asked to repeat shard #{range:?}");
                sender
                    .send(
                        UdpMessage::ask_to_repeat(self.id, Part::RepeatRange(range)),
                        Recepients::One(self.sender),
                    )
                    .ok();
            }
            debug!("New terminal: {}", self.terminal);
            Err("Missing Shards".into())
            // self.shards.clear(); // FIXME
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
    networker: NetWorker,
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
                Command::File => Content::FileLink(FileLink::from_text(&msg.read_text()).unwrap()), // FIXME
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
            networker: sender,
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
        self.networker.port = user.port();
        self.networker.name = user.name().to_string();
        self.networker.connect(user.multicast())?;
        self.listen();
        Ok(())
    }

    pub fn run(&mut self, ctx: &impl Repaintable) {
        self.networker
            .send(UdpMessage::enter(&self.name), Recepients::All)
            .inspect_err(|e| error!("{e}"))
            .ok();
        self.receive(ctx);
    }

    fn listen(&mut self) {
        self.thread_handle = self.networker.socket.as_ref().map(|socket| {
            let local_ip = self.networker.ip;
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
            //     // FIXME file request timer
            //     self.inbox.0.retain(|_, msg| {
            //         !(SystemTime::now()
            //             .duration_since(msg.ts)
            //             .is_ok_and(|d| d > TIMEOUT_CHECK)
            //             && msg
            //                 .combine(&mut self.networker, &self.downloads_path, ctx)
            //                 .is_ok())
            //     });

            match event {
                ChatEvent::Front(front) => {
                    match self
                        .networker
                        .handle_front_event(&mut self.outbox, ctx, front)
                    {
                        ControlFlow::Continue(_) => continue,
                        ControlFlow::Break(_) => break,
                    }
                }
                ChatEvent::Incoming((r_ip, r_msg)) => {
                    self.networker.handle_message(
                        &mut self.inbox,
                        &mut self.outbox,
                        ctx,
                        r_ip,
                        r_msg,
                        &self.downloads_path,
                    );
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
