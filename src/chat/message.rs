use super::{
    file::FileLink,
    networker::{send, NetWorker},
    notifier::Repaintable,
    peers::PeerId,
    Content, Outbox, Recepients, TextMessage,
};
use crc::{Crc, CRC_16_IBM_SDLC};
use enumn::N;
use log::{debug, error};
use std::{
    error::Error,
    fmt,
    mem::size_of,
    net::{SocketAddrV4, UdpSocket},
    ops::RangeInclusive,
    sync::Arc,
    time::SystemTime,
};
use system_interface::fs::FileIoExt;

pub const CRC: Crc<u16> = Crc::<u16>::new(&CRC_16_IBM_SDLC);
pub const MAX_EMOJI_SIZE: usize = 8;
pub const MAX_NAME_SIZE: usize = 40;
pub const MAX_PREVIEW_CHARS: usize = 13;
pub const DATA_LIMIT_BYTES: usize = 956;

pub type Id = u32;
pub type CheckSum = u16;
pub type ShardCount = u64;

#[derive(Debug, Eq, PartialEq, Copy, Clone, N)]
#[repr(u8)]
pub enum Command {
    Enter,
    Greating,
    Text,
    File,
    AskToRepeat,
    Repeat,
    Exit,
    Seen,
    Error,
    Abort,
}
impl Command {
    pub fn to_code(self) -> u8 {
        self as u8
    }
    pub fn from_code(code: u8) -> Self {
        Command::n(code).unwrap_or(Command::Error)
    }
}

#[derive(Debug, Clone)]
#[cfg_attr(test, derive(PartialEq))]
pub enum Part {
    Single,
    Init(PartInit),
    AskRange(RangeInclusive<ShardCount>),
    Shard(ShardCount),
}
impl Part {
    fn to_code(&self) -> u8 {
        match self {
            Part::Single => 0,
            Part::Init(_) => 1,
            Part::AskRange(_) => 2,
            Part::Shard(_) => 3,
        }
    }
}
#[derive(Debug, Clone)]
#[cfg_attr(test, derive(PartialEq))]
pub struct PartInit {
    total_checksum: CheckSum,
    count: ShardCount,
}
impl PartInit {
    pub fn checksum(&self) -> CheckSum {
        self.total_checksum
    }
    pub fn count(&self) -> ShardCount {
        self.count
    }
}

#[derive(Debug, Clone)]
#[cfg_attr(test, derive(PartialEq))]
pub struct UdpMessage {
    pub from_peer_id: PeerId,
    pub id: Id,
    pub public: bool,
    pub part: Part,
    checksum: CheckSum,
    pub command: Command,
    pub data: Vec<u8>,
}

impl fmt::Display for UdpMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "\nMessage #{}\nChecksum: {}\n{:?}\n'{}'\n",
            self.id,
            self.checksum,
            self.command,
            match self.command {
                Command::Text | Command::Error | Command::Repeat => string_from_be_u8(&self.data),
                Command::AskToRepeat => u32::from_be_bytes(
                    (0..4)
                        .map(|i| *self.data.get(i).unwrap_or(&0))
                        .collect::<Vec<u8>>()
                        .try_into()
                        .expect("array collected"),
                )
                .to_string(),
                _ => format!("{:?}", &self.data),
            }
        )
    }
}

impl UdpMessage {
    pub fn new_single(from_peer_id: PeerId, command: Command, data: Vec<u8>, public: bool) -> Self {
        let id = new_id();
        let total_checksum = CRC.checksum(&data);
        let checksum = total_checksum;
        UdpMessage {
            from_peer_id,
            id,
            checksum,
            part: Part::Single,
            public,
            command,
            data,
        }
    }

    pub fn enter(from_peer_id: PeerId, name: &str) -> Self {
        UdpMessage::new_single(from_peer_id, Command::Enter, be_u8_from_str(name), true)
    }
    pub fn greating(from_peer_id: PeerId, name: &str) -> Self {
        UdpMessage::new_single(from_peer_id, Command::Greating, be_u8_from_str(name), true)
    }
    pub fn exit(from_peer_id: PeerId) -> Self {
        UdpMessage::new_single(from_peer_id, Command::Exit, vec![], true)
    }
    pub fn seen_msg(from_peer_id: PeerId, msg: &TextMessage) -> Self {
        UdpMessage::seen_id(from_peer_id, msg.id, msg.public)
    }
    pub fn seen_id(from_peer_id: PeerId, id: Id, public: bool) -> Self {
        UdpMessage {
            from_peer_id,
            id,
            checksum: 0,
            part: Part::Single,
            command: Command::Seen,
            public,
            data: vec![],
        }
    }
    pub fn abort(from_peer_id: PeerId, id: Id) -> Self {
        UdpMessage {
            from_peer_id,
            id,
            public: false,
            part: Part::Single,
            checksum: 0,
            command: Command::Abort,
            data: vec![],
        }
    }
    pub fn ask_to_repeat(from_peer_id: PeerId, id: Id, part: Part) -> Self {
        UdpMessage {
            from_peer_id,
            id,
            public: false,
            part,
            checksum: 0,
            command: Command::AskToRepeat,
            data: vec![],
        }
    }

    pub fn send_message(
        msg: &TextMessage,
        sender: &mut NetWorker,
        outbox: &mut Outbox,
    ) -> Result<(), Box<dyn Error + 'static>> {
        let (command, data) = match &msg.content {
            Content::Ping(name) => (Command::Enter, be_u8_from_str(name)),
            Content::Text(text) => (Command::Text, be_u8_from_str(text)),
            Content::Big(big) => (Command::Text, be_u8_from_str(&format!(" {big}"))),
            Content::Icon(icon) => (Command::Text, be_u8_from_str(&format!("/{icon}"))),
            Content::Exit => (Command::Exit, vec![]),
            Content::Empty => (Command::Error, vec![]),
            Content::FileLink(link) => (Command::File, be_u8_from_str(&link.name)),
            Content::Seen => (Command::Seen, vec![]),
        };

        let peer_id = msg.peer_id();
        if let Content::FileLink(link) = &msg.content {
            let count = link.size.div_ceil(DATA_LIMIT_BYTES as u64);
            debug!("Count {count}");
            let total_checksum = 0;
            let message = UdpMessage {
                from_peer_id: sender.id(),
                id: msg.id,
                part: Part::Init(PartInit {
                    total_checksum,
                    count,
                }),
                public: msg.public,
                checksum: CRC.checksum(&data),
                command,
                data,
            };

            outbox.add(peer_id, message.clone());
            sender.send(message, peer_id)?;
            outbox.files.insert(msg.id, link.clone());

            Ok(())
        } else {
            let checksum = CRC.checksum(&data);
            let total_checksum = CRC.checksum(&data);
            let count = data.chunks(DATA_LIMIT_BYTES).count() as u64;
            let chunks = data.chunks(DATA_LIMIT_BYTES);
            if data.len() < DATA_LIMIT_BYTES {
                let message = UdpMessage {
                    from_peer_id: sender.id(),
                    id: msg.id,
                    part: Part::Single,
                    public: msg.public,
                    checksum,
                    command,
                    data,
                };
                if message.command == Command::Text && !msg.is_public() {
                    outbox.add(peer_id, message.clone());
                }
                sender.send(message, peer_id)?;
                Ok(())
            } else {
                let message = UdpMessage {
                    from_peer_id: sender.id(),
                    id: msg.id,
                    part: Part::Init(PartInit {
                        total_checksum,
                        count,
                    }),
                    public: msg.public,
                    checksum,
                    command,
                    data: vec![],
                };
                if message.command == Command::Text && !msg.is_public() {
                    outbox.add(peer_id, message.clone());
                }
                sender.send(message, peer_id)?;

                for (i, chunk) in chunks.enumerate() {
                    sender.send(
                        UdpMessage {
                            from_peer_id: sender.id(),
                            id: msg.id,
                            part: Part::Shard(i as ShardCount),
                            checksum: CRC.checksum(chunk),
                            public: msg.public,
                            command,
                            data: chunk.to_vec(),
                        },
                        peer_id,
                    )?;
                }
                Ok(())
            }
        }
    }

    pub fn from_be_bytes(bytes: &[u8]) -> Result<Self, Box<dyn Error + 'static>> {
        let mut header = u8::from_be(*bytes.first().ok_or("Empty header")?);
        let public = (header & 1) != 0;
        header >>= 1;
        let part_n = header & 3;
        header >>= 2;
        let mut shift = 1;
        let command = Command::from_code(header);
        let from_peer_id =
            PeerId(u32::read_bytes(bytes, &mut shift).inspect_err(|e| error!("PeerId {e}"))?);
        let id = u32::read_bytes(bytes, &mut shift).inspect_err(|e| error!("MessageId {e}"))?;
        let checksum =
            u16::read_bytes(bytes, &mut shift).inspect_err(|e| error!("Checksum {e}"))?;
        let (part, data) = match part_n {
            1 => (
                Part::Init(PartInit {
                    total_checksum: u16::read_bytes(bytes, &mut shift)
                        .inspect_err(|e| error!("TotalChecksum {e}"))?,
                    count: u64::read_bytes(bytes, &mut shift)
                        .inspect_err(|e| error!("Count {e}"))?,
                }),
                bytes.get(shift..).unwrap_or_default().to_owned(),
            ),
            2 => (
                Part::AskRange(
                    u64::read_bytes(bytes, &mut shift).inspect_err(|e| error!("RangeStart {e}"))?
                        ..=u64::read_bytes(bytes, &mut shift)
                            .inspect_err(|e| error!("RangeEnd {e}"))?,
                ),
                bytes.get(shift..).unwrap_or_default().to_owned(),
            ),
            3 => (
                Part::Shard(
                    u64::read_bytes(bytes, &mut shift).inspect_err(|e| error!("Shard {e}"))?,
                ),
                bytes.get(shift..).unwrap_or_default().to_owned(),
            ),
            _ => (
                Part::Single,
                bytes.get(shift..).unwrap_or_default().to_owned(),
            ),
        };
        // FIXME
        // if checksum == CRC.checksum(&data) || command == Command::Repeat {
        Ok(UdpMessage {
            from_peer_id,
            id,
            checksum,
            part,
            command,
            public,
            data,
        })
        // } else {
        //     Some(UdpMessage {
        //         id,
        //         checksum,
        //         part,
        //         command: Command::Error,
        //         public,
        //         data,
        //     })
        // }
    }

    pub fn to_be_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::<u8>::new();
        let header = self.public as u8 | (self.part.to_code() << 1) | (self.command.to_code() << 3);

        bytes.push(header.to_be());
        bytes.extend(self.from_peer_id.0.to_be_bytes());
        bytes.extend(self.id.to_be_bytes());
        bytes.extend(self.checksum.to_be_bytes());
        match &self.part {
            Part::Single => (),
            Part::Init(init) => {
                bytes.extend(init.total_checksum.to_be_bytes());
                bytes.extend(init.count.to_be_bytes());
            }
            Part::AskRange(range) => {
                bytes.extend(range.start().to_be_bytes());
                bytes.extend(range.end().to_be_bytes());
            }
            Part::Shard(remains) => bytes.extend(remains.to_be_bytes()),
        }
        bytes.extend(self.data.to_owned());

        bytes
    }

    pub fn read_text(&self) -> String {
        string_from_be_u8(&self.data)
    }

    pub fn checksum(&self) -> CheckSum {
        self.checksum
    }
}

pub fn string_from_be_u8(bytes: &[u8]) -> String {
    std::str::from_utf8(bytes).unwrap_or("UNKNOWN").to_string()
}

fn be_u8_from_str(text: &str) -> Vec<u8> {
    text.as_bytes().to_owned()
}

pub fn new_id() -> Id {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("System Time")
        .as_secs() as u32
}

#[allow(clippy::too_many_arguments)] // FIXME
pub fn send_shards(
    peer_id: PeerId,
    link: Arc<FileLink>,
    range: RangeInclusive<ShardCount>,
    id: Id,
    recepients: Recepients,
    socket: Arc<UdpSocket>,
    multicast: SocketAddrV4,
    ctx: impl Repaintable,
) -> Result<(), Box<dyn Error + 'static>> {
    let file = std::fs::File::open(&link.path)?;

    for i in range {
        if link.is_aborted() || link.is_ready() {
            break;
        }
        let mut data = vec![0; DATA_LIMIT_BYTES];
        file.read_at(&mut data, DATA_LIMIT_BYTES as u64 * i)?;
        send(
            &socket,
            multicast,
            UdpMessage {
                from_peer_id: peer_id,
                id,
                part: Part::Shard(i),
                checksum: CRC.checksum(&data),
                public: recepients.is_public(),
                command: Command::File,
                data,
            },
            recepients,
        )?;
        link.completed
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        ctx.request_repaint();
    }

    Ok(())
}

trait FromBytes: Sized {
    fn from_be_bytes(bytes: &[u8]) -> Result<Self, Box<dyn Error + 'static>>;
    fn read_bytes(bytes: &[u8], shift: &mut usize) -> Result<Self, Box<dyn Error + 'static>> {
        let size = size_of::<Self>();
        let slice = bytes.get(*shift..*shift + size);
        let value = Self::from_be_bytes(slice.ok_or("Out of Range!")?)?;
        *shift += size;
        Ok(value)
    }
}

impl FromBytes for u16 {
    fn from_be_bytes(bytes: &[u8]) -> Result<Self, Box<dyn Error + 'static>> {
        Ok(u16::from_be_bytes(
            bytes
                .try_into()
                .inspect_err(|e| error!("[u8;{}] {e}", bytes.len()))?,
        ))
    }
}
impl FromBytes for u32 {
    fn from_be_bytes(bytes: &[u8]) -> Result<Self, Box<dyn Error + 'static>> {
        Ok(u32::from_be_bytes(
            bytes
                .try_into()
                .inspect_err(|e| error!("[u8;{}] {e}", bytes.len()))?,
        ))
    }
}
impl FromBytes for u64 {
    fn from_be_bytes(bytes: &[u8]) -> Result<Self, Box<dyn Error + 'static>> {
        Ok(u64::from_be_bytes(
            bytes
                .try_into()
                .inspect_err(|e| error!("[u8;{}] {e}", bytes.len()))?,
        ))
    }
}
