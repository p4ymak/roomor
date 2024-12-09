use crate::chat::{
    inbox::InMessage,
    message::{self, send_shards, Command, ShardCount},
    TextMessage,
};

use super::{
    file::FileLink,
    message::{Id, UdpMessage},
    notifier::Repaintable,
    peers::{PeerId, PeersMap},
    BackEvent, Content, FrontEvent, Inbox, Outbox, Recepients,
};
use flume::Sender;
use log::{debug, error};
use std::{
    error::Error,
    net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket},
    ops::{ControlFlow, RangeInclusive},
    path::Path,
    sync::Arc,
    thread,
    time::{Duration, SystemTime},
};

pub type Port = u16;

pub const TIMEOUT_ALIVE: Duration = Duration::from_secs(60);
pub const TIMEOUT_CHECK: Duration = Duration::from_secs(10);
pub const TIMEOUT_SECOND: Duration = Duration::from_secs(1);
pub const PORT_DEFAULT: Port = 4444;
pub const IP_MULTICAST_DEFAULT: Ipv4Addr = Ipv4Addr::new(225, 225, 225, 225);
pub const IP_UNSPECIFIED: Ipv4Addr = Ipv4Addr::UNSPECIFIED;

pub struct NetWorker {
    id: PeerId,
    pub name: String,
    pub socket: Option<Arc<UdpSocket>>,
    pub multicast: SocketAddrV4,
    pub _ip: Ipv4Addr,
    pub peers: PeersMap,
    pub front_tx: Sender<BackEvent>,
}

impl NetWorker {
    pub fn new(_ip: Ipv4Addr, front_tx: Sender<BackEvent>) -> Self {
        NetWorker {
            id: PeerId::default(),
            name: String::new(),
            socket: None,
            multicast: SocketAddrV4::new(IP_MULTICAST_DEFAULT, PORT_DEFAULT),
            _ip,
            peers: PeersMap::new(),
            front_tx,
        }
    }
    pub fn set_id(&mut self, id: PeerId) {
        self.id = id;
    }
    pub fn id(&self) -> PeerId {
        self.id
    }
    pub fn connect(&mut self, multicast: Ipv4Addr) -> Result<(), Box<dyn Error + 'static>> {
        let socket = UdpSocket::bind(SocketAddrV4::new(IP_UNSPECIFIED, self.multicast.port()))?;
        socket.set_broadcast(true)?;
        socket.set_multicast_loop_v4(true)?;
        socket.join_multicast_v4(&multicast, &IP_UNSPECIFIED)?;
        socket.set_nonblocking(false)?;
        self.multicast.set_ip(multicast);
        self.socket = Some(Arc::new(socket));
        Ok(())
    }

    pub fn send(&self, message: UdpMessage, peer_id: PeerId) -> std::io::Result<usize> {
        let recepients = if message.public || peer_id == PeerId::PUBLIC {
            Recepients::All
        } else {
            let ip = self
                .peers
                .ids
                .get(&peer_id)
                .map(|p| p.ip())
                .expect("Peer doesn't exist!");
            Recepients::One(ip)
        };
        if let Some(socket) = &self.socket {
            debug!("Send to ID: {} IPs: {:?}", peer_id.0, recepients);
            send(socket, self.multicast, message, recepients)
        } else {
            Ok(0)
        }
    }

    pub fn send_shards(
        &self,
        link: Arc<FileLink>,
        range: RangeInclusive<ShardCount>,
        id: Id,
        recepients: Recepients,
        ctx: &impl Repaintable,
    ) {
        if let Some(socket) = &self.socket {
            let socket = socket.clone();
            let multicast_port = self.multicast;
            let ctx = ctx.clone();
            let peer_id = self.id();

            thread::Builder::new()
                .name("shard_sender".to_string())
                .spawn(move || {
                    match send_shards(
                        peer_id,
                        link,
                        range,
                        id,
                        recepients,
                        socket,
                        multicast_port,
                        ctx,
                    ) {
                        Ok(_) => (),
                        Err(err) => error!("{err}"),
                    }
                })
                .ok();
        }
    }

    pub fn handle_back_event(&mut self, event: BackEvent, ctx: &impl Repaintable) {
        match &event {
            BackEvent::PeerJoined(ref ip, ref peer_id, ref user_name) => {
                let new_comer = self.peers.peer_joined(*ip, *peer_id, user_name.as_ref());
                if new_comer {
                    self.send(UdpMessage::greating(self.id, &self.name), *peer_id)
                        .inspect_err(|e| error!("{e}"))
                        .ok();
                }
                ctx.request_repaint();
            }
            BackEvent::PeerLeft(peer_id) => {
                self.peers.peer_exited(*peer_id);
                ctx.request_repaint();
            }
            BackEvent::Message(msg) => {
                if matches!(msg.content, Content::Seen) {
                    ctx.request_repaint();
                } else {
                    let text = msg.get_text();
                    let name = self.peers.get_display_name(msg.peer_id());
                    let notification_text = format!("{name}: {text}");
                    ctx.notify(&notification_text);
                }
            }
        };
        self.front_tx.send(event).ok();
    }

    pub fn handle_front_event(
        &mut self,
        outbox: &mut Outbox,
        ctx: &impl Repaintable,
        event: FrontEvent,
    ) -> ControlFlow<()> {
        match event {
            FrontEvent::Message(msg) => {
                UdpMessage::send_message(&msg, self, outbox)
                    .inspect_err(|e| error!("{e}"))
                    .ok();

                self.front_tx.send(BackEvent::Message(msg)).ok();
                ctx.request_repaint();
            }
            FrontEvent::Ping(peer_id) => {
                debug!("Ping {peer_id:?}");
                self.send(UdpMessage::enter(self.id, &self.name), peer_id)
                    .inspect_err(|e| error!("{e}"))
                    .ok();
            }
            FrontEvent::Exit => {
                debug!("I'm Exit");
                self.send(UdpMessage::exit(self.id()), PeerId::PUBLIC)
                    .inspect_err(|e| error!("{e}"))
                    .ok();

                return ControlFlow::Break(());
            }
        }
        ControlFlow::Continue(())
    }

    pub fn handle_message(
        &mut self,
        inbox: &mut Inbox,
        outbox: &mut Outbox,
        ctx: &impl Repaintable,
        r_ip: Ipv4Addr,
        r_msg: UdpMessage,
        downloads_path: &Path,
    ) {
        if r_msg.from_peer_id == self.id {
            debug!("Loop");
            return;
        }
        let r_id = r_msg.id;
        self.incoming(r_msg.from_peer_id, r_ip);
        match r_msg.command {
            Command::Enter | Command::Greating => {
                let user_name = r_msg.read_text();

                self.handle_back_event(
                    BackEvent::PeerJoined(r_ip, r_msg.from_peer_id, Some(user_name)),
                    ctx,
                );
                if r_msg.command == Command::Enter {
                    self.send(
                        UdpMessage::greating(self.id, &self.name),
                        r_msg.from_peer_id,
                    )
                    .inspect_err(|e| error!("{e}"))
                    .ok();
                }

                for undelivered in outbox.undelivered(r_msg.from_peer_id) {
                    self.send(undelivered.clone(), r_msg.from_peer_id)
                        .inspect_err(|e| error!("{e}"))
                        .ok();
                }

                // } else {
                //     self.send(
                //         UdpMessage::greating(self.id, &self.name),
                //         Recepients::One(r_ip),
                //     )
                //     .inspect_err(|e| error!("{e}"))
                //     .ok();
                // }
            }

            Command::Exit => {
                self.handle_back_event(BackEvent::PeerLeft(r_msg.from_peer_id), ctx);
                inbox.peer_left(r_msg.from_peer_id);
            }

            Command::Text | Command::File | Command::Repeat => match r_msg.part {
                message::Part::Single => {
                    let txt_msg = TextMessage::from_udp(&r_msg);
                    self.send(UdpMessage::seen_msg(self.id, &txt_msg), r_msg.from_peer_id)
                        .inspect_err(|e| error!("{e}"))
                        .ok();
                    self.handle_back_event(BackEvent::Message(txt_msg), ctx);
                }
                message::Part::Init(_) => {
                    debug!("incoming PartInit");
                    if let Some(msg) = inbox.get_mut(&r_id) {
                        if msg.is_old_enough() {
                            msg.combine(self, ctx).ok();
                        }
                    } else if let Some(mut inmsg) = InMessage::new(r_ip, r_msg, downloads_path) {
                        let txt_msg = TextMessage::from_inmsg(&inmsg);
                        if inmsg.command == Command::File {
                            self.handle_back_event(BackEvent::Message(txt_msg), ctx);
                        }
                        inmsg.combine(self, ctx).ok();
                        inbox.insert(r_id, inmsg);
                    }
                }
                message::Part::Shard(count) => {
                    if let Some(inmsg) = inbox.get_mut(&r_id) {
                        inmsg.insert(count, r_msg, self, ctx);
                    } else {
                        self.send(UdpMessage::abort(self.id, r_id), r_msg.from_peer_id)
                            .inspect_err(|e| error!("{e}"))
                            .ok();
                    }
                }

                _ => (),
            },

            Command::Seen => {
                debug!("SEEN! {}", r_id);
                let txt_msg = TextMessage::from_udp(&r_msg);
                outbox.remove(r_msg.from_peer_id, txt_msg.id());
                outbox.files.remove(&r_id);
                self.handle_back_event(BackEvent::Message(txt_msg), ctx);
            }
            Command::Abort => {
                debug!("ABORTING! {}", r_id);
                if let Some(msg) = inbox.get_mut(&r_id) {
                    msg.link.abort();
                }
                // inbox.remove(&r_id);

                outbox.remove(r_msg.from_peer_id, r_id);
                if let Some(link) = outbox.files.get(&r_id) {
                    link.abort();
                }
                outbox.files.remove(&r_id);
            }
            Command::Error => {
                self.send(
                    UdpMessage::new_single(
                        self.id,
                        Command::AskToRepeat,
                        r_msg.id.to_be_bytes().to_vec(),
                        r_msg.public,
                    ),
                    r_msg.from_peer_id,
                )
                .inspect_err(|e| error!("{e}"))
                .ok();
            }

            Command::AskToRepeat => {
                debug!("Was asked to repeat {r_id}, part: {:?}", r_msg.part);
                // Resend my Name
                let mut not_found_text = false;
                let mut not_found_file = false;
                if r_id == 0 {
                    self.send(
                        UdpMessage::greating(self.id, &self.name),
                        r_msg.from_peer_id,
                    )
                    .inspect_err(|e| error!("{e}"))
                    .ok();
                } else if let message::Part::AskRange(range) = &r_msg.part {
                    let mut is_aborted = false;
                    if let Some(link) = outbox.files.get(&r_id) {
                        is_aborted = link.is_aborted();
                        if !is_aborted {
                            let completed =
                                link.completed.load(std::sync::atomic::Ordering::Relaxed);
                            link.completed.store(
                                completed.saturating_sub(range.clone().count() as ShardCount),
                                std::sync::atomic::Ordering::Relaxed,
                            );
                            debug!("sending shards {range:?}");
                            self.send_shards(
                                link.clone(),
                                range.to_owned(),
                                r_id,
                                Recepients::One(r_ip),
                                ctx,
                            );
                        }
                    } else {
                        not_found_file = true;
                    }
                    if is_aborted {
                        self.send(UdpMessage::abort(self.id, r_id), r_msg.from_peer_id)
                            .inspect_err(|e| error!("{e}"))
                            .ok();
                        debug!("Transmition aborted by user.");
                        outbox.files.remove(&r_id);
                    }
                } else if let Some(message) = outbox.get(r_msg.from_peer_id, r_id) {
                    debug!("Message found..");
                    let mut message = message.clone();
                    message.command = Command::Repeat;
                    self.send(message, r_msg.from_peer_id)
                        .inspect_err(|e| error!("{e}"))
                        .ok();
                } else {
                    not_found_text = true;
                }
                if not_found_file || not_found_text {
                    self.send(UdpMessage::abort(self.id, r_id), r_msg.from_peer_id)
                        .inspect_err(|e| error!("{e}"))
                        .ok();
                    error!("Message or File not found. Aborting transmission!");
                }
            }
        }
    }

    pub fn incoming(&mut self, peer_id: PeerId, ip: Ipv4Addr) {
        let mut ask_name = false;
        match self.peers.ids.get_mut(&peer_id) {
            None => {
                let noname: Option<&String> = None;
                self.peers.peer_joined(ip, peer_id, noname);
                ask_name = true;
            }
            Some(peer) => {
                peer.set_last_time(SystemTime::now());
                if !peer.has_name() {
                    ask_name = true;
                }
            }
        };
        if ask_name {
            self.send(UdpMessage::enter(self.id, &self.name), peer_id)
                .inspect_err(|e| error!("{e}"))
                .ok();
        }
        self.front_tx
            .send(BackEvent::PeerJoined(ip, peer_id, None))
            .ok();
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

pub fn send(
    socket: &Arc<UdpSocket>,
    multicast: SocketAddrV4,
    message: UdpMessage,
    addrs: Recepients,
) -> std::io::Result<usize> {
    let bytes = message.to_be_bytes();
    let result = match addrs {
        Recepients::All => socket.send_to(&bytes, multicast),
        Recepients::One(ip) => socket.send_to(&bytes, SocketAddrV4::new(ip, multicast.port())),
    };
    match &result {
        Ok(_num) => (), // debug!("Sent {num} bytes of '{:?}' to {addrs:?}", message.command),
        Err(err) => error!("Could't send '{:?}' to {addrs:?}: {err}", message.command),
    };
    result
}
