use crc::{Crc, CRC_16_IBM_SDLC};
use enumn::N;
use std::{fmt, time::SystemTime};

use super::{Content, TextMessage};

pub const CRC: Crc<u16> = Crc::<u16>::new(&CRC_16_IBM_SDLC);
pub const MAX_TEXT_SIZE: usize = 116;
pub const MAX_EMOJI_SIZE: usize = 8;
pub const MAX_NAME_SIZE: usize = 44;

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

pub type Id = u32;

#[derive(Debug, Clone)]
pub struct UdpMessage {
    pub id: Id,
    checksum: u16,
    pub command: Command,
    pub public: bool,
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
    pub fn new(command: Command, data: Vec<u8>, public: bool) -> Self {
        let id = new_id();
        let checksum = CRC.checksum(&data);
        UdpMessage {
            id,
            checksum,
            public,
            command,
            data,
        }
    }

    pub fn enter(name: &str) -> Self {
        UdpMessage::new(Command::Enter, be_u8_from_str(name), true)
    }
    pub fn greating(name: &str) -> Self {
        UdpMessage::new(Command::Greating, be_u8_from_str(name), true)
    }
    pub fn exit() -> Self {
        UdpMessage::new(Command::Exit, vec![], true)
    }
    pub fn ask_name() -> Self {
        UdpMessage::new(Command::AskToRepeat, 0_u32.to_be_bytes().to_vec(), true)
    }
    pub fn from_message(msg: &TextMessage) -> Self {
        let (command, data) = match &msg.content {
            Content::Ping(name) => (Command::Enter, be_u8_from_str(name)),
            Content::Text(text) => (Command::Text, be_u8_from_str(text)),
            Content::Icon(icon) => (Command::Text, be_u8_from_str(&format!(" {icon}"))),
            Content::Exit => (Command::Exit, vec![]),
            Content::Empty => (Command::Error, vec![]),
            Content::FileLink(link) => (Command::Text, be_u8_from_str(&link.to_text())),
            Content::FileData(_) => todo!(),
            Content::FileEnding(_) => todo!(),
        };
        UdpMessage::new(command, data, msg.public)
    }
    pub fn from_be_bytes(bytes: &[u8]) -> Option<Self> {
        let command = Command::from_code(u8::from_be_bytes([*bytes.first()?]));
        let public = bytes.get(1)?.count_ones() > 0;
        let id = u32::from_be_bytes([
            *bytes.get(2)?,
            *bytes.get(3)?,
            *bytes.get(4)?,
            *bytes.get(5)?,
        ]);
        let checksum = u16::from_be_bytes([*bytes.get(6)?, *bytes.get(7)?]);
        let data = match bytes.len() {
            0..=8 => [].to_vec(),
            _ => bytes[8..].to_owned(),
        };
        if checksum == CRC.checksum(&data) || command == Command::Repeat {
            Some(UdpMessage {
                id,
                checksum,
                command,
                public,
                data,
            })
        } else {
            Some(UdpMessage {
                id,
                checksum,
                command: Command::Error,
                public,
                data,
            })
        }
    }

    pub fn to_be_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::<u8>::new();
        bytes.extend(self.command.to_code().to_be_bytes());
        bytes.extend([self.public as u8]);
        bytes.extend(self.id.to_be_bytes());
        bytes.extend(self.checksum.to_be_bytes());
        bytes.extend(self.data.to_owned());

        bytes
    }

    pub fn read_text(&self) -> String {
        string_from_be_u8(&self.data)
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
