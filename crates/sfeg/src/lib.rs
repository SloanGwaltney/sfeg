//! SFEG: Safe, Fun, Easy Games.
//!
//! Games depend on this crate only. It re-exports the engine crates so the
//! split between them stays an engine detail (ADR 0007).

pub use sfeg_core::math;
pub use sfeg_core::{Core, GameState, GameTime, TickContext, TickRate, TickRateError};
