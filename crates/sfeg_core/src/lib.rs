//! SFEG's window-free core.
//!
//! The core runs game logic without a window or GPU (ADR 0006). It never
//! depends on winit, wgpu, or any other platform library (ADR 0007), so it
//! builds and tests on its own.
//!
//! Right now it holds the game state and one [`Scene`] of game objects
//! with its scene state,
//! advances them one fixed tick at a time with [`Core::tick`] or by real
//! elapsed time with [`Core::advance`], collects what they draw with
//! [`Core::draw`], and provides the [`math`] types. What the runner and
//! renderer take from it lives in [`platform`].

pub mod assets;
mod camera;
mod color;
mod core_loop;
mod draw;
mod frame;
mod game_object;
mod game_state;
mod handle;
pub mod input;
pub mod math;
pub mod platform;
mod scene;
mod scene_state;
mod time;

pub use camera::{
    ActiveCamera, ActiveCameraError, Orthographic, Perspective, Projection, ProjectionError,
};
pub use color::{Color, ColorError};
pub use core_loop::{AdvanceSummary, Core, CoreSettings};
pub use draw::DrawContext;
pub use frame::FrameContext;
pub use game_object::GameObject;
pub use game_state::{GameState, GameStateTickContext};
pub use handle::{Handle, ObjectHandle};
pub use scene::{
    HierarchyError, Parent, Scene, TickContext, TransformInterpolation, TypedHandleError,
};
pub use scene_state::{SceneState, SceneStateTickContext};
pub use time::{GameTime, MaxTicksPerFrame, MaxTicksPerFrameError, TickRate, TickRateError};
