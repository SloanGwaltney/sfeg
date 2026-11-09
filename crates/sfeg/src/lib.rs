//! SFEG: Safe, Fun, Easy Games.
//!
//! Games depend on this crate only. It re-exports the engine crates so the
//! split between them stays an engine detail (ADR 0007).
//!
//! [`platform`] is what the runner and renderer use. Games only need it in
//! tests, to check what [`Core::draw`] drew.

pub use sfeg_core::{
    ActiveCamera, ActiveCameraError, AdvanceSummary, BeginNextSceneError, Color, ColorError, Core,
    CoreSettings, DrawContext, FrameContext, GameObject, GameState, GameStateTickContext, GameTime,
    Handle, HierarchyError, MaxTicksPerFrame, MaxTicksPerFrameError, NoNextSceneError,
    ObjectHandle, Orthographic, Parent, Perspective, Projection, ProjectionError,
    QueueSceneChangeError, Scene, SceneState, SceneStateTickContext, TickContext, TickRate,
    TickRateError, TransformInterpolation, TypedHandleError,
};
pub use sfeg_core::{assets, math, platform};
pub use sfeg_winit::{RunError, WindowSettings, install_default_tracing_subscriber, run};

/// Rendering with no window, into images read back to the CPU. Headless
/// runs and tests use it to see what a frame shows. See
/// [`sfeg_wgpu::OffscreenRenderer`].
pub mod capture {
    pub use sfeg_wgpu::{
        CapturedImage, CapturedImageError, OffscreenRenderer, OffscreenRendererError,
        RenderToImageError,
    };
}

/// Input actions (ADR 0016). See [`sfeg_core::input`].
pub mod input {
    pub use sfeg_core::input::*;
    /// Declares an input action: `#[derive(Action)]` with
    /// `#[action(name = "jump", kind = Button)]`.
    pub use sfeg_macros::Action;
}
