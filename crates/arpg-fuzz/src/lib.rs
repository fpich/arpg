//! Fuzzing and network chaos harnesses (SPEC.md sections 183, 186-187, M10).
//!
//! Targets (section 183): protocol decoder, datapack validation, inventory
//! operations, recipe matcher, skill IR validator, quest validator. An
//! invalid input must produce an error, **never a panic**.
//!
//! Network chaos (section 186): simulated latency, jitter, loss,
//! duplication, reordering, disconnect/reconnect. The server hash must stay
//! independent of network anomalies for the same set of finally accepted
//! commands.

pub mod chaos;
pub mod corpus;

use arpg_core::{PlayerId, Tick, WorldPos};
use arpg_sim::ClientCommand::NoOp;
use arpg_sim::{
    ClientCommand, CommandEnvelope, InteractIntent, MoveIntent, MovementMode, UseSkillIntent,
};

/// Deterministic RNG for corpus generation (no global RNG, SPEC section 60):
/// splitmix64 seeded per domain.
pub struct FuzzRng(pub u64);

impl FuzzRng {
    pub fn gen(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }

    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            0
        } else {
            self.gen() % n
        }
    }
}

/// A raw fuzzed command decoded from arbitrary bytes: exercises the
/// admission path (sequence, ticks, intents) with hostile values.
pub fn decode_fuzzed_command(bytes: &[u8]) -> Result<CommandEnvelope, &'static str> {
    if bytes.len() < 9 {
        return Err("truncated command");
    }
    let sequence = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let client_tick = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as u64;
    let player = PlayerId(bytes[9.min(bytes.len() - 1)] as u32);
    let command = match bytes.len() {
        0..9 => unreachable!(),
        9..=12 => ClientCommand::NoOp,
        13..=15 => ClientCommand::NoOp,
        _ => match bytes[9] % 4 {
            0 => ClientCommand::Move(MoveIntent {
                direction: WorldPos::new(
                    i32::from_le_bytes([bytes[10], bytes[11], bytes[12], bytes[13]]),
                    i32::from_le_bytes([bytes[14], bytes[15], bytes[16.min(bytes.len() - 1)], 0]),
                ),
                movement_mode: MovementMode::Run,
                sequence,
            }),
            1 => ClientCommand::UseSkill(UseSkillIntent {
                skill: arpg_core::SkillId(u32::from_le_bytes([
                    bytes[10], bytes[11], bytes[12], bytes[13],
                ])),
                target: Some(WorldPos::new(bytes[14] as i32, bytes[15] as i32)),
            }),
            2 => ClientCommand::Interact(InteractIntent {
                target: arpg_core::ObjectId(u64::from_le_bytes([
                    bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15], 0, 0,
                ])),
            }),
            _ => NoOp,
        },
    };
    if sequence == 0 {
        return Err("invalid sequence");
    }
    Ok(CommandEnvelope {
        sequence,
        client_tick: Tick(client_tick),
        player,
        command,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzed_commands_never_panic_and_reject_truncated() {
        let mut rng = FuzzRng(0xDEADBEEF);
        for _ in 0..10_000 {
            let len = rng.below(64) as usize;
            let bytes: Vec<u8> = (0..len).map(|_| rng.gen() as u8).collect();
            // must never panic: either a valid envelope or a clean error
            let _ = decode_fuzzed_command(&bytes);
        }
        assert!(decode_fuzzed_command(&[0; 4]).is_err());
        let mut ok_bytes = [0u8; 16];
        ok_bytes[0] = 1; // sequence = 1
        ok_bytes[9] = 12; // move-ish tag -> NoOp path (len 16 >= 13)
        let env = decode_fuzzed_command(&ok_bytes).unwrap();
        assert_eq!(env.sequence, 1);
    }

    #[test]
    fn rng_is_deterministic_per_seed() {
        let mut a = FuzzRng(42);
        let mut b = FuzzRng(42);
        for _ in 0..100 {
            assert_eq!(a.gen(), b.gen());
        }
    }
}
