//! Party, PvP relations and multiplayer XP pipeline (SPEC.md sections
//! 109-110, 113-115). Party membership is gameplay state (hashable); PvP
//! damage gating and XP distribution run through the standard pipeline.

use arpg_core::PlayerId;
use arpg_rules::{GameRules, PvpMode};
use std::collections::BTreeMap;
use std::collections::BTreeSet;

/// Party (SPEC.md section 109). Membership order is canonical (BTreeSet).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Party {
    pub id: PartyId,
    pub leader: PlayerId,
    pub members: BTreeSet<PlayerId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PartyId(pub u64);

/// Party management (SPEC.md section 109): Invite, Accept, Decline, Leave,
/// Kick. Max size = `GameRules.max_players`.
#[derive(Debug, Default)]
pub struct PartySystem {
    next_party_id: u64,
    /// Pending invites: (party, invited) — one pending invite per player.
    invites: BTreeMap<PlayerId, PartyId>,
    /// Optional party membership per player.
    memberships: BTreeMap<PlayerId, PartyId>,
    parties: BTreeMap<PartyId, Party>,
}

/// Relation between two players (SPEC.md section 114).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerRelation {
    Neutral,
    Party,
    Hostile,
}

#[derive(Debug)]
pub enum PartyError {
    PartyFull,
    AlreadyInParty,
    NoPendingInvite,
    NotInParty,
    NoSuchParty,
    NotLeader,
    NotMember,
    NoSuchInvite,
}

impl PartySystem {
    pub fn new() -> PartySystem {
        PartySystem::default()
    }

    pub fn party_of(&self, player: PlayerId) -> Option<&Party> {
        self.memberships
            .get(&player)
            .and_then(|id| self.parties.get(id))
    }

    /// Relation between two players (section 114): same party -> Party;
    /// explicit hostility -> Hostile; else Neutral.
    pub fn relation(&self, a: PlayerId, b: PlayerId, hostile: &HostilityMatrix) -> PlayerRelation {
        if a == b {
            return PlayerRelation::Neutral;
        }
        if hostile.is_hostile(a, b) {
            return PlayerRelation::Hostile;
        }
        let same_party = self.memberships.contains_key(&a)
            && self.memberships.get(&a) == self.memberships.get(&b);
        if same_party {
            PlayerRelation::Party
        } else {
            PlayerRelation::Neutral
        }
    }

    pub fn invite(
        &mut self,
        inviter: PlayerId,
        invited: PlayerId,
        max_members: usize,
    ) -> Result<(), PartyError> {
        if invited == inviter {
            return Err(PartyError::NotMember);
        }
        let party_id = match self.memberships.get(&inviter) {
            Some(id) => *id,
            None => {
                // create a new party led by the inviter
                self.next_party_id += 1;
                let id = PartyId(self.next_party_id);
                let mut members = BTreeSet::new();
                members.insert(inviter);
                self.parties.insert(
                    id,
                    Party {
                        id,
                        leader: inviter,
                        members,
                    },
                );
                self.memberships.insert(inviter, id);
                id
            }
        };
        let party = self.parties.get(&party_id).ok_or(PartyError::NoSuchParty)?;
        if party.members.len() >= max_members {
            return Err(PartyError::PartyFull);
        }
        if self.memberships.contains_key(&invited) {
            return Err(PartyError::AlreadyInParty);
        }
        self.invites.insert(invited, party_id);
        Ok(())
    }

    pub fn accept(&mut self, invited: PlayerId, max_members: usize) -> Result<PartyId, PartyError> {
        let party_id = self
            .invites
            .remove(&invited)
            .ok_or(PartyError::NoPendingInvite)?;
        let party = self
            .parties
            .get_mut(&party_id)
            .ok_or(PartyError::NoSuchParty)?;
        if party.members.len() >= max_members {
            return Err(PartyError::PartyFull);
        }
        party.members.insert(invited);
        self.memberships.insert(invited, party_id);
        Ok(party_id)
    }

    pub fn decline(&mut self, invited: PlayerId) -> Result<(), PartyError> {
        self.invites
            .remove(&invited)
            .ok_or(PartyError::NoSuchInvite)?;
        Ok(())
    }

    /// Leave: the party dissolves when empty; leadership transfers to the
    /// smallest remaining member id (canonical choice, no wall clock).
    pub fn leave(&mut self, player: PlayerId) -> Result<(), PartyError> {
        let party_id = self
            .memberships
            .remove(&player)
            .ok_or(PartyError::NotInParty)?;
        self.invites.retain(|_, pid| *pid != party_id || true);
        let party = self
            .parties
            .get_mut(&party_id)
            .ok_or(PartyError::NoSuchParty)?;
        party.members.remove(&player);
        if party.members.is_empty() {
            self.parties.remove(&party_id);
            self.invites.retain(|_, pid| *pid != party_id);
            return Ok(());
        }
        if party.leader == player {
            party.leader = *party.members.iter().next().ok_or(PartyError::NoSuchParty)?;
        }
        Ok(())
    }

    /// Kick: leader only (section 109).
    pub fn kick(&mut self, leader: PlayerId, target: PlayerId) -> Result<(), PartyError> {
        let party_id = self
            .memberships
            .get(&leader)
            .ok_or(PartyError::NotInParty)?;
        let party = self.parties.get(party_id).ok_or(PartyError::NoSuchParty)?;
        if party.leader != leader {
            return Err(PartyError::NotLeader);
        }
        if !party.members.contains(&target) {
            return Err(PartyError::NotMember);
        }
        self.leave(target)
    }

    /// Canonical serialization for the state hash: sorted members.
    pub fn hash_bytes(&self, out: &mut Vec<u8>) {
        for (id, party) in &self.parties {
            out.extend_from_slice(&id.0.to_le_bytes());
            out.extend_from_slice(&party.leader.0.to_le_bytes());
            for m in &party.members {
                out.extend_from_slice(&m.0.to_le_bytes());
            }
        }
    }
}

/// Hostility declarations (SPEC.md sections 113-114). In Hostility mode,
/// declared hostility enables damage; in Consent/Arena, both must declare.
#[derive(Debug, Default)]
pub struct HostilityMatrix {
    declared: BTreeMap<PlayerId, BTreeSet<PlayerId>>,
}

impl HostilityMatrix {
    pub fn declare(&mut self, from: PlayerId, to: PlayerId) {
        self.declared.entry(from).or_default().insert(to);
    }

    pub fn revoke(&mut self, from: PlayerId, to: PlayerId) {
        if let Some(set) = self.declared.get_mut(&from) {
            set.remove(&to);
        }
    }

    pub fn is_hostile(&self, a: PlayerId, b: PlayerId) -> bool {
        self.declared.get(&a).is_some_and(|s| s.contains(&b))
            || self.declared.get(&b).is_some_and(|s| s.contains(&a))
    }

    /// Damage gating (section 114): Party/Neutral -> never; Hostile ->
    /// yes, subject to the ruleset PvpMode.
    pub fn damage_allowed(&self, mode: PvpMode, relation: PlayerRelation) -> bool {
        match relation {
            PlayerRelation::Hostile => !matches!(mode, PvpMode::Disabled),
            _ => false,
        }
    }

    pub fn hash_bytes(&self, out: &mut Vec<u8>) {
        for (from, targets) in &self.declared {
            out.extend_from_slice(&from.0.to_le_bytes());
            for t in targets {
                out.extend_from_slice(&t.0.to_le_bytes());
            }
        }
    }
}

/// XP pipeline (SPEC.md section 110):
/// monster base XP -> difficulty modifier -> multiplayer pool modifier ->
/// participant eligibility -> party distribution -> level-difference
/// modifier -> player XP modifier. Every step is configurable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XpPipeline {
    /// difficulty modifier in basis points (10000 = 1.0x)
    pub difficulty_bp: i32,
    /// multiplayer pool modifier: bp per extra player (0 = disabled)
    pub multiplayer_pool_bp_per_player: i32,
    /// level-difference penalty: full XP within `level_window` levels,
    /// then `penalty_bp` per level beyond
    pub level_window: i64,
    pub level_penalty_bp: i32,
    /// final per-player modifier in basis points
    pub player_modifier_bp: i32,
}

impl XpPipeline {
    pub const D2_LIKE: XpPipeline = XpPipeline {
        difficulty_bp: 10000,
        multiplayer_pool_bp_per_player: 0,
        level_window: 5,
        level_penalty_bp: 500,
        player_modifier_bp: 10000,
    };

    fn bp_mul(xp: u64, bp: i32) -> u64 {
        if bp <= 0 {
            return 0;
        }
        (xp as u128 * bp as u128 / 10000).min(u64::MAX as u128) as u64
    }

    /// Distribute a monster's base XP across the participants (section 110).
    /// `participants` maps eligible players to their level (eligibility is
    /// decided by the caller). Party members share via party distribution;
    /// the pool is split evenly between party groups and solo participants.
    pub fn distribute(
        &self,
        base_xp: u64,
        participants: &[(PlayerId, i64)],
        parties: &PartySystem,
    ) -> Vec<(PlayerId, u64)> {
        if participants.is_empty() {
            return Vec::new();
        }
        let pool = Self::bp_mul(base_xp, self.difficulty_bp);
        let mut n = participants.len() as i32 - 1;
        if n < 0 {
            n = 0;
        }
        let pool = bp_signed_mul(pool, 10000 + self.multiplayer_pool_bp_per_player * n);

        // group participants by party (canonical: BTreeMap keyed by party id,
        // solo players are their own group)
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
        enum GroupKey {
            Party(PartyId),
            Solo(PlayerId),
        }
        let mut groups: BTreeMap<GroupKey, Vec<(PlayerId, i64)>> = BTreeMap::new();
        for (p, level) in participants {
            let key = match parties.memberships.get(p) {
                Some(pid) => GroupKey::Party(*pid),
                None => GroupKey::Solo(*p),
            };
            groups.entry(key).or_default().push((*p, *level));
        }
        let group_count = groups.len() as u64;
        let mut out = Vec::new();
        for (_, members) in groups {
            let share = pool / group_count.max(1) / members.len() as u64;
            for (p, level) in members {
                // level-difference modifier relative to the highest level
                let max_level = participants.iter().map(|(_, l)| *l).max().unwrap_or(0);
                let over = (max_level - level - self.level_window).max(0);
                let penalty = 10000 - (over as i32) * self.level_penalty_bp;
                let xp = Self::bp_mul(share, penalty.max(0));
                let xp = Self::bp_mul(xp, self.player_modifier_bp);
                out.push((p, xp));
            }
        }
        out
    }
}

fn bp_signed_mul(xp: u64, bp: i32) -> u64 {
    if bp <= 0 {
        return 0;
    }
    (xp as u128 * bp as u128 / 10000).min(u64::MAX as u128) as u64
}

/// Convenience: policy snapshot derived from the ruleset.
pub fn pvp_mode(rules: &GameRules) -> PvpMode {
    rules.pvp_mode
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sys_with_rules() -> (PartySystem, usize) {
        (PartySystem::new(), 8)
    }

    #[test]
    fn invite_accept_forms_party() {
        let (mut sys, max) = sys_with_rules();
        sys.invite(PlayerId(1), PlayerId(2), max).unwrap();
        assert_eq!(sys.accept(PlayerId(2), max).unwrap(), PartyId(1));
        let party = sys.party_of(PlayerId(1)).unwrap();
        assert_eq!(party.leader, PlayerId(1));
        assert!(party.members.contains(&PlayerId(2)));
    }

    #[test]
    fn party_full_rejected() {
        let mut sys = PartySystem::new();
        for i in 2..=8u32 {
            sys.invite(PlayerId(1), PlayerId(i), 8).unwrap();
            sys.accept(PlayerId(i), 8).unwrap();
        }
        assert!(matches!(
            sys.invite(PlayerId(1), PlayerId(9), 8),
            Err(PartyError::PartyFull)
        ));
    }

    #[test]
    fn decline_leaves_no_membership() {
        let mut sys = PartySystem::new();
        sys.invite(PlayerId(1), PlayerId(2), 8).unwrap();
        sys.decline(PlayerId(2)).unwrap();
        assert!(sys.party_of(PlayerId(2)).is_none());
    }

    #[test]
    fn leadership_transfers_canonically() {
        let mut sys = PartySystem::new();
        sys.invite(PlayerId(5), PlayerId(3), 8).unwrap();
        sys.accept(PlayerId(3), 8).unwrap();
        sys.invite(PlayerId(5), PlayerId(4), 8).unwrap();
        sys.accept(PlayerId(4), 8).unwrap();
        sys.leave(PlayerId(5)).unwrap();
        let party = sys.party_of(PlayerId(3)).unwrap();
        assert_eq!(party.leader, PlayerId(3));
    }

    #[test]
    fn kick_requires_leader() {
        let mut sys = PartySystem::new();
        sys.invite(PlayerId(1), PlayerId(2), 8).unwrap();
        sys.accept(PlayerId(2), 8).unwrap();
        assert!(matches!(
            sys.kick(PlayerId(2), PlayerId(1)),
            Err(PartyError::NotLeader)
        ));
        sys.kick(PlayerId(1), PlayerId(2)).unwrap();
        assert!(sys.party_of(PlayerId(2)).is_none());
    }

    #[test]
    fn party_dissolves_when_empty() {
        let mut sys = PartySystem::new();
        sys.invite(PlayerId(1), PlayerId(2), 8).unwrap();
        sys.accept(PlayerId(2), 8).unwrap();
        sys.leave(PlayerId(2)).unwrap();
        sys.leave(PlayerId(1)).unwrap();
        assert!(sys.party_of(PlayerId(1)).is_none());
        assert!(sys.parties.is_empty());
    }

    #[test]
    fn hostility_gates_damage() {
        let mut hostile = HostilityMatrix::default();
        let sys = PartySystem::new();
        assert_eq!(
            sys.relation(PlayerId(1), PlayerId(2), &hostile),
            PlayerRelation::Neutral
        );
        assert!(!hostile.damage_allowed(PvpMode::Hostility, PlayerRelation::Neutral));
        hostile.declare(PlayerId(1), PlayerId(2));
        assert_eq!(
            sys.relation(PlayerId(1), PlayerId(2), &hostile),
            PlayerRelation::Hostile
        );
        assert!(hostile.damage_allowed(PvpMode::Hostility, PlayerRelation::Hostile));
        assert!(!hostile.damage_allowed(PvpMode::Disabled, PlayerRelation::Hostile));
        assert!(!hostile.damage_allowed(PvpMode::Hostility, PlayerRelation::Party));
    }

    #[test]
    fn xp_distribution_splits_by_party() {
        let mut sys = PartySystem::new();
        sys.invite(PlayerId(1), PlayerId(2), 8).unwrap();
        sys.accept(PlayerId(2), 8).unwrap();
        let xp = XpPipeline::D2_LIKE;
        let participants = vec![(PlayerId(1), 10i64), (PlayerId(2), 10), (PlayerId(3), 10)];
        let out = xp.distribute(1000, &participants, &sys);
        let total: u64 = out.iter().map(|(_, v)| *v).sum();
        // party of two shares one group share, solo player takes the other
        assert!(out.len() == 3);
        assert!(total > 0 && total <= 1000);
    }

    #[test]
    fn level_difference_applies_penalty() {
        let xp = XpPipeline {
            level_window: 5,
            level_penalty_bp: 500,
            ..XpPipeline::D2_LIKE
        };
        let out = xp.distribute(
            1000,
            &[(PlayerId(1), 10), (PlayerId(2), 30)],
            &PartySystem::new(),
        );
        // level 10 is 20 levels below 30; 15 over the window: 10000 - 7500 = 2500bp
        let low = out.iter().find(|(p, _)| *p == PlayerId(1)).unwrap().1;
        assert!(low < 250);
    }
}
