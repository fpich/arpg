use arpg_core::{EntityId, MonsterDefId, PlayerId, WorldPos};
use arpg_sim::{
    DifficultyId, GameInstance, QuestAccess, QuestAction, QuestDefId, QuestDefinition, QuestEvent,
    QuestObjective, QuestRule, QuestStatus, QuestTrigger, WaypointId,
};

#[test]
fn monster_kill_completes_quest_through_the_tick_pipeline() {
    let data = std::sync::Arc::new(arpg_data::GameData::default());
    let rules = std::sync::Arc::new(arpg_rules::GameRules::default());
    let mut inst = GameInstance::new(data, rules, [42u8; 32]);
    inst.add_player(PlayerId(1), WorldPos::new(0, 0));
    inst.add_player(PlayerId(2), WorldPos::new(1, 0));

    inst.quests.register(QuestDefinition {
        id: QuestDefId(1),
        name: "Den of Evil".into(),
        act: 1,
        access: QuestAccess::Party,
        objectives: vec![QuestObjective {
            id: 0,
            description: "Kill the boss".into(),
        }],
        rules: vec![QuestRule {
            trigger: QuestTrigger::MonsterKilled(MonsterDefId(0)),
            conditions: vec![],
            actions: vec![
                QuestAction::CompleteObjective(0),
                QuestAction::UnlockWaypoint(WaypointId(3)),
            ],
        }],
    });

    let monster = inst.spawn_monster(WorldPos::new(3, 0));
    let killer = EntityId(PlayerId(1).0 as u64);
    inst.apply_damage(monster, killer, 1000);
    inst.tick();

    assert_eq!(
        inst.quests.game_state(QuestDefId(1)).unwrap().status,
        QuestStatus::Completed
    );
    for p in [PlayerId(1), PlayerId(2)] {
        assert_eq!(
            inst.quests.character_state(p).quests.get(&QuestDefId(1)),
            Some(&QuestStatus::Completed)
        );
        assert!(inst
            .waypoints
            .is_unlocked(p, DifficultyId(0), WaypointId(3)));
    }
}

#[test]
fn quest_events_fire_once_per_tick_even_with_duplicate_evaluation() {
    let data = std::sync::Arc::new(arpg_data::GameData::default());
    let rules = std::sync::Arc::new(arpg_rules::GameRules::default());
    let mut inst = GameInstance::new(data, rules, [7u8; 32]);
    inst.add_player(PlayerId(1), WorldPos::new(0, 0));
    inst.quests.register(QuestDefinition {
        id: QuestDefId(2),
        name: "Counter".into(),
        act: 1,
        access: QuestAccess::Everyone,
        objectives: vec![QuestObjective {
            id: 0,
            description: "o".into(),
        }],
        rules: vec![QuestRule {
            trigger: QuestTrigger::CustomEvent(1),
            conditions: vec![],
            actions: vec![QuestAction::IncrementVariable("kills".into(), 1)],
        }],
    });
    let source = EntityId(PlayerId(1).0 as u64);
    inst.queue_quest_event(QuestTrigger::CustomEvent(1), source);
    inst.queue_quest_event(QuestTrigger::CustomEvent(1), source);
    inst.tick();
    assert_eq!(
        inst.quests
            .game_state(QuestDefId(2))
            .unwrap()
            .variables
            .get("kills"),
        Some(&1),
        "guarded by once-per-quest-per-tick"
    );
    // next tick fires again
    inst.queue_quest_event(QuestTrigger::CustomEvent(1), source);
    inst.tick();
    assert_eq!(
        inst.quests
            .game_state(QuestDefId(2))
            .unwrap()
            .variables
            .get("kills"),
        Some(&2)
    );
}

#[test]
fn quest_evaluation_is_deterministic() {
    let run = || {
        let data = std::sync::Arc::new(arpg_data::GameData::default());
        let rules = std::sync::Arc::new(arpg_rules::GameRules::default());
        let mut inst = GameInstance::new(data, rules, [9u8; 32]);
        inst.add_player(PlayerId(1), WorldPos::new(0, 0));
        inst.quests.register(QuestDefinition {
            id: QuestDefId(1),
            name: "q".into(),
            act: 1,
            access: QuestAccess::Party,
            objectives: vec![QuestObjective {
                id: 0,
                description: "o".into(),
            }],
            rules: vec![QuestRule {
                trigger: QuestTrigger::MonsterKilled(MonsterDefId(0)),
                conditions: vec![],
                actions: vec![QuestAction::CompleteObjective(0)],
            }],
        });
        let monster = inst.spawn_monster(WorldPos::new(3, 0));
        inst.apply_damage(monster, EntityId(1), 1000);
        let _ = QuestEvent {
            trigger: QuestTrigger::MonsterKilled(MonsterDefId(0)),
            owner: None,
            eligible: vec![],
        };
        inst.tick();
        inst.state.state_hash()
    };
    assert_eq!(run(), run());
}
