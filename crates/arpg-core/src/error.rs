use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum GameError {
    #[error("invalid command")]
    InvalidCommand,
    #[error("invalid state")]
    InvalidState,
    #[error("insufficient resource")]
    InsufficientResource,
    #[error("out of range")]
    OutOfRange,
    #[error("invalid target")]
    InvalidTarget,
    #[error("item unavailable")]
    ItemUnavailable,
    #[error("inventory full")]
    InventoryFull,
    #[error("requirement not met")]
    RequirementNotMet,
    #[error("internal invariant violated")]
    InternalInvariantError,
}
