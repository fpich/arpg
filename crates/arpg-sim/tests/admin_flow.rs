//! Admin commands (SPEC.md section 169): spawn, give_item, give_skill,
//! teleport, set_stat, kill, complete_quest, dump_entity, dump_rng.
//! Deterministic: same commands on the same state produce the same hashes.

use arpg_core::{EntityId, ItemDefId, PlayerId, WorldPos};
use arpg_sim::admin::{AdminCommand, AdminError, AdminStat};
use arpg_sim::GameInstance;
use std::sync::Arc;

fn game() -> GameInstance {
    let data = Arc::new(arpg_data::datapack::compile_reference_datapack());
    let rules = Arc::new(arpg_rules::GameRules::default());
    let mut inst = GameInstance::new(data, rules, [55u8; 32]);
    inst.add_player(PlayerId(1), WorldPos::new(0, 0));
    inst
}

#[test]
fn spawn_kill_and_dump_entity() {
    let mut inst = game();
    let spawned = inst.apply_admin_command(&AdminCommand::Spawn {
        def: 0,
        pos: WorldPos::new(3, 3),
    });
    let entity = match spawned.unwrap() {
        arpg_sim::admin::AdminOutcome::Spawned(e) => e,
        _ => panic!("expected Spawned"),
    };
    assert!(inst.monsters.contains_key(&entity));

    // dump before death
    match inst
        .apply_admin_command(&AdminCommand::DumpEntity { entity })
        .unwrap()
    {
        arpg_sim::admin::AdminOutcome::EntityDump { pos, life } => {
            assert_eq!(pos, WorldPos::new(3, 3));
            assert!(life > 0);
        }
        _ => panic!("expected EntityDump"),
    }

    inst.apply_admin_command(&AdminCommand::Kill { entity })
        .unwrap();
    assert_eq!(inst.monsters.get(&entity).unwrap().life, 0);

    let err = inst
        .apply_admin_command(&AdminCommand::Kill {
            entity: EntityId(9999),
        })
        .unwrap_err();
    assert_eq!(err, AdminError::UnknownEntity);
}

#[test]
fn teleport_set_stat_and_dump_rng() {
    let mut inst = game();
    inst.apply_admin_command(&AdminCommand::Teleport {
        player: PlayerId(1),
        pos: WorldPos::new(10, 20),
    })
    .unwrap();
    assert_eq!(
        inst.state.players.get(&PlayerId(1)).unwrap().pos,
        WorldPos::new(10, 20)
    );

    inst.apply_admin_command(&AdminCommand::SetStat {
        player: PlayerId(1),
        stat: AdminStat::Life,
        value: 777,
    })
    .unwrap();
    assert_eq!(inst.state.players.get(&PlayerId(1)).unwrap().life, 777);

    match inst.apply_admin_command(&AdminCommand::DumpRng).unwrap() {
        arpg_sim::admin::AdminOutcome::RngDump { seed, tick } => {
            assert_eq!(seed, [55u8; 32]);
            assert_eq!(tick, inst.state.tick.0);
        }
        _ => panic!("expected RngDump"),
    }

    let err = inst
        .apply_admin_command(&AdminCommand::Teleport {
            player: PlayerId(42),
            pos: WorldPos::new(0, 0),
        })
        .unwrap_err();
    assert_eq!(err, AdminError::UnknownPlayer);
}

#[test]
fn give_item_mints_deterministic_ids() {
    let mut a = game();
    let mut b = game();
    let cmd = AdminCommand::GiveItem {
        player: PlayerId(1),
        def: ItemDefId(1),
    };
    let first = a.apply_admin_command(&cmd).unwrap();
    let second = b.apply_admin_command(&cmd).unwrap();
    assert_eq!(first, second, "admin item minting must be deterministic");
}

#[test]
fn admin_sequence_is_deterministic() {
    let run = || {
        let mut inst = game();
        inst.apply_admin_command(&AdminCommand::Spawn {
            def: 2,
            pos: WorldPos::new(1, 1),
        })
        .unwrap();
        inst.apply_admin_command(&AdminCommand::Teleport {
            player: PlayerId(1),
            pos: WorldPos::new(5, 5),
        })
        .unwrap();
        let mut hash = [0u8; 32];
        for _ in 0..10 {
            hash = inst.tick().state_hash;
        }
        hash
    };
    assert_eq!(run(), run());
}
