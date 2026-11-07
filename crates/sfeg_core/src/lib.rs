//! SFEG's window-free core.
//!
//! The core runs game logic without a window or GPU (ADR 0006). It never
//! depends on winit, wgpu, or any other platform library (ADR 0007), so it
//! builds and tests on its own.
//!
//! Right now it holds the game state and one [`Scene`] of game objects,
//! advances them one fixed tick at a time with [`Core::tick`], and
//! provides the [`math`] types.

mod core_loop;
mod game_object;
mod game_state;
mod handle;
pub mod math;
mod scene;
mod time;

pub use core_loop::Core;
pub use game_object::GameObject;
pub use game_state::{GameState, GameStateTickContext};
pub use handle::{Handle, ObjectHandle};
pub use scene::{Scene, TickContext, TypedHandleError};
pub use time::{GameTime, TickRate, TickRateError};
