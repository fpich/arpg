//! Datapack skill wiring: class signature skills and shared attacks
//! translate into sim IR and execute (SPEC.md sections 35-37, 151, 193).

use arpg_core::{PlayerId, SkillId, WorldPos};
use arpg_sim::{ClientCommand, CommandEnvelope, GameInstance, UseSkillIntent};
use std::sync::Arc;

fn instance() -> GameInstance {
    let data = Arc::new(arpg_data::datapack::compile_reference_datapack());
    let rules = Arc::new(arpg_rules::GameRules::default());
    let mut inst = GameInstance::new(data, rules, [21u8; 32]);
    inst.register_datapack_skills().unwrap();
    inst
}

fn skill_cmd(player: PlayerId, seq: u32, skill: SkillId) -> CommandEnvelope {
    CommandEnvelope {
        sequence: seq,
        client_tick: arpg_core::Tick(0),
        player,
        command: ClientCommand::UseSkill(UseSkillIntent {
            skill,
            target: Some(WorldPos::new(5, 0)),
        }),
    }
}

#[test]
fn all_class_skills_register_from_the_datapack() {
    let inst = instance();
    // 7 class signature skills + 3 shared attacks
    for class_skill in (100..=600).step_by(100) {
        assert!(
            inst.skills.contains_key(&SkillId(class_skill)),
            "class skill {class_skill}"
        );
    }
    assert!(inst.skills.contains_key(&SkillId(1)));
    assert!(inst.skills.contains_key(&SkillId(2)));
    assert!(inst.skills.contains_key(&SkillId(3)));
}

#[test]
fn barbarian_signature_skill_deals_damage() {
    let mut inst = instance();
    inst.add_player(PlayerId(1), WorldPos::ZERO);
    let monster = inst.spawn_monster(WorldPos::new(5, 0));
    let life_before = inst.monsters.get(&monster).unwrap().life;
    inst.submit_command(skill_cmd(PlayerId(1), 1, SkillId(400)));
    for _ in 0..10 {
        inst.tick();
    }
    let life_after = inst.monsters.get(&monster);
    assert!(
        life_after.is_none() || life_after.unwrap().life < life_before,
        "Bash (25 physical) must damage the monster"
    );
}

#[test]
fn sorceress_fireball_spawns_a_missile() {
    let mut inst = instance();
    inst.add_player(PlayerId(1), WorldPos::ZERO);
    inst.submit_command(skill_cmd(PlayerId(1), 1, SkillId(600)));
    for _ in 0..5 {
        inst.tick();
    }
    assert!(
        !inst.missiles.is_empty(),
        "Fireball spawns a missile at cast completion"
    );
}

#[test]
fn mana_cost_is_enforced() {
    let mut inst = instance();
    inst.add_player(PlayerId(1), WorldPos::ZERO);
    // drain mana first
    {
        let p = inst.state.players.get_mut(&PlayerId(1)).unwrap();
        p.mana = 1;
    }
    let mana_before = inst.state.players.get(&PlayerId(1)).unwrap().mana;
    inst.submit_command(skill_cmd(PlayerId(1), 1, SkillId(600)));
    for _ in 0..5 {
        inst.tick();
    }
    let mana_after = inst.state.players.get(&PlayerId(1)).unwrap().mana;
    assert_eq!(mana_before, mana_after, "not enough mana: cast refused");
}

#[test]
fn datapack_skills_are_deterministic() {
    let run = || {
        let mut inst = instance();
        inst.add_player(PlayerId(1), WorldPos::ZERO);
        let m = inst.spawn_monster(WorldPos::new(5, 0));
        inst.submit_command(skill_cmd(PlayerId(1), 1, SkillId(400)));
        let mut hash = [0u8; 32];
        for _ in 0..10 {
            hash = inst.tick().state_hash;
        }
        (hash, inst.monsters.get(&m).map(|x| x.life))
    };
    assert_eq!(run(), run());
}
#[test]
fn native_effect_budget_respected() {
    let inst = instance();
    let total = inst.skills.len();
    assert!(total > 0, "the datapack must register skills");
    let native = inst
        .skills
        .values()
        .filter(|def| {
            def.program
                .ops
                .iter()
                .any(|op| matches!(op, arpg_sim::SkillOp::Native(_)))
        })
        .count();
    let without = total - native;
    assert!(
        without * 10 >= total * 9,
        "SPEC.md section 38: at least 90% of skills must avoid native effects, got {native}/{total} natives"
    );
}
