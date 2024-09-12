use super::{
    file::FileLink,
    networker::{send, NetWorker, Port},
    notifier::Repaintable,
    Content, Outbox, Recepients, TextMessage,
};
use crc::{Crc, CRC_16_IBM_SDLC};
use enumn::N;
use log::debug;
use std::{
    error::Error, fmt, fs, net::UdpSocket, ops::RangeInclusive, os::unix::fs::FileExt, sync::Arc,
    time::SystemTime,
};

pub const CRC: Crc<u16> = Crc::<u16>::new(&CRC_16_IBM_SDLC);
pub const MAX_EMOJI_SIZE: usize = 8;
pub const MAX_NAME_SIZE: usize = 44;
pub const MAX_PREVIEW_CHARS: usize = 13;
pub const DATA_LIMIT_BYTES: usize = 960;

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
pub enum Part {
    Single,
    Init(PartInit),
    AskRange(RangeInclusive<ShardCount>),
    Shard(ShardCount),
    Abort,
}
impl Part {
    fn to_code(&self) -> u8 {
        match self {
            Part::Single => 0,
            Part::Init(_) => 1,
            Part::AskRange(_) => 2,
            Part::Shard(_) => 3,
            Part::Abort => 4,
        }
    }
}
#[derive(Debug, Clone)]
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
pub struct UdpMessage {
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
    pub fn new_single(command: Command, data: Vec<u8>, public: bool) -> Self {
        let id = new_id();
        let total_checksum = CRC.checksum(&data);
        let checksum = total_checksum;
        UdpMessage {
            id,
            checksum,
            part: Part::Single,
            public,
            command,
            data,
        }
    }

    pub fn enter(name: &str) -> Self {
        UdpMessage::new_single(Command::Enter, be_u8_from_str(name), true)
    }
    pub fn greating(name: &str) -> Self {
        UdpMessage::new_single(Command::Greating, be_u8_from_str(name), true)
    }
    pub fn exit() -> Self {
        UdpMessage::new_single(Command::Exit, vec![], true)
    }
    pub fn seen_msg(msg: &TextMessage) -> Self {
        UdpMessage::seen_id(msg.id, msg.public)
    }
    pub fn seen_id(id: Id, public: bool) -> Self {
        UdpMessage {
            id,
            checksum: 0,
            part: Part::Single,
            command: Command::Seen,
            public,
            data: vec![],
        }
    }
    pub fn abort(id: Id) -> Self {
        UdpMessage {
            id,
            public: false,
            part: Part::Abort,
            checksum: 0,
            command: Command::File,
            data: vec![],
        }
    }
    pub fn ask_to_repeat(id: Id, part: Part) -> Self {
        UdpMessage {
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
        let recepients = Recepients::from_ip(msg.ip(), msg.public);
        let (command, data) = match &msg.content {
            Content::Ping(name) => (Command::Enter, be_u8_from_str(name)),
            Content::Text(text) => (Command::Text, be_u8_from_str(text)),
            Content::Icon(icon) => (Command::Text, be_u8_from_str(&format!(" {icon}"))),
            Content::Exit => (Command::Exit, vec![]),
            Content::Empty => (Command::Error, vec![]),
            Content::FileLink(link) => (Command::File, be_u8_from_str(&link.name)),
            Content::FileData(_) => todo!(),
            Content::FileEnding(_) => todo!(),
            Content::Seen => (Command::Seen, vec![]),
        };

        if let Content::FileLink(link) = &msg.content {
            let count = link.size.div_ceil(DATA_LIMIT_BYTES as u64);
            debug!("Count {count}");
            let total_checksum = 0;
            let message = UdpMessage {
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
            // outbox.add(msg.ip(), message.clone());
            sender.send(message, recepients)?;
            outbox.files.insert(msg.id, link.clone());

            Ok(())
        } else {
            let checksum = CRC.checksum(&data);
            let total_checksum = CRC.checksum(&data);
            let count = data.chunks(DATA_LIMIT_BYTES).count() as u64;
            let chunks = data.chunks(DATA_LIMIT_BYTES);
            if data.len() < DATA_LIMIT_BYTES {
                let message = UdpMessage {
                    id: msg.id,
                    part: Part::Single,
                    public: msg.public,
                    checksum,
                    command,
                    data,
                };
                if message.command == Command::Text && !msg.is_public() {
                    outbox.add(msg.ip(), message.clone());
                }
                sender.send(message, recepients)?;
                Ok(())
            } else {
                let message = UdpMessage {
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
                    outbox.add(msg.ip(), message.clone());
                }
                sender.send(message, recepients)?;

                for (i, chunk) in chunks.enumerate() {
                    sender.send(
                        UdpMessage {
                            id: msg.id,
                            part: Part::Shard(i as ShardCount),
                            checksum: CRC.checksum(chunk),
                            public: msg.public,
                            command,
                            data: chunk.to_vec(),
                        },
                        recepients,
                    )?;
                }
                Ok(())
            }
        }
    }

    pub fn from_be_bytes(bytes: &[u8]) -> Option<Self> {
        let mut header = u8::from_be(*bytes.first()?);
        let public = (header & 1) != 0;
        header >>= 1;
        let part_n = header & 3;
        header >>= 2;
        let command = Command::from_code(header);
        let id = u32::from_be_bytes(bytes.get(1..=4)?.try_into().ok()?);
        let checksum = u16::from_be_bytes(bytes.get(5..=6)?.try_into().ok()?);
        let (part, data) = match part_n {
            1 => (
                Part::Init(PartInit {
                    total_checksum: u16::from_be_bytes(bytes.get(7..=8)?.try_into().ok()?),
                    count: u64::from_be_bytes(bytes.get(9..=16)?.try_into().ok()?),
                }),
                bytes.get(17..)?.to_owned(),
            ),
            2 => (
                Part::AskRange(
                    u64::from_be_bytes(bytes.get(7..=14)?.try_into().ok()?)
                        ..=u64::from_be_bytes(bytes.get(15..=22)?.try_into().ok()?),
                ),
                bytes.get(23..)?.to_owned(),
            ),
            3 => (
                Part::Shard(u64::from_be_bytes(bytes.get(7..=14)?.try_into().ok()?)),
                bytes.get(15..)?.to_owned(),
            ),
            _ => (Part::Single, bytes.get(7..)?.to_owned()),
        };
        // FIXME
        // if checksum == CRC.checksum(&data) || command == Command::Repeat {
        Some(UdpMessage {
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
        bytes.extend(self.id.to_be_bytes());
        bytes.extend(self.checksum.to_be_bytes());
        match &self.part {
            Part::Single | Part::Abort => (),
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

pub fn send_shards(
    link: Arc<FileLink>,
    range: RangeInclusive<ShardCount>,
    id: Id,
    recepients: Recepients,
    socket: Arc<UdpSocket>,
    port: Port,
    ctx: impl Repaintable,
) -> Result<(), Box<dyn Error + 'static>> {
    let file = fs::File::open(&link.path)?;

    for i in range {
        let mut data = vec![0; DATA_LIMIT_BYTES];
        file.read_at(&mut data, DATA_LIMIT_BYTES as u64 * i)?;
        send(
            &socket,
            port,
            UdpMessage {
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
