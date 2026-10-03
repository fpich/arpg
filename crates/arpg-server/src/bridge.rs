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
        deltas: vec![],
    }
}

/// Convert a sim ClientReplication into wire deltas (section 141).
pub fn replication_to_wire(
    repl: &arpg_sim::ClientReplication,
    ack: &mut impl FnMut(PlayerId, u64),
) -> msg::Snapshot {
    let mut deltas = Vec::new();
    for &(entity, base, rev, ref p) in &repl.deltas {
        ack(entity, rev);
        deltas.push(msg::EntityDelta {
            entity_id: entity.0 as u64,
            base_revision: base,
            new_revision: rev,
            field_mask: 0xFFFF,
            player_view: Some(msg::PlayerView {
                player_id: p.player.0,
                x: p.pos.x,
                y: p.pos.y,
                life: p.life,
                mana: p.mana,
            }),
        });
    }
    msg::Snapshot {
        snapshot_id: repl.snapshot_id,
        tick: repl.tick.0,
        players: vec![],
        last_processed_command_sequence: 0,
        deltas,
    }
}

/// Full entity state after an EntityResyncRequest (section 142).
pub fn resync_to_wire(
    entity: PlayerId,
    revision: u64,
    p: &arpg_sim::PlayerState,
) -> msg::EntityResyncResponse {
    msg::EntityResyncResponse {
        entity_id: entity.0 as u64,
        server_revision: revision,
        player_view: Some(msg::PlayerView {
            player_id: p.player.0,
            x: p.pos.x,
            y: p.pos.y,
            life: p.life,
            mana: p.mana,
        }),
    }
}
