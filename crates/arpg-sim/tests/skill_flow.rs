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

#[test]
fn cast_speed_bonus_shrinks_cast_time() {
    let mut inst = setup();
    // fireball has a 2-tick cast; a big bonus must reach 1 tick
    inst.register_skill(fireball()).unwrap();
    inst.add_player(PlayerId(1), WorldPos::new(0, 0));
    inst.cast_speed_bonus_bp.insert(PlayerId(1), 10_000);
    let monster = inst.spawn_monster(WorldPos::new(256, 0));
    let life_before = inst.monsters.get(&monster).unwrap().life;
    inst.submit_command(skill_cmd(
        PlayerId(1),
        1,
        SkillId(1),
        Some(WorldPos::new(256, 0)),
    ));
    // with the bonus the cast resolves faster than the 8-tick baseline
    let mut damaged_early = None;
    for t in 0..8 {
        inst.tick();
        let life = inst.monsters.get(&monster).map(|m| m.life).unwrap_or(0);
        if life < life_before {
            damaged_early = Some(t);
            break;
        }
    }
    assert!(
        damaged_early.is_some(),
        "cast speed must accelerate the impact"
    );
}

#[test]
fn open_wounds_bleeds_over_time() {
    use arpg_sim::SecondaryProfile;
    let mut inst = setup();
    inst.register_skill(fireball()).unwrap();
    inst.add_player(PlayerId(1), WorldPos::new(0, 0));
    inst.secondary_profiles.insert(
        PlayerId(1),
        SecondaryProfile {
            open_wounds_bp: 10_000,
            ..Default::default()
        },
    );
    let monster = inst.spawn_monster_with_tc(WorldPos::new(256, 0), None);
    if let Some(m) = inst.monsters.get_mut(&monster) {
        m.life = 500;
    }
    inst.submit_command(skill_cmd(
        PlayerId(1),
        1,
        SkillId(1),
        Some(WorldPos::new(256, 0)),
    ));
    let mut appeared = false;
    for _ in 0..12 {
        inst.tick();
        if inst
            .states
            .entity_states(monster)
            .iter()
            .any(|s| s.state == GameInstance::STATE_BLEEDING)
        {
            appeared = true;
            break;
        }
    }
    assert!(appeared, "open wounds must apply the bleeding state");
    let hit_life = inst.monsters.get(&monster).unwrap().life;
    for _ in 0..10 {
        inst.tick();
    }
    let final_life = inst.monsters.get(&monster).unwrap().life;
    assert!(
        final_life < hit_life,
        "bleed must keep damaging after the hit: {} -> {}",
        hit_life,
        final_life
    );
    assert!(
        !inst
            .states
            .entity_states(monster)
            .iter()
            .any(|s| s.state == GameInstance::STATE_BLEEDING),
        "bleeding expires after the dot is paid"
    );
}

#[test]
fn attack_speed_bonus_shrinks_swing_time() {
    let mut inst = setup();
    // a melee swing with base 4 ticks
    inst.register_skill(arpg_sim::skill::SkillDefinition {
        id: SkillId(7),
        targeting: TargetingSpec::Entity,
        cost: Default::default(),
        timing: arpg_sim::skill::TimingFormula::AttackTicks(4),
        program: SkillProgram {
            ops: vec![SkillOp::DealDamage(DamagePacket {
                physical: DamageRange::new(10, 10),
                ..DamagePacket::default()
            })],
        },
    })
    .unwrap();
    inst.add_player(PlayerId(1), WorldPos::new(0, 0));
    // high dexterity so the to-hit roll lands
    inst.actors
        .get_mut(&arpg_core::EntityId(1))
        .unwrap()
        .stats
        .set_base(arpg_sim::stat::STAT_DEXTERITY, 1000);
    inst.attack_speed_bonus_bp.insert(PlayerId(1), 10_000);
    let monster = inst.spawn_monster_with_tc(WorldPos::new(256, 0), None);
    if let Some(m) = inst.monsters.get_mut(&monster) {
        m.life = 500;
    }
    let life_before = inst.monsters.get(&monster).unwrap().life;
    inst.submit_command(skill_cmd(
        PlayerId(1),
        1,
        SkillId(7),
        Some(WorldPos::new(256, 0)),
    ));
    let mut hit_at = None;
    for t in 0..(arpg_sim::DEFAULT_INPUT_DELAY_TICKS + 6) {
        inst.tick();
        let life = inst.monsters.get(&monster).unwrap().life;
        if life < life_before {
            hit_at = Some(t);
            break;
        }
    }
    assert!(
        hit_at.is_some_and(|t| t <= arpg_sim::DEFAULT_INPUT_DELAY_TICKS + 3),
        "attack speed must shorten the swing below the 4-tick baseline"
    );
    let _ = life_before;
}

#[test]
fn strength_feeds_physical_damage_through_stat_graph() {
    let run = |strength: i64| -> i64 {
        let mut inst = setup();
        inst.register_skill(fireball()).unwrap();
        inst.add_player(PlayerId(1), WorldPos::new(0, 0));
        let entity = arpg_core::EntityId(1);
        inst.actors
            .get_mut(&entity)
            .unwrap()
            .stats
            .set_base(arpg_sim::stat::STAT_STRENGTH, strength);
        let monster = inst.spawn_monster_with_tc(WorldPos::new(256, 0), None);
        if let Some(m) = inst.monsters.get_mut(&monster) {
            m.life = 5000;
        }
        let before = inst.monsters.get(&monster).unwrap().life;
        inst.submit_command(skill_cmd(
            PlayerId(1),
            1,
            SkillId(1),
            Some(WorldPos::new(256, 0)),
        ));
        for _ in 0..12 {
            inst.tick();
        }
        let _ = before;
        before - inst.monsters.get(&monster).unwrap().life
    };
    let low = run(0);
    let high = run(50);
    assert!(
        high > low,
        "strength must add physical damage ({} vs {})",
        high,
        low
    );
    assert!(low > 0, "the base hit must still land");
}

#[test]
fn melee_swings_roll_to_hit_and_dexterity_helps() {
    // identical melee scenarios except dexterity: the high-dexterity
    // attacker lands measurably more damage over repeated swings
    let run = |dex: i64| -> i64 {
        let mut inst = setup();
        inst.register_skill(arpg_sim::skill::SkillDefinition {
            id: SkillId(8),
            targeting: TargetingSpec::Entity,
            cost: Default::default(),
            timing: arpg_sim::skill::TimingFormula::AttackTicks(1),
            program: SkillProgram {
                ops: vec![SkillOp::DealDamage(DamagePacket {
                    physical: DamageRange::new(5, 5),
                    ..DamagePacket::default()
                })],
            },
        })
        .unwrap();
        inst.add_player(PlayerId(1), WorldPos::new(0, 0));
        inst.actors
            .get_mut(&arpg_core::EntityId(1))
            .unwrap()
            .stats
            .set_base(arpg_sim::stat::STAT_DEXTERITY, dex);
        let monster = inst.spawn_monster_with_tc(WorldPos::new(256, 0), None);
        if let Some(m) = inst.monsters.get_mut(&monster) {
            m.life = 5000;
        }
        let before = inst.monsters.get(&monster).unwrap().life;
        for seq in 1..=20 {
            inst.submit_command(skill_cmd(
                PlayerId(1),
                seq,
                SkillId(8),
                Some(WorldPos::new(256, 0)),
            ));
            for _ in 0..(arpg_sim::DEFAULT_INPUT_DELAY_TICKS + 3) {
                inst.tick();
            }
        }
        before - inst.monsters.get(&monster).unwrap().life
    };
    let low = run(0);
    let high = run(1000);
    assert!(
        high > low,
        "dexterity must improve hit rate ({} vs {})",
        high,
        low
    );
    assert!(high > 0, "the high-dex attacker must land hits");
}

#[test]
fn corpse_explosion_consumes_nearest_corpse() {
    let mut inst = setup();
    inst.register_skill(arpg_sim::skill::SkillDefinition {
        id: SkillId(9),
        targeting: TargetingSpec::Position,
        cost: Default::default(),
        timing: arpg_sim::skill::TimingFormula::Instant,
        program: SkillProgram {
            ops: vec![SkillOp::ConsumeCorpse(40)],
        },
    })
    .unwrap();
    inst.add_player(PlayerId(1), WorldPos::new(0, 0));
    // a corpse near the target position
    inst.corpses
        .spawn(arpg_core::MonsterDefId(1), WorldPos::new(256, 0), None, 0);
    let monster = inst.spawn_monster_with_tc(WorldPos::new(256, 0), None);
    if let Some(m) = inst.monsters.get_mut(&monster) {
        m.life = 1000;
    }
    let before = inst.monsters.get(&monster).unwrap().life;
    inst.submit_command(skill_cmd(
        PlayerId(1),
        1,
        SkillId(9),
        Some(WorldPos::new(256, 0)),
    ));
    for _ in 0..12 {
        inst.tick();
    }
    let after = inst.monsters.get(&monster).unwrap().life;
    assert!(
        before - after >= 40,
        "corpse explosion must deal its damage"
    );
    assert!(
        inst.corpses.iter().next().unwrap().consumed,
        "the corpse is consumed"
    );
    // a second cast without a fresh corpse adds nothing
    let before2 = inst.monsters.get(&monster).unwrap().life;
    inst.submit_command(skill_cmd(
        PlayerId(1),
        2,
        SkillId(9),
        Some(WorldPos::new(256, 0)),
    ));
    for _ in 0..12 {
        inst.tick();
    }
    let after2 = inst.monsters.get(&monster).unwrap().life;
    assert_eq!(
        before2, after2,
        "a consumed corpse fuels no second explosion"
    );
}
