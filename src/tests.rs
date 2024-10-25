use crate::chat::message::{Command, Part, UdpMessage};

#[test]
pub fn protocol() {
    for cmd_id in 0..=10 {
        let cmd = Command::from_code(cmd_id);
        let msg = match cmd {
            Command::Enter => UdpMessage::enter("name"),
            Command::Greating => UdpMessage::greating("name"),
            Command::Text => UdpMessage::new_single(Command::Text, vec![], false),
            Command::File => UdpMessage::new_single(Command::File, vec![], false),
            Command::AskToRepeat => UdpMessage::ask_to_repeat(4, Part::Single),
            Command::Repeat => UdpMessage::new_single(Command::Text, vec![], false),
            Command::Exit => UdpMessage::exit(),
            Command::Seen => UdpMessage::seen_id(0, true),
            Command::Error => UdpMessage::new_single(Command::Error, vec![], true),
            Command::Abort => UdpMessage::abort(5),
        };
        assert_eq!(UdpMessage::from_be_bytes(&msg.to_be_bytes()), Some(msg));
    }
}
