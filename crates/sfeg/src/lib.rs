//! SFEG: Safe, Fun, Easy Games.
//!
//! Games depend on this crate only. It re-exports the engine crates so the
//! split between them stays an engine detail (ADR 0007).
//!
//! [`platform`] is what the runner and renderer use. Games only need it in
//! tests, to check what [`Core::draw`] drew.

pub use sfeg_core::{
    ActiveCamera, ActiveCameraError, AdvanceSummary, Color, ColorError, Core, CoreSettings,
    DrawContext, FrameContext, GameObject, GameState, GameStateTickContext, GameTime, Handle,
    HierarchyError, MaxTicksPerFrame, MaxTicksPerFrameError, ObjectHandle, Orthographic, Parent,
    Perspective, Projection, ProjectionError, Scene, SceneState, SceneStateTickContext,
    TickContext, TickRate, TickRateError, TransformInterpolation, TypedHandleError,
};
pub use sfeg_core::{math, platform};
pub use sfeg_winit::{RunError, WindowSettings, run};

/// Input actions (ADR 0016). See [`sfeg_core::input`].
pub mod input {
    pub use sfeg_core::input::*;
    /// Declares an input action: `#[derive(Action)]` with
    /// `#[action(name = "jump", kind = Button)]`.
    pub use sfeg_macros::Action;
}
