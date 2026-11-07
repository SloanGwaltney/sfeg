//! SFEG: Safe, Fun, Easy Games.
//!
//! Games depend on this crate only. It re-exports the engine crates so the
//! split between them stays an engine detail (ADR 0007).
//!
//! [`platform`] is what the runner and renderer use. Games only need it in
//! tests, to check what [`Core::draw`] drew.

pub use sfeg_core::{
    ActiveCamera, ActiveCameraError, AdvanceSummary, Color, ColorError, Core, DrawContext,
    FrameContext, GameObject, GameState, GameStateTickContext, GameTime, Handle, HierarchyError,
    MaxTicksPerFrame, MaxTicksPerFrameError, ObjectHandle, Orthographic, Parent, Perspective,
    Projection, ProjectionError, Scene, TickContext, TickRate, TickRateError,
    TransformInterpolation, TypedHandleError,
};
pub use sfeg_core::{math, platform};
pub use sfeg_winit::{RunError, WindowSettings, run};
