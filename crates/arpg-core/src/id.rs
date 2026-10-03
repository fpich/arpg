use std::fmt;

macro_rules! newtype_id {
    ($name:ident($ty:ty)) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
        #[repr(transparent)]
        pub struct $name(pub $ty);

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

newtype_id!(EntityId(u64));
newtype_id!(PlayerId(u32));
newtype_id!(ItemId(u128));
newtype_id!(GameId(u128));
newtype_id!(SkillId(u32));
newtype_id!(StatId(u32));
newtype_id!(MonsterDefId(u32));
newtype_id!(ItemDefId(u32));
newtype_id!(LevelDefId(u32));
newtype_id!(LevelInstanceId(u64));
newtype_id!(QuestDefId(u32));
newtype_id!(ObjectDefId(u32));
newtype_id!(ObjectId(u64));
newtype_id!(ClassId(u32));
