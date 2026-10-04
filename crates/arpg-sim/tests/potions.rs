//! Potions (SPEC.md sections 82-83): instant life/mana gain, over-time
//! regen applied per tick during the Regeneration phase, consumption of
//! the item on use, and rejection of unknown/non-potion items.

use arpg_core::{ItemDefId, ItemId, PlayerId, WorldPos};
use arpg_sim::command::{ClientCommand, CommandEnvelope, UseItemIntent};
use arpg_sim::{GameInstance, MoveIntent, MovementMode};
use std::sync::Arc;

fn game() -> GameInstance {
    let data = Arc::new(arpg_data::datapack::compile_reference_datapack());
    let rules = Arc::new(arpg_rules::GameRules::default());
    let mut inst = GameInstance::new(data, rules, [61u8; 32]);
    inst.add_player(PlayerId(1), WorldPos::new(0, 0));
    inst
}

fn submit_use_item(inst: &mut GameInstance, seq: u32, item: ItemId) {
    inst.submit_command(CommandEnvelope {
        sequence: seq,
        client_tick: arpg_core::Tick(0),
        player: PlayerId(1),
        command: ClientCommand::UseItem(UseItemIntent { item }),
    });
}

fn spawn_potion_in_belt(inst: &mut GameInstance, def: ItemDefId) -> ItemId {
    let id = arpg_core::ItemId(inst.inventory.highest_item_id() + 1);
    let item = arpg_sim::item::ItemInstance {
        id,
        definition: def,
        quality: arpg_sim::item::ItemQuality::Normal,
        item_level: 1,
        generation_seed: [0; 32],
        affixes: smallvec::SmallVec::new(),
        sockets: smallvec::SmallVec::new(),
        durability: None,
        flags: 0,
    };
    inst.inventory
        .spawn_ground(item, arpg_core::LevelInstanceId(0), WorldPos::new(0, 0));
    id
}

#[test]
fn instant_healing_potion_restores_life() {
    let mut inst = game();
    // damage the player first
    inst.apply_admin_command(&arpg_sim::admin::AdminCommand::SetStat {
        player: PlayerId(1),
        stat: arpg_sim::admin::AdminStat::Life,
        value: 55,
    })
    .unwrap();
    let before = inst.state.players.get(&PlayerId(1)).unwrap().life;
    assert!(before < 100);

    let potion = spawn_potion_in_belt(&mut inst, ItemDefId(1001)); // Healing Potion
    submit_use_item(&mut inst, 1, potion);
    for _ in 0..(arpg_sim::DEFAULT_INPUT_DELAY_TICKS + 2) {
        inst.tick();
    }
    let after = inst.state.players.get(&PlayerId(1)).unwrap().life;
    assert!(
        after > before,
        "healing potion must restore life ({before} -> {after})"
    );
    // the potion is consumed
    assert!(inst.inventory.get(potion).is_none());
}

#[test]
fn overtime_potion_regen_spreads_over_ticks() {
    let mut inst = game();
    inst.apply_admin_command(&arpg_sim::admin::AdminCommand::SetStat {
        player: PlayerId(1),
        stat: arpg_sim::admin::AdminStat::Life,
        value: 40,
    })
    .unwrap();
    let before = inst.state.players.get(&PlayerId(1)).unwrap().life;

    let potion = spawn_potion_in_belt(&mut inst, ItemDefId(1011)); // Slow Refill
    submit_use_item(&mut inst, 1, potion);
    for _ in 0..(arpg_sim::DEFAULT_INPUT_DELAY_TICKS + 2) {
        inst.tick();
    }
    let mid = inst.state.players.get(&PlayerId(1)).unwrap().life;
    assert!(mid > before, "regen must tick life up ({before} -> {mid})");

    // let the effect finish
    for _ in 0..110 {
        inst.tick();
    }
    let after = inst.state.players.get(&PlayerId(1)).unwrap().life;
    assert!(after > mid);
    assert!(inst
        .state
        .players
        .get(&PlayerId(1))
        .unwrap()
        .active_regen
        .is_none());
}

#[test]
fn non_potion_item_is_rejected() {
    let mut inst = game();
    let sword = spawn_potion_in_belt(&mut inst, ItemDefId(1)); // base weapon, no potion effect
    let before = inst.state.players.get(&PlayerId(1)).unwrap().life;
    submit_use_item(&mut inst, 1, sword);
    for _ in 0..(arpg_sim::DEFAULT_INPUT_DELAY_TICKS + 2) {
        inst.tick();
    }
    let after = inst.state.players.get(&PlayerId(1)).unwrap().life;
    assert_eq!(before, after, "a non-potion has no effect");
    // not consumed either
    assert!(inst.inventory.get(sword).is_some());
}

#[test]
fn unknown_item_is_ignored() {
    let mut inst = game();
    submit_use_item(&mut inst, 1, ItemId(9999));
    for _ in 0..(arpg_sim::DEFAULT_INPUT_DELAY_TICKS + 2) {
        inst.tick();
    }
    assert_eq!(inst.state.players.get(&PlayerId(1)).unwrap().life, 100);
}

#[test]
fn potion_use_is_deterministic() {
    let run = || {
        let mut inst = game();
        inst.apply_admin_command(&arpg_sim::admin::AdminCommand::SetStat {
            player: PlayerId(1),
            stat: arpg_sim::admin::AdminStat::Life,
            value: 50,
        })
        .unwrap();
        let potion = spawn_potion_in_belt(&mut inst, ItemDefId(1000));
        submit_use_item(&mut inst, 1, potion);
        // a movement command too, to exercise the scheduler ordering
        inst.submit_command(CommandEnvelope {
            sequence: 2,
            client_tick: arpg_core::Tick(0),
            player: PlayerId(1),
            command: ClientCommand::Move(MoveIntent {
                direction: WorldPos::new(1, 0),
                movement_mode: MovementMode::Walk,
                sequence: 2,
            }),
        });
        let mut hash = [0u8; 32];
        for _ in 0..20 {
            hash = inst.tick().state_hash;
        }
        hash
    };
    assert_eq!(run(), run());
}

#[test]
fn resistance_potion_applies_and_expires_state() {
    let mut inst = game();
    let elixir = spawn_potion_in_belt(&mut inst, ItemDefId(1010)); // Fire Resist 600 ticks
    submit_use_item(&mut inst, 1, elixir);
    for _ in 0..(arpg_sim::DEFAULT_INPUT_DELAY_TICKS + 2) {
        inst.tick();
    }
    let entity = arpg_core::EntityId(1);
    assert!(
        inst.states
            .get(entity, GameInstance::STATE_RESIST_FIRE, entity)
            .is_some(),
        "fire resistance state must be active"
    );
    // expiry: run past the duration
    for _ in 0..700 {
        inst.tick();
    }
    assert!(
        inst.states
            .get(entity, GameInstance::STATE_RESIST_FIRE, entity)
            .is_none(),
        "resistance state must expire"
    );
}

#[test]
fn antidote_cures_poison_state() {
    let mut inst = game();
    let entity = arpg_core::EntityId(1);
    // apply a poison state first (section 54)
    let instance = arpg_sim::StateInstance {
        state: GameInstance::STATE_POISONED,
        source: entity,
        source_skill: None,
        applied_tick: inst.state.tick,
        expires_tick: Some(arpg_core::Tick(inst.state.tick.0 + 500)),
        stack_key: (GameInstance::STATE_POISONED, entity.0),
        magnitude_bp: 0,
    };
    inst.states
        .apply(entity, instance, arpg_sim::StackPolicy::Refresh);
    assert!(inst
        .states
        .get(entity, GameInstance::STATE_POISONED, entity)
        .is_some());

    let antidote = spawn_potion_in_belt(&mut inst, ItemDefId(1007));
    submit_use_item(&mut inst, 1, antidote);
    for _ in 0..(arpg_sim::DEFAULT_INPUT_DELAY_TICKS + 2) {
        inst.tick();
    }
    assert!(
        inst.states
            .get(entity, GameInstance::STATE_POISONED, entity)
            .is_none(),
        "antidote must cure poison"
    );
}

#[test]
fn resistance_potion_halves_elemental_damage() {
    use arpg_sim::StateInstance;
    let mut inst = game();
    let entity = arpg_core::EntityId(1);
    // Fire resistance state at 50% (section 54), as the elixir would apply
    let instance = StateInstance {
        state: GameInstance::STATE_RESIST_FIRE,
        source: entity,
        source_skill: None,
        applied_tick: inst.state.tick,
        expires_tick: Some(arpg_core::Tick(inst.state.tick.0 + 600)),
        stack_key: (GameInstance::STATE_RESIST_FIRE, entity.0),
        magnitude_bp: 5000,
    };
    inst.states
        .apply(entity, instance, arpg_sim::StackPolicy::Refresh);
    let resists = inst.effective_resists(entity);
    assert_eq!(resists.fire_bp, 5000);
    // A fire attack of 40 must resolve to 20
    let roll = arpg_sim::RollAmounts {
        physical: 0,
        magic: 0,
        fire: 40,
        cold: 0,
        lightning: 0,
    };
    assert_eq!(arpg_sim::damage::resolve_damage(roll, &resists), 20);
}

#[test]
fn monster_fire_damage_reduced_by_resist_state() {
    let mut inst = game();
    let player = arpg_core::EntityId(1);
    // find a fire-damage monster in the datapack (ranged => elemental)
    let fire_id = inst
        .data
        .monsters
        .iter()
        .find(|(_, d)| d.damage_type == 2)
        .map(|(id, _)| *id)
        .expect("datapack has a fire monster");
    let monster = inst.spawn_monster_def(fire_id, WorldPos::new(0, 1));
    let m = inst.monsters.get(&monster).unwrap();
    assert_eq!(m.damage_type, 2);
    let raw = m.damage;
    // Without resistance
    inst.monster_attack(monster, player);
    let trace = inst.traces.recent_attacks().last().unwrap();
    assert_eq!(trace.resistance_percent, 0);
    let baseline = trace.final_damage;
    assert!(baseline > 0);
    // Apply 50% fire resistance and attack again at a later tick
    let instance = arpg_sim::StateInstance {
        state: GameInstance::STATE_RESIST_FIRE,
        source: player,
        source_skill: None,
        applied_tick: inst.state.tick,
        expires_tick: Some(arpg_core::Tick(inst.state.tick.0 + 600)),
        stack_key: (GameInstance::STATE_RESIST_FIRE, player.0),
        magnitude_bp: 5000,
    };
    inst.states
        .apply(player, instance, arpg_sim::StackPolicy::Refresh);
    inst.state.tick = arpg_core::Tick(inst.state.tick.0 + 1);
    inst.monster_attack(monster, player);
    let trace = inst.traces.recent_attacks().last().unwrap();
    assert_eq!(trace.resistance_percent, 50);
    // Misses reduce final damage to 0; only compare when both hit
    let attacks: Vec<_> = inst.traces.recent_attacks().collect();
    if attacks[attacks.len() - 2].hit && trace.hit {
        assert_eq!(
            trace.final_damage,
            attacks[attacks.len() - 2].final_damage / 2
        );
        assert_eq!(trace.physical_raw, raw);
    }
}
