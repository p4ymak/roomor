use crc::{Crc, CRC_16_IBM_SDLC};
use enumn::N;
use std::{fmt, time::SystemTime};

use super::TextMessage;

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
    Icon,
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
pub struct Message {
    pub id: Id,
    checksum: u16,
    pub command: Command,
    pub public: bool,
    pub data: Vec<u8>,
}

impl fmt::Display for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "\nMessage #{}\nChecksum: {}\n{:?}\n'{}'\n",
            self.id,
            self.checksum,
            self.command,
            match self.command {
                Command::Text | Command::Icon | Command::Error | Command::Repeat =>
                    string_from_be_u8(&self.data),
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

impl Message {
    pub fn new(command: Command, data: Vec<u8>, public: bool) -> Self {
        let id = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("System Time")
            .as_secs() as u32;
        let checksum = CRC.checksum(&data);
        Message {
            id,
            checksum,
            public,
            command,
            data,
        }
    }

    pub fn _retry_text(id: u32, text: &str) -> Self {
        let data = be_u8_from_str(
            text.to_owned()
                .chars()
                .filter(|c| !c.is_control())
                .collect::<String>()
                .as_ref(),
        );
        let checksum = CRC.checksum(&data);
        Message {
            id,
            checksum,
            command: Command::Repeat,
            data,
            public: true,
        }
    }

    pub fn enter(name: &str) -> Self {
        Message::new(Command::Enter, be_u8_from_str(name), true)
    }
    pub fn greating(name: &str) -> Self {
        Message::new(Command::Greating, be_u8_from_str(name), true)
    }
    pub fn exit() -> Self {
        Message::new(Command::Exit, vec![], true)
    }
    pub fn ask_name() -> Self {
        Message::new(Command::AskToRepeat, 0_u32.to_be_bytes().to_vec(), true)
    }
    pub fn from_message(msg: &TextMessage) -> Self {
        let (command, text) = match &msg.content {
            super::Content::Enter(name) => (Command::Enter, name.as_str()),
            super::Content::Text(text) => (Command::Text, text.as_str()),
            super::Content::Icon(icon) => (Command::Icon, icon.as_str()),
            super::Content::Alive => (Command::Greating, ""),
            super::Content::Exit => (Command::Exit, ""),
            super::Content::Empty => (Command::Error, ""),
        };
        let data = be_u8_from_str(
            text.chars()
                .filter(|c| !c.is_control())
                .collect::<String>()
                .as_ref(),
        );
        Message::new(command, data, msg.public)
    }
    pub fn text(text: &str, public: bool) -> Self {
        Message::new(
            Command::Text,
            be_u8_from_str(
                text.to_owned()
                    .chars()
                    .filter(|c| !c.is_control())
                    .collect::<String>()
                    .as_ref(),
            ),
            public,
        )
    }
    pub fn icon(text: &str, public: bool) -> Self {
        Message::new(
            Command::Icon,
            be_u8_from_str(
                text.to_owned()
                    .chars()
                    .filter(|c| !c.is_control())
                    .collect::<String>()
                    .as_ref(),
            ),
            public,
        )
    }
    pub fn from_be_bytes(bytes: &[u8]) -> Option<Self> {
        let id = u32::from_be_bytes([
            *bytes.first()?,
            *bytes.get(1)?,
            *bytes.get(2)?,
            *bytes.get(3)?,
        ]);
        let checksum = u16::from_be_bytes([*bytes.get(4)?, *bytes.get(5)?]);
        let command = Command::from_code(u8::from_be_bytes([*bytes.get(6)?]));
        let public = bytes.get(7)?.count_ones() > 0;
        let data = match bytes.len() {
            0..=8 => [].to_vec(),
            _ => bytes[8..].to_owned(),
        };
        if checksum == CRC.checksum(&data) || command == Command::Repeat {
            Some(Message {
                id,
                checksum,
                command,
                public,
                data,
            })
        } else {
            Some(Message {
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
        bytes.extend(self.id.to_be_bytes());
        bytes.extend(self.checksum.to_be_bytes());
        bytes.extend(self.command.to_code().to_be_bytes());
        bytes.extend([self.public as u8]);
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
    text.trim().as_bytes().to_owned()
}
