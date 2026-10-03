use arpg_core::EntityId;

/// Pack roles (SPEC.md section 64).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackRole {
    PackLeader,
    Minion,
    ChampionPack,
    UniquePack,
    BossEncounter,
}

/// A monster pack: leader, members, formation, aggro linkage (SPEC.md
/// section 64).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonsterPack {
    pub leader: EntityId,
    pub members: Vec<EntityId>,
    pub formation: FormationPolicy,
    pub aggro_linkage: bool,
}

impl MonsterPack {
    pub fn new(leader: EntityId, members: Vec<EntityId>) -> MonsterPack {
        MonsterPack {
            leader,
            members,
            formation: FormationPolicy::Loose,
            aggro_linkage: true,
        }
    }

    /// With aggro linkage, any member's aggro spreads to the whole pack in
    /// canonical member order.
    pub fn aggro_members(&self, triggered: EntityId) -> Vec<EntityId> {
        if !self.aggro_linkage || !self.contains(triggered) {
            return vec![triggered];
        }
        let mut members = self.members.clone();
        members.push(self.leader);
        members.sort();
        members
    }

    pub fn contains(&self, entity: EntityId) -> bool {
        entity == self.leader || self.members.contains(&entity)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormationPolicy {
    Loose,
    Surround,
    LineBehind,
}

/// Composable champion/unique modifiers (SPEC.md section 65). Described by
/// data wherever possible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChampionModifier {
    ExtraFast,
    ExtraStrong,
    Resistant,
    Aura,
    Multishot,
    Teleport,
    Cursed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChampionProfile {
    pub modifiers: Vec<ChampionModifier>,
}

impl ChampionProfile {
    pub fn is_champion(&self) -> bool {
        !self.modifiers.is_empty()
    }

    /// Stat modifier contributions in basis points (SPEC.md section 65).
    pub fn speed_bonus_bp(&self) -> i32 {
        if self.modifiers.contains(&ChampionModifier::ExtraFast) {
            5_000
        } else {
            0
        }
    }

    pub fn damage_bonus_bp(&self) -> i32 {
        if self.modifiers.contains(&ChampionModifier::ExtraStrong) {
            7_500
        } else {
            0
        }
    }

    pub fn resist_bonus_bp(&self) -> i32 {
        if self.modifiers.contains(&ChampionModifier::Resistant) {
            4_000
        } else {
            0
        }
    }
}
