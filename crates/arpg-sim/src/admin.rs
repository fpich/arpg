//! Administrative debug commands (SPEC.md section 169). These are test and
//! operations tools: they mutate the game state directly and can be recorded
//! in replays of test games so that admin-assisted sessions stay
//! reproducible. They are never exposed on the untrusted client path
//! (section 170).

use crate::item::{ItemInstance, ItemQuality};
use crate::quest::QuestDefId;
use crate::state::GameInstance;
use arpg_core::{EntityId, ItemDefId, ItemId, PlayerId, SkillId, WorldPos};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdminCommand {
    Spawn {
        def: u32,
        pos: WorldPos,
    },
    GiveItem {
        player: PlayerId,
        def: ItemDefId,
    },
    GiveSkill {
        player: PlayerId,
        skill: SkillId,
    },
    Teleport {
        player: PlayerId,
        pos: WorldPos,
    },
    SetStat {
        player: PlayerId,
        stat: AdminStat,
        value: i64,
    },
    Kill {
        entity: EntityId,
    },
    CompleteQuest {
        quest: QuestDefId,
    },
    DumpEntity {
        entity: EntityId,
    },
    DumpRng,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdminStat {
    Life,
    Mana,
    Level,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdminOutcome {
    Spawned(EntityId),
    ItemGiven(ItemId),
    SkillGiven(SkillId),
    Teleported(WorldPos),
    StatSet,
    Killed,
    QuestCompleted,
    EntityDump { pos: WorldPos, life: i64 },
    RngDump { seed: [u8; 32], tick: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AdminError {
    #[error("unknown player")]
    UnknownPlayer,
    #[error("unknown entity")]
    UnknownEntity,
    #[error("unknown quest")]
    UnknownQuest,
    #[error("unknown skill")]
    UnknownSkill,
    #[error("unknown item definition")]
    UnknownItem,
}

impl GameInstance {
    /// Apply an admin command directly. Deterministic: identical inputs on
    /// identical states produce identical outcomes, so replaying an
    /// admin-assisted session reproduces the same state hashes. Stats fixed
    /// at spawn are never retroactively changed (section 112).
    pub fn apply_admin_command(&mut self, cmd: &AdminCommand) -> Result<AdminOutcome, AdminError> {
        match cmd {
            AdminCommand::Spawn { def, pos } => {
                let entity = self.spawn_monster_def(arpg_core::MonsterDefId(*def), *pos);
                Ok(AdminOutcome::Spawned(entity))
            }
            AdminCommand::GiveItem { player, def } => {
                if !self.state.players.contains_key(player) {
                    return Err(AdminError::UnknownPlayer);
                }
                if !self.data.items.contains_key(&def.0) {
                    return Err(AdminError::UnknownItem);
                }
                let id = self.mint_admin_item_id();
                let item = ItemInstance {
                    id,
                    definition: *def,
                    quality: ItemQuality::Normal,
                    item_level: 1,
                    generation_seed: crate::state::derive_drop_seed(
                        self.state.seed(),
                        EntityId(id.0 as u64),
                        self.state.tick,
                    ),
                    affixes: smallvec::SmallVec::new(),
                    sockets: smallvec::SmallVec::new(),
                    durability: None,
                    flags: 0,
                    charges: None,
                    hands: Default::default(),
                };
                let pos = self
                    .state
                    .players
                    .get(player)
                    .map(|p| p.pos)
                    .unwrap_or(WorldPos::new(0, 0));
                self.inventory
                    .spawn_ground(item, arpg_core::LevelInstanceId(0), pos);
                if let Some(metrics) = &self.metrics {
                    metrics
                        .lock()
                        .unwrap()
                        .incr(arpg_metrics::names::ITEM_GENERATED);
                }
                Ok(AdminOutcome::ItemGiven(id))
            }
            AdminCommand::GiveSkill { player, skill } => {
                if !self.state.players.contains_key(player) {
                    return Err(AdminError::UnknownPlayer);
                }
                if !self.skills.contains_key(skill) {
                    return Err(AdminError::UnknownSkill);
                }
                Ok(AdminOutcome::SkillGiven(*skill))
            }
            AdminCommand::Teleport { player, pos } => {
                let Some(p) = self.state.players.get_mut(player) else {
                    return Err(AdminError::UnknownPlayer);
                };
                p.pos = *pos;
                Ok(AdminOutcome::Teleported(*pos))
            }
            AdminCommand::SetStat {
                player,
                stat,
                value,
            } => {
                let Some(p) = self.state.players.get_mut(player) else {
                    return Err(AdminError::UnknownPlayer);
                };
                match stat {
                    AdminStat::Life => p.life = *value,
                    AdminStat::Mana => p.mana = *value,
                    AdminStat::Level => p.level = *value,
                }
                Ok(AdminOutcome::StatSet)
            }
            AdminCommand::Kill { entity } => {
                let Some(m) = self.monsters.get(entity) else {
                    return Err(AdminError::UnknownEntity);
                };
                let life = m.life.max(1);
                // route through the standard damage path so PendingDeath,
                // kill credit and loot all resolve normally (section 13)
                self.apply_damage(*entity, *entity, life);
                Ok(AdminOutcome::Killed)
            }
            AdminCommand::CompleteQuest { quest } => {
                let Some(state) = self.quests.game_states.get_mut(quest) else {
                    return Err(AdminError::UnknownQuest);
                };
                state.status = crate::quest::QuestStatus::Completed;
                Ok(AdminOutcome::QuestCompleted)
            }
            AdminCommand::DumpEntity { entity } => {
                if let Some(m) = self.monsters.get(entity) {
                    return Ok(AdminOutcome::EntityDump {
                        pos: m.pos,
                        life: m.life,
                    });
                }
                let player = PlayerId(entity.0 as u32);
                if let Some(p) = self.state.players.get(&player) {
                    return Ok(AdminOutcome::EntityDump {
                        pos: p.pos,
                        life: p.life,
                    });
                }
                Err(AdminError::UnknownEntity)
            }
            AdminCommand::DumpRng => Ok(AdminOutcome::RngDump {
                seed: self.state.seed(),
                tick: self.state.tick.0,
            }),
        }
    }

    /// Deterministic fresh item id: one above the highest known id, so the
    /// same admin sequence always mints the same ids in replay.
    fn mint_admin_item_id(&self) -> ItemId {
        ItemId(self.inventory.highest_item_id() + 1)
    }
}
