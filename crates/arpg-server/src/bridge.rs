use arpg_core::{PlayerId, SkillId, WorldPos};
use arpg_protocol::messages as msg;
use arpg_sim::command::Admission;
use arpg_sim::{ClientCommand, CommandEnvelope, MoveIntent, MovementMode, UseSkillIntent};

/// Translation layer between wire DTOs and simulation intents.
/// A wire command describes an intention, never a result (INV-009).
pub fn wire_to_sim(envelope: &msg::CommandEnvelope) -> Option<CommandEnvelope> {
    let player = PlayerId(envelope.player_id);
    let command = match &envelope.command {
        Some(msg::command_envelope::Command::Move(m)) => ClientCommand::Move(MoveIntent {
            direction: WorldPos::new(m.x, m.y),
            movement_mode: movement_mode_from_wire(m.movement_mode),
            sequence: envelope.sequence,
        }),
        Some(msg::command_envelope::Command::UseSkill(s)) => {
            ClientCommand::UseSkill(UseSkillIntent {
                skill: SkillId(s.skill_id),
                target: match (s.target_x, s.target_y) {
                    (Some(x), Some(y)) => Some(WorldPos::new(x, y)),
                    _ => None,
                },
            })
        }
        Some(msg::command_envelope::Command::NoOp(_)) => ClientCommand::NoOp,
        None => return None,
    };
    Some(CommandEnvelope {
        sequence: envelope.sequence,
        client_tick: arpg_core::Tick(envelope.client_tick),
        player,
        command,
    })
}

fn movement_mode_from_wire(v: u32) -> MovementMode {
    match v {
        0 => MovementMode::Walk,
        1 => MovementMode::Run,
        2 => MovementMode::Forced,
        3 => MovementMode::Knockback,
        _ => MovementMode::Teleport,
    }
}

pub fn admission_to_wire(a: Admission) -> &'static str {
    match a {
        Admission::Accepted => "Accepted",
        Admission::Deferred => "Deferred",
        Admission::RejectedTooOld => "RejectedTooOld",
        Admission::RejectedInvalid => "RejectedInvalid",
        Admission::Duplicate => "Duplicate",
    }
}

/// Build a wire snapshot from the authoritative state (INV-013: the client
/// receives only a subset of server state).
pub fn snapshot_to_wire(
    snapshot_id: u64,
    tick: arpg_core::Tick,
    players: &std::collections::BTreeMap<PlayerId, arpg_sim::PlayerState>,
    last_seq: u32,
) -> msg::Snapshot {
    msg::Snapshot {
        snapshot_id,
        tick: tick.0,
        players: players
            .values()
            .map(|p| msg::PlayerView {
                player_id: p.player.0,
                x: p.pos.x,
                y: p.pos.y,
                life: p.life,
                mana: p.mana,
            })
            .collect(),
        last_processed_command_sequence: last_seq,
    }
}
