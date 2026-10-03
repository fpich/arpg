//! Integration tests for party, PvP, XP and summons (SPEC.md sections
//! 66-67, 109-110, 113-115) wired through GameInstance.

use arpg_core::{EntityId, PlayerId, WorldPos};
use arpg_sim::social::PlayerRelation;
use arpg_sim::summon::{ReplacementPolicy, SummonFamily, SummonLimits};
use arpg_sim::GameInstance;
use std::sync::Arc;

fn setup() -> GameInstance {
    let data = Arc::new(arpg_data::GameData::default());
    let rules = Arc::new(arpg_rules::GameRules::default());
    GameInstance::new(data, rules, [13u8; 32])
}

#[test]
fn party_membership_and_hash_change() {
    let mut inst = setup();
    inst.add_player(PlayerId(1), WorldPos::ZERO);
    inst.add_player(PlayerId(2), WorldPos::ZERO);
    let before = inst.tick().state_hash;
    inst.parties.invite(PlayerId(1), PlayerId(2), 8).unwrap();
    inst.parties.accept(PlayerId(2), 8).unwrap();
    let after = inst.tick().state_hash;
    assert_ne!(before, after, "party membership is gameplay state");
    assert_eq!(
        inst.parties
            .relation(PlayerId(1), PlayerId(2), &inst.hostility),
        PlayerRelation::Party
    );
}

#[test]
fn neutral_players_cannot_damage_each_other() {
    let mut inst = setup();
    inst.add_player(PlayerId(1), WorldPos::ZERO);
    inst.add_player(PlayerId(2), WorldPos::ZERO);
    inst.apply_damage(EntityId(1), EntityId(2), 100);
    assert_eq!(inst.state.players.get(&PlayerId(1)).unwrap().life, 100);
}

#[test]
fn party_members_cannot_damage_each_other() {
    let mut inst = setup();
    inst.add_player(PlayerId(1), WorldPos::ZERO);
    inst.add_player(PlayerId(2), WorldPos::ZERO);
    inst.parties.invite(PlayerId(1), PlayerId(2), 8).unwrap();
    inst.parties.accept(PlayerId(2), 8).unwrap();
    inst.apply_damage(EntityId(1), EntityId(2), 100);
    assert_eq!(inst.state.players.get(&PlayerId(1)).unwrap().life, 100);
}

#[test]
fn hostile_players_damage_each_other() {
    let mut inst = setup();
    inst.add_player(PlayerId(1), WorldPos::ZERO);
    inst.add_player(PlayerId(2), WorldPos::ZERO);
    inst.hostility.declare(PlayerId(1), PlayerId(2));
    inst.apply_damage(EntityId(1), EntityId(2), 30);
    assert_eq!(inst.state.players.get(&PlayerId(1)).unwrap().life, 70);
}

#[test]
fn pvp_disabled_blocks_even_hostile_damage() {
    let mut inst = setup();
    let rules = arpg_rules::GameRules {
        pvp_mode: arpg_rules::PvpMode::Disabled,
        ..arpg_rules::GameRules::default()
    };
    inst.rules = Arc::new(rules);
    inst.add_player(PlayerId(1), WorldPos::ZERO);
    inst.add_player(PlayerId(2), WorldPos::ZERO);
    inst.hostility.declare(PlayerId(1), PlayerId(2));
    inst.apply_damage(EntityId(1), EntityId(2), 30);
    assert_eq!(inst.state.players.get(&PlayerId(1)).unwrap().life, 100);
}

#[test]
fn monster_kill_awards_xp_to_alive_players() {
    let mut inst = setup();
    inst.add_player(PlayerId(1), WorldPos::ZERO);
    inst.add_player(PlayerId(2), WorldPos::ZERO);
    let xp_before_1 = inst.state.players.get(&PlayerId(1)).unwrap().experience;
    let xp_before_2 = inst.state.players.get(&PlayerId(2)).unwrap().experience;
    let monster = inst.spawn_monster(WorldPos::new(10, 0));
    inst.apply_damage(monster, EntityId(1), 1000);
    inst.tick();
    assert!(inst.state.players.get(&PlayerId(1)).unwrap().experience > xp_before_1);
    assert!(inst.state.players.get(&PlayerId(2)).unwrap().experience > xp_before_2);
}

#[test]
fn dead_players_get_no_xp() {
    let mut inst = setup();
    inst.add_player(PlayerId(1), WorldPos::ZERO);
    inst.add_player(PlayerId(2), WorldPos::ZERO);
    inst.hostility.declare(PlayerId(1), PlayerId(2));
    inst.apply_damage(EntityId(2), EntityId(1), 1000);
    inst.tick();
    let xp_dead = inst.state.players.get(&PlayerId(2)).unwrap().experience;
    assert_eq!(xp_dead, 0, "dead player is not an XP participant");
}

#[test]
fn party_shares_xp_group() {
    let mut inst = setup();
    inst.add_player(PlayerId(1), WorldPos::ZERO);
    inst.add_player(PlayerId(2), WorldPos::ZERO);
    inst.parties.invite(PlayerId(1), PlayerId(2), 8).unwrap();
    inst.parties.accept(PlayerId(2), 8).unwrap();
    inst.add_player(PlayerId(3), WorldPos::ZERO);
    let monster = inst.spawn_monster(WorldPos::new(10, 0));
    inst.apply_damage(monster, EntityId(1), 1000);
    inst.tick();
    let xp1 = inst.state.players.get(&PlayerId(1)).unwrap().experience;
    let xp2 = inst.state.players.get(&PlayerId(2)).unwrap().experience;
    assert_eq!(xp1, xp2, "party members at the same level earn equally");
}

#[test]
fn summons_expire_deterministically() {
    let mut inst = setup();
    inst.add_player(PlayerId(1), WorldPos::ZERO);
    let limits = SummonLimits::new(3, ReplacementPolicy::Reject);
    let (entity, evicted) = inst
        .summons
        .summon(
            PlayerId(1),
            SummonFamily(1),
            &limits,
            |_, slot| EntityId(5000 + slot as u64),
            Some(3),
        )
        .unwrap();
    assert!(evicted.is_none());
    assert_eq!(entity, EntityId(5000));
    assert_eq!(inst.summons.active_of(PlayerId(1), SummonFamily(1)), 1);
    for _ in 0..4 {
        let expired = inst.summons.tick_expire();
        for e in expired {
            let _ = e;
        }
    }
    assert_eq!(inst.summons.active_of(PlayerId(1), SummonFamily(1)), 0);
}

#[test]
fn social_state_is_hashed_deterministically() {
    let run = || {
        let mut inst = setup();
        inst.add_player(PlayerId(1), WorldPos::ZERO);
        inst.add_player(PlayerId(2), WorldPos::ZERO);
        inst.parties.invite(PlayerId(1), PlayerId(2), 8).unwrap();
        inst.parties.accept(PlayerId(2), 8).unwrap();
        inst.hostility.declare(PlayerId(2), PlayerId(3));
        inst.tick().state_hash
    };
    assert_eq!(run(), run());
}
