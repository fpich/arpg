//! Fuzz corpora for the remaining section 183 targets: inventory
//! operations, recipe matcher, skill IR validator, quest validator. Each
//! entry must produce an error or a valid result, never a panic.

use crate::FuzzRng;
use arpg_core::{ItemDefId, MonsterDefId, PlayerId, WorldPos};
use arpg_sim::economy::transmute;
use arpg_sim::{
    CostFormula, CubeRecipe, EquipmentSlot, GridPos, ItemInstance, ItemLocation, ItemQuality,
    QuestAccess, QuestAction, QuestCondition, QuestDefinition, QuestObjective, QuestRule,
    QuestTrigger, SkillDefinition, SkillOp, SkillProgram, StashPos, TargetingSpec, TimingFormula,
    TreasureClass, TreasureKind, WeightedTreasureEntry,
};

/// Fuzz the inventory transaction path with hostile move combinations.
pub fn fuzz_inventory(seed: u64, iterations: u32) -> Result<(), arpg_sim::ItemError> {
    let mut rng = FuzzRng(seed);
    let mut inv = arpg_sim::InventorySystem::new();
    let player = PlayerId(1);
    // seed the inventory with a few items
    for i in 0..5u32 {
        let item = ItemInstance {
            id: arpg_core::ItemId(i as u128),
            definition: ItemDefId(7),
            quality: ItemQuality::Normal,
            item_level: 10,
            generation_seed: [i as u8; 32],
            affixes: Default::default(),
            sockets: Default::default(),
            durability: Some(50),
            flags: 0,
        };
        inv.spawn_ground(
            item,
            arpg_core::LevelInstanceId(0),
            WorldPos::new(i as i32, 0),
        );
    }
    for _ in 0..iterations {
        let item = arpg_core::ItemId(rng.below(8) as u128);
        let x = rng.below(20) as u8;
        let y = rng.below(20) as u8;
        let to = match rng.below(6) {
            0 => ItemLocation::PlayerInventory(player, GridPos { x, y }),
            1 => ItemLocation::Equipment(player, EquipmentSlot::PrimarySet),
            2 => ItemLocation::Belt(player, rng.below(8) as u8),
            3 => ItemLocation::Stash(
                player,
                StashPos {
                    page: rng.below(3) as u8,
                    x: x % 10,
                    y: y % 10,
                },
            ),
            4 => ItemLocation::Cube(player, GridPos { x: x % 4, y: y % 4 }),
            _ => ItemLocation::Ground(
                arpg_core::LevelInstanceId(rng.below(4)),
                WorldPos::new(rng.below(100) as i32, rng.below(100) as i32),
            ),
        };
        let from = match rng.below(3) {
            0 => ItemLocation::Ground(arpg_core::LevelInstanceId(0), WorldPos::new(0, 0)),
            1 => ItemLocation::PlayerInventory(player, GridPos { x: 0, y: 0 }),
            _ => to,
        };
        // every error is fine; a panic is not
        let _ = inv.move_item(item, from, to);
    }
    Ok(())
}

/// Fuzz the cube recipe matcher with hostile input sets.
pub fn fuzz_recipes(seed: u64, iterations: u32) -> Result<CubeRecipe, arpg_sim::CubeError> {
    let mut rng = FuzzRng(seed);
    let recipes = vec![
        CubeRecipe {
            inputs: vec![ItemDefId(1), ItemDefId(2)],
            output: ItemDefId(50),
            output_quality: ItemQuality::Crafted,
        },
        CubeRecipe {
            inputs: vec![ItemDefId(9)],
            output: ItemDefId(51),
            output_quality: ItemQuality::Magic,
        },
    ];
    for _ in 0..iterations {
        let n = rng.below(5) as usize;
        let inputs: Vec<ItemDefId> = (0..n).map(|_| ItemDefId(rng.below(60) as u32)).collect();
        let _ = transmute(&recipes, &inputs);
    }
    transmute(&recipes, &[ItemDefId(1), ItemDefId(2)])
}

/// Fuzz the skill IR validator with hostile programs.
pub fn fuzz_skill_ir(seed: u64, iterations: u32) {
    let mut rng = FuzzRng(seed);
    for _ in 0..iterations {
        let def = SkillDefinition {
            id: arpg_core::SkillId(rng.below(1000) as u32),
            targeting: TargetingSpec::NoTarget,
            cost: CostFormula {
                mana_cost: (rng.below(1000) as i64) - 500,
                life_cost: (rng.below(100) as i64) - 50,
            },
            timing: TimingFormula::Instant,
            program: SkillProgram {
                ops: vec![SkillOp::Heal(1)],
            },
        };
        // either valid or a clean validation error, never a panic
        let _ = def.validate();
    }
}

/// Fuzz the treasure class validator with hostile classes.
pub fn fuzz_treasure(seed: u64, iterations: u32) {
    let mut rng = FuzzRng(seed);
    for _ in 0..iterations {
        let entries = (0..rng.below(4))
            .map(|_| WeightedTreasureEntry {
                weight: rng.below(10) as u32,
                kind: TreasureKind::Item(ItemDefId(rng.below(50) as u32)),
            })
            .collect();
        let tc = TreasureClass {
            picks: (rng.below(10) as i16) - 5,
            no_drop_weight: rng.below(100) as u32,
            entries,
        };
        let mut roller = arpg_sim::LootRoller::new();
        // invalid classes produce validation errors; valid ones roll
        let _ = roller.roll(&tc, [rng.gen() as u8; 32], rng.below(60) as u16);
    }
}

/// Fuzz the quest evaluator with hostile definitions.
pub fn fuzz_quests(seed: u64, iterations: u32) {
    let mut rng = FuzzRng(seed);
    for _ in 0..iterations {
        let mut system = arpg_sim::QuestSystem::new();
        let def = QuestDefinition {
            id: arpg_sim::QuestDefId(rng.below(10) as u32),
            name: "fuzz".into(),
            act: rng.below(6) as u32,
            access: QuestAccess::Party,
            objectives: vec![QuestObjective {
                id: 0,
                description: "o".into(),
            }],
            rules: vec![QuestRule {
                trigger: QuestTrigger::MonsterKilled(MonsterDefId(rng.below(50) as u32)),
                conditions: vec![
                    QuestCondition::VariableAtLeast("v".into(), rng.below(100) as i64),
                    QuestCondition::HasFlag("f".into()),
                ],
                actions: vec![
                    QuestAction::IncrementVariable("v".into(), rng.below(5) as i64),
                    QuestAction::CompleteObjective(0),
                ],
            }],
        };
        system.register(def);
        let event = arpg_sim::QuestEvent {
            trigger: QuestTrigger::MonsterKilled(MonsterDefId(rng.below(50) as u32)),
            owner: Some(PlayerId(1)),
            eligible: vec![PlayerId(1), PlayerId(2)],
        };
        let _ = system.evaluate(&event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arpg_core::Tick;
    use arpg_sim::{CommandEnvelope, GameInstance};

    #[test]
    fn inventory_fuzz_never_panics() {
        for seed in 1..20 {
            fuzz_inventory(seed, 500).unwrap();
        }
    }

    #[test]
    fn recipe_fuzz_never_panics() {
        for seed in 1..20 {
            let r = fuzz_recipes(seed, 500).unwrap();
            assert_eq!(r.output, ItemDefId(50));
        }
    }

    #[test]
    fn skill_ir_fuzz_never_panics() {
        fuzz_skill_ir(1234, 2000);
    }

    #[test]
    fn treasure_fuzz_never_panics() {
        fuzz_treasure(777, 2000);
    }

    #[test]
    fn quest_fuzz_never_panics() {
        fuzz_quests(999, 2000);
    }

    #[test]
    fn fuzzed_game_with_many_players_stays_deterministic() {
        let run = || {
            let data = std::sync::Arc::new(arpg_data::GameData::default());
            let rules = std::sync::Arc::new(arpg_rules::GameRules::default());
            let mut inst = GameInstance::new(data, rules, [5u8; 32]);
            for p in 1..=8 {
                inst.add_player(PlayerId(p), WorldPos::new(p as i32, 0));
                inst.spawn_monster(WorldPos::new(p as i32 + 10, 0));
            }
            let mut rng = FuzzRng(31337);
            let mut hash = [0u8; 32];
            for _ in 0..50 {
                for p in 1..=8u32 {
                    if rng.below(3) == 0 {
                        let seq = (rng.below(100) + 1) as u32;
                        let env = CommandEnvelope {
                            sequence: seq,
                            client_tick: Tick(0),
                            player: PlayerId(p),
                            command: arpg_sim::ClientCommand::Move(arpg_sim::MoveIntent {
                                direction: WorldPos::new(
                                    rng.below(10) as i32,
                                    rng.below(10) as i32,
                                ),
                                movement_mode: arpg_sim::MovementMode::Run,
                                sequence: seq,
                            }),
                        };
                        inst.submit_command(env);
                    }
                }
                hash = inst.tick().state_hash;
            }
            hash
        };
        assert_eq!(run(), run());
    }
}
