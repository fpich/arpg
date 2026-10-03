use arpg_core::{PlayerId, Tick};
use arpg_protocol::messages::CommandEnvelope;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Connecting,
    Authenticated,
    Joining,
    Synchronizing,
    Running,
    DisconnectedGrace,
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProtocolVersion(pub u32);

pub const PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion(1);

/// Server-side session (SPEC.md section 128). A gameplay command is only
/// accepted while Running.
#[derive(Debug)]
pub struct Session {
    pub player: PlayerId,
    pub state: SessionState,
    pub protocol_version: u32,
    pub last_processed_sequence: u32,
    pub joined_tick: Tick,
}

impl Session {
    pub fn new(player: PlayerId) -> Session {
        Session {
            player,
            state: SessionState::Connecting,
            protocol_version: 0,
            last_processed_sequence: 0,
            joined_tick: Tick(0),
        }
    }

    pub fn is_command_accepted(&self) -> bool {
        self.state == SessionState::Running
    }

    pub fn handle_client_hello(&mut self, hello: &arpg_protocol::messages::ClientHello) -> bool {
        if hello.protocol_version != PROTOCOL_VERSION.0 {
            return false;
        }
        self.protocol_version = hello.protocol_version;
        self.state = SessionState::Authenticated;
        true
    }

    pub fn handle_join_accepted(&mut self, tick: Tick) {
        self.joined_tick = tick;
        self.state = SessionState::Running;
    }

    pub fn record_processed(&mut self, envelope: &CommandEnvelope) {
        self.last_processed_sequence = envelope.sequence;
    }

    pub fn disconnect(&mut self) {
        self.state = SessionState::DisconnectedGrace;
    }

    pub fn close(&mut self) {
        self.state = SessionState::Closed;
    }
}
