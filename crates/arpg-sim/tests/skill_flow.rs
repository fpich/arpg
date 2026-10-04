use arpg_core::{PlayerId, SkillId, WorldPos};
use arpg_sim::{
    ClientCommand, CommandEnvelope, CostFormula, DamagePacket, DamageRange, SkillDefinition,
    SkillOp, SkillProgram, TargetingSpec, TimingFormula, UseSkillIntent,
};
use std::sync::Arc;

fn setup() -> GameInstance {
    let data = Arc::new(arpg_data::GameData::default());
    let rules = Arc::new(arpg_rules::GameRules::default());
    GameInstance::new(data, rules, [42u8; 32])
}

fn skill_cmd(
    player: PlayerId,
    seq: u32,
    skill: SkillId,
    target: Option<WorldPos>,
) -> CommandEnvelope {
    CommandEnvelope {
        sequence: seq,
        client_tick: arpg_core::Tick(1),
        player,
        command: ClientCommand::UseSkill(UseSkillIntent { skill, target }),
    }
}

fn fireball() -> SkillDefinition {
    SkillDefinition {
        id: SkillId(1),
        targeting: TargetingSpec::Position,
        cost: CostFormula::default(),
        timing: TimingFormula::Ticks(2),
        program: SkillProgram {
            ops: vec![
                SkillOp::DealDamage(DamagePacket {
                    fire: DamageRange::new(30, 30),
                    ..DamagePacket::default()
                }),
                SkillOp::SpawnMissile(1),
            ],
        },
    }
}

#[test]
fn use_skill_impact_damages_target() {
    let mut inst = setup();
    inst.register_skill(fireball()).unwrap();
    inst.add_player(PlayerId(1), WorldPos::new(0, 0));
    inst.add_player(PlayerId(2), WorldPos::new(256, 0));
    inst.hostility.declare(PlayerId(1), PlayerId(2));
    let target_pos = WorldPos::new(256, 0);
    inst.submit_command(skill_cmd(PlayerId(1), 1, SkillId(1), Some(target_pos)));
    let life_before = inst.state.players.get(&PlayerId(2)).unwrap().life;
    for _ in 0..8 {
        inst.tick();
    }
    let life_after = inst.state.players.get(&PlayerId(2)).unwrap().life;
    assert!(
        life_after < life_before,
        "target must take damage: {life_after} vs {life_before}"
    );
    let actor = inst.actors.get(&arpg_core::EntityId(1)).unwrap();
    assert!(actor.action.is_none(), "cast action completed");
}

#[test]
fn use_skill_spawns_missile_that_hits() {
    let mut inst = setup();
    // missile-only skill: no direct damage
    let missile_skill = SkillDefinition {
        id: SkillId(2),
        targeting: TargetingSpec::Position,
        cost: CostFormula::default(),
        timing: TimingFormula::Instant,
        program: SkillProgram {
            ops: vec![SkillOp::SpawnMissile(1)],
        },
    };
    inst.register_skill(missile_skill).unwrap();
    inst.add_player(PlayerId(1), WorldPos::new(0, 0));
    inst.add_player(PlayerId(2), WorldPos::new(256, 0));
    inst.hostility.declare(PlayerId(1), PlayerId(2));
    inst.submit_command(skill_cmd(
        PlayerId(1),
        1,
        SkillId(2),
        Some(WorldPos::new(256, 0)),
    ));
    let life_before = inst.state.players.get(&PlayerId(2)).unwrap().life;
    for _ in 0..8 {
        inst.tick();
    }
    let life_after = inst.state.players.get(&PlayerId(2)).unwrap().life;
    assert!(life_after < life_before, "missile must hit the target");
    assert!(inst.missiles.is_empty(), "missile destroyed on hit");
}

#[test]
fn missile_expires_without_hit() {
    let mut inst = setup();
    let missile_skill = SkillDefinition {
        id: SkillId(2),
        targeting: TargetingSpec::Position,
        cost: CostFormula::default(),
        timing: TimingFormula::Instant,
        program: SkillProgram {
            ops: vec![SkillOp::SpawnMissile(1)],
        },
    };
    inst.register_skill(missile_skill).unwrap();
    inst.add_player(PlayerId(1), WorldPos::new(0, 0));
    // no second player: nothing to hit
    inst.submit_command(skill_cmd(
        PlayerId(1),
        1,
        SkillId(2),
        Some(WorldPos::new(4096, 0)),
    ));
    for _ in 0..30 {
        inst.tick();
    }
    assert!(inst.missiles.is_empty(), "missile must expire by lifetime");
}

#[test]
fn unregistered_skill_is_ignored() {
    let mut inst = setup();
    inst.add_player(PlayerId(1), WorldPos::new(0, 0));
    inst.add_player(PlayerId(2), WorldPos::new(256, 0));
    inst.hostility.declare(PlayerId(1), PlayerId(2));
    inst.submit_command(skill_cmd(
        PlayerId(1),
        1,
        SkillId(99),
        Some(WorldPos::new(256, 0)),
    ));
    for _ in 0..8 {
        inst.tick();
    }
    let actor = inst.actors.get(&arpg_core::EntityId(1)).unwrap();
    assert!(actor.action.is_none(), "no cast without a registered skill");
    assert_eq!(inst.state.players.get(&PlayerId(2)).unwrap().life, 100);
}

#[test]
fn skill_flow_is_deterministic() {
    let run = || {
        let mut inst = setup();
        inst.register_skill(fireball()).unwrap();
        inst.add_player(PlayerId(1), WorldPos::new(0, 0));
        inst.add_player(PlayerId(2), WorldPos::new(256, 0));
        inst.submit_command(skill_cmd(
            PlayerId(1),
            1,
            SkillId(1),
            Some(WorldPos::new(256, 0)),
        ));
        (0..12).map(|_| inst.tick().state_hash).collect::<Vec<_>>()
    };
    assert_eq!(run(), run());
}

use arpg_sim::GameInstance;

#[test]
fn secondary_effects_crit_and_leech_apply() {
    use arpg_sim::SecondaryProfile;
    let mut inst = setup();
    inst.register_skill(fireball()).unwrap();
    inst.add_player(PlayerId(1), WorldPos::new(0, 0));
    inst.secondary_profiles.insert(
        PlayerId(1),
        SecondaryProfile {
            critical_strike_bp: 10_000,
            life_leech_bp: 1000,
            ..Default::default()
        },
    );
    inst.apply_admin_command(&arpg_sim::admin::AdminCommand::SetStat {
        player: PlayerId(1),
        stat: arpg_sim::admin::AdminStat::Life,
        value: 50,
    })
    .unwrap();
    let monster = inst.spawn_monster_with_tc(WorldPos::new(256, 0), None);
    // give the monster enough life to survive the amplified hit
    if let Some(m) = inst.monsters.get_mut(&monster) {
        m.life = 500;
    }
    let m_life_before = inst.monsters.get(&monster).unwrap().life;
    let p_life_before = inst.state.players.get(&PlayerId(1)).unwrap().life;
    inst.submit_command(skill_cmd(
        PlayerId(1),
        1,
        SkillId(1),
        Some(WorldPos::new(256, 0)),
    ));
    for _ in 0..8 {
        inst.tick();
    }
    let m_life_after = inst.monsters.get(&monster).unwrap().life;
    let p_life_after = inst.state.players.get(&PlayerId(1)).unwrap().life;
    // crit doubles the 30-fire fireball; leech returns 10% of the hit
    assert!(
        m_life_before - m_life_after >= 60,
        "crit must amplify: {} -> {}",
        m_life_before,
        m_life_after
    );
    assert!(p_life_after > p_life_before, "life leech must heal");
}

#[test]
fn secondary_thorns_reflect_to_attacker() {
    use arpg_sim::SecondaryProfile;
    let mut inst = setup();
    inst.register_skill(fireball()).unwrap();
    inst.add_player(PlayerId(1), WorldPos::new(0, 0));
    inst.secondary_profiles.insert(
        PlayerId(1),
        SecondaryProfile {
            thorns_bp: 2500,
            ..Default::default()
        },
    );
    let monster = inst.spawn_monster(WorldPos::new(256, 0));
    let p_life_before = inst.state.players.get(&PlayerId(1)).unwrap().life;
    inst.submit_command(skill_cmd(
        PlayerId(1),
        1,
        SkillId(1),
        Some(WorldPos::new(256, 0)),
    ));
    for _ in 0..8 {
        inst.tick();
    }
    let p_life_after = inst.state.players.get(&PlayerId(1)).unwrap().life;
    assert!(p_life_after < p_life_before, "thorns must reflect damage");
    let _ = monster;
}

#[test]
fn secondary_knockback_pushes_monster_away() {
    use arpg_sim::SecondaryProfile;
    let mut inst = setup();
    inst.register_skill(fireball()).unwrap();
    inst.add_player(PlayerId(1), WorldPos::new(0, 0));
    inst.secondary_profiles.insert(
        PlayerId(1),
        SecondaryProfile {
            knockback_bp: 10_000,
            ..Default::default()
        },
    );
    let monster = inst.spawn_monster(WorldPos::new(256, 0));
    let pos_before = inst.monsters.get(&monster).unwrap().pos;
    inst.submit_command(skill_cmd(
        PlayerId(1),
        1,
        SkillId(1),
        Some(WorldPos::new(256, 0)),
    ));
    for _ in 0..8 {
        inst.tick();
    }
    let pos_after = inst.monsters.get(&monster).unwrap().pos;
    assert!(pos_after.x > pos_before.x, "knockback must push away");
}
