//! SFEG's window-free core.
//!
//! The core runs game logic without a window or GPU (ADR 0006). It never
//! depends on winit, wgpu, or any other platform library (ADR 0007), so it
//! builds and tests on its own.
//!
//! Right now it holds the game state and advances it one fixed tick at a
//! time with [`Core::tick`].

mod core_loop;
mod game_state;
mod time;

pub use core_loop::Core;
pub use game_state::{GameState, TickContext};
pub use time::{TickRate, TickRateError, Time};
