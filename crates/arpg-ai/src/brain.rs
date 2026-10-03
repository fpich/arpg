use crate::{AiAgent, AiIntent, AiParams, PerceivedEntity, PerceptionView};
use arpg_core::{EntityId, Tick, WorldPos};
use arpg_sim::ai::{AiBrain, AiCommand, AiWorldView};
use std::collections::BTreeMap;

/// Engine-facing AI brain (SPEC.md sections 59-63): holds one HFSM agent
/// per monster, thinks on the staggered schedule, and emits canonical
/// intents.
pub struct HfsmBrain {
    agents: BTreeMap<EntityId, AiAgent>,
    params: AiParams,
}

impl HfsmBrain {
    pub fn new(params: AiParams) -> HfsmBrain {
        HfsmBrain {
            agents: BTreeMap::new(),
            params,
        }
    }

    pub fn agent_count(&self) -> usize {
        self.agents.len()
    }
}

impl AiBrain for HfsmBrain {
    fn on_spawn(&mut self, entity: EntityId, home: WorldPos) {
        self.agents.insert(entity, AiAgent::new(entity, home));
    }

    fn think(&mut self, tick: Tick, view: &AiWorldView) -> Vec<(EntityId, AiCommand)> {
        let mut commands = Vec::new();
        for (entity, agent) in self.agents.iter_mut() {
            if !agent.should_think(tick, self.params.think_interval_ticks) {
                continue;
            }
            // monsters see players as enemies
            let enemies: Vec<PerceivedEntity> = view
                .players
                .iter()
                .map(|(e, pos)| PerceivedEntity {
                    entity: *e,
                    pos: *pos,
                })
                .collect();
            let perception = PerceptionView { enemies };
            let current_pos = view
                .monsters
                .iter()
                .find(|(e, _)| *e == *entity)
                .map(|(_, p)| *p)
                .unwrap_or(agent.blackboard.home_position);
            let intent = agent.think(tick, current_pos, &perception, &self.params);
            let command = match intent {
                AiIntent::MoveTo(pos) => AiCommand::MoveTo(pos),
                AiIntent::AttackTarget(t) => AiCommand::Attack(t),
                AiIntent::None => continue,
            };
            commands.push((*entity, command));
        }
        commands
    }
}
