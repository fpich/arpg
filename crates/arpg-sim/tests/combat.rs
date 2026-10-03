use arpg_core::{EntityId, FixedVec2, SkillId, WorldPos};
use arpg_sim::skill::execute_program;
use arpg_sim::UseSkillIntent;
use arpg_sim::{
    CostFormula, DamagePacket, DamageRange, DotAccumulator, MissileInstance, MissileMovement,
    Resistances, RollAmounts, SkillDefinition, SkillOp, SkillProgram, TargetingSpec, TimingFormula,
};

#[test]
fn resistances_reduce_and_immunity_nullifies() {
    let roll = RollAmounts {
        physical: 100,
        magic: 100,
        fire: 100,
        cold: 100,
        lightning: 100,
    };
    let resists = Resistances {
        poison_bp: 0,
        physical_bp: 5000,
        magic_bp: 0,
        fire_bp: 10_000,
        cold_bp: -5000,
        lightning_bp: 7500,
    };
    let total = arpg_sim::damage::resolve_damage(roll, &resists);
    // physical 50, magic 100, fire immune 0, cold amplified 150, lightning 25
    assert_eq!(total, 325);
}

#[test]
fn dot_accumulator_pays_exact_total() {
    let mut dot = DotAccumulator::new(1000, 3);
    let t1 = dot.tick_amount();
    let t2 = dot.tick_amount();
    let t3 = dot.tick_amount();
    assert_eq!(t1 + t2 + t3, 1000);
    assert!(t1 > 0 && t3 > 0);
    assert_eq!(dot.tick_amount(), 0);
}

#[test]
fn dot_accumulator_handles_non_divisible_totals() {
    let mut dot = DotAccumulator::new(100, 3);
    let mut paid = 0;
    for _ in 0..3 {
        paid += dot.tick_amount();
    }
    assert_eq!(paid, 100);
}

#[test]
fn invalid_skills_fail_validation() {
    let mk = |ops| SkillDefinition {
        id: SkillId(1),
        targeting: TargetingSpec::NoTarget,
        cost: CostFormula {
            mana_cost: -5,
            life_cost: 0,
        },
        timing: TimingFormula::Instant,
        program: SkillProgram { ops },
    };
    let negative = mk(vec![SkillOp::NoOp]);
    assert!(negative.validate().is_err());

    let deep = SkillDefinition {
        cost: CostFormula::default(),
        ..mk(vec![SkillOp::NoOp])
    };
    assert!(deep.validate().is_ok());

    // nesting beyond the bound fails
    let mut ops = vec![SkillOp::NoOp];
    for _ in 0..20 {
        ops = vec![SkillOp::Sequence(ops)];
    }
    let too_deep = SkillDefinition {
        cost: CostFormula::default(),
        ..mk(ops)
    };
    assert!(too_deep.validate().is_err());
}

#[test]
fn program_execution_sums_outcomes() {
    let def = SkillDefinition {
        id: SkillId(1),
        targeting: TargetingSpec::Position,
        cost: CostFormula::default(),
        timing: TimingFormula::Instant,
        program: SkillProgram {
            ops: vec![
                SkillOp::DealDamage(DamagePacket {
                    physical: DamageRange::new(10, 20),
                    ..DamagePacket::default()
                }),
                SkillOp::Heal(5),
                SkillOp::Sequence(vec![SkillOp::RestoreMana(3), SkillOp::Knockback(2)]),
                SkillOp::SpawnMissile(7),
            ],
        },
    };
    def.validate().unwrap();
    let intent = UseSkillIntent {
        skill: SkillId(1),
        target: Some(WorldPos::new(256, 256)),
    };
    let mut missiles = Vec::new();
    let outcome = execute_program(&def.program, &intent, |m| missiles.push(m));
    assert_eq!(outcome.damage, 15);
    assert_eq!(outcome.heal, 5);
    assert_eq!(outcome.mana_restored, 3);
    assert_eq!(outcome.knockback, 2);
    assert_eq!(missiles, vec![7]);
}

#[test]
fn missile_moves_linearly_and_expires() {
    let mut missile = MissileInstance {
        entity: EntityId(100),
        definition: 1,
        owner: EntityId(1),
        source_skill: Some(SkillId(1)),
        position: WorldPos::new(0, 0),
        velocity: FixedVec2 { x: 256, y: 0 },
        lifetime: 3,
        movement: MissileMovement::Linear,
        hit_entities: Vec::new(),
        remaining_pierces: 0,
    };
    assert!(!missile.advance());
    assert_eq!(missile.position, WorldPos::new(256, 0));
    assert!(!missile.advance());
    assert_eq!(missile.position, WorldPos::new(512, 0));
    assert!(missile.advance());
    assert_eq!(missile.position, WorldPos::new(768, 0));
}

#[test]
fn missile_pierces_until_exhausted() {
    let mut missile = MissileInstance {
        entity: EntityId(100),
        definition: 1,
        owner: EntityId(1),
        source_skill: None,
        position: WorldPos::new(0, 0),
        velocity: FixedVec2::ZERO,
        lifetime: 10,
        movement: MissileMovement::Linear,
        hit_entities: Vec::new(),
        remaining_pierces: 1,
    };
    assert!(!missile.register_hit(EntityId(2)));
    assert!(
        !missile.register_hit(EntityId(2)),
        "same entity never twice"
    );
    assert!(missile.register_hit(EntityId(3)), "pierces exhausted");
}

#[test]
fn missile_hits_at_radius() {
    let missile = MissileInstance {
        entity: EntityId(100),
        definition: 1,
        owner: EntityId(1),
        source_skill: None,
        position: WorldPos::new(0, 0),
        velocity: FixedVec2::ZERO,
        lifetime: 10,
        movement: MissileMovement::Linear,
        hit_entities: Vec::new(),
        remaining_pierces: 0,
    };
    assert!(missile.hits_at(WorldPos::new(100, 0), 128));
    assert!(!missile.hits_at(WorldPos::new(200, 0), 128));
}
