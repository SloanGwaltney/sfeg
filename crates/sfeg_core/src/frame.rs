use crate::input::InputView;
use crate::time::GameTime;

/// What a `frame` callback can see (ADR 0008).
///
/// `frame` runs once per rendered frame and only updates presentation, such
/// as camera smoothing, animation, and particles. So this context has no
/// command queue and no access to other objects. Code that changes the
/// game belongs in `tick`.
///
/// The same context is passed to game objects, the game state, and the
/// scene state.
pub struct FrameContext<'a> {
    game_time: &'a GameTime,
    real_dt: f32,
    interpolation_factor: f32,
    frame_input: &'a InputView,
}

impl<'a> FrameContext<'a> {
    pub(crate) fn new(
        game_time: &'a GameTime,
        real_dt: f32,
        interpolation_factor: f32,
        frame_input: &'a InputView,
    ) -> FrameContext<'a> {
        FrameContext {
            game_time,
            real_dt,
            interpolation_factor,
            frame_input,
        }
    }

    /// The frame view of the input actions: "just pressed" means since the
    /// last frame (ADR 0008). Use it for presentation that should react
    /// with the least delay, such as UI. Gameplay reads
    /// [`TickContext::tick_input`](crate::TickContext::tick_input).
    pub fn frame_input(&self) -> &InputView {
        self.frame_input
    }

    /// Game time after this frame's ticks ran.
    pub fn game_time(&self) -> &GameTime {
        self.game_time
    }

    /// Real (wall-clock) seconds since the previous frame.
    ///
    /// Unlike [`GameTime::dt`], this varies from frame to frame, and it
    /// keeps counting while the game slows down at the tick cap.
    pub fn real_dt(&self) -> f32 {
        self.real_dt
    }

    /// How far real time has moved past the last tick, as a fraction of one
    /// tick, from 0 up to but not including 1.
    ///
    /// Use it to blend values between the previous tick and the current
    /// one, so motion looks smooth at any refresh rate.
    pub fn interpolation_factor(&self) -> f32 {
        self.interpolation_factor
    }
}
