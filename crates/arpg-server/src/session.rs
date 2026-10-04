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
    /// Remaining ticks of the reconnection grace window (section 146).
    grace_remaining_ticks: u32,
}

impl Session {
    pub fn new(player: PlayerId) -> Session {
        Session {
            player,
            state: SessionState::Connecting,
            protocol_version: 0,
            last_processed_sequence: 0,
            joined_tick: Tick(0),
            grace_remaining_ticks: 0,
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

    /// Enter the reconnection grace window (SPEC.md section 146): the
    /// character stays in the world, no new actions, engaged actions
    /// finish normally, the player can be attacked. Policy: 30 seconds,
    /// overridable.
    pub fn disconnect(&mut self) {
        self.disconnect_with_policy(Self::DEFAULT_RECONNECT_GRACE_TICKS);
    }

    pub fn disconnect_with_policy(&mut self, grace_ticks: u32) {
        self.grace_remaining_ticks = grace_ticks.max(1);
        self.state = SessionState::DisconnectedGrace;
    }

    /// A reconnecting client resumes its session: back to Running with the
    /// grace window cleared (section 146).
    pub fn reconnect(&mut self) {
        self.grace_remaining_ticks = 0;
        self.state = SessionState::Running;
    }

    /// Advance the grace window by one tick; returns true when the grace
    /// expired (definitive disconnection, section 147).
    pub fn tick_grace(&mut self) -> bool {
        if self.state != SessionState::DisconnectedGrace {
            return false;
        }
        if self.grace_remaining_ticks > 0 {
            self.grace_remaining_ticks -= 1;
        }
        self.grace_remaining_ticks == 0
    }

    pub fn grace_remaining_ticks(&self) -> u32 {
        self.grace_remaining_ticks
    }

    pub fn close(&mut self) {
        self.state = SessionState::Closed;
    }
}

impl Session {
    /// Default reconnection policy (section 146): 30 seconds at 25 ticks/s.
    pub const DEFAULT_RECONNECT_GRACE_TICKS: u32 = 30 * 25;
}
