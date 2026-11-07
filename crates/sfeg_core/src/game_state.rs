use crate::frame::FrameContext;
use crate::input::InputView;
use crate::time::GameTime;

/// The game-wide state: one game-defined struct that lasts for the whole run
/// (ADR 0012).
///
/// Game state is optional. A game without one uses `()`.
pub trait GameState {
    /// Runs once per fixed tick and changes the game (ADR 0008).
    fn tick(&mut self, ctx: &mut GameStateTickContext) {
        let _ = ctx;
    }

    /// Runs once per rendered frame, before any object's `frame`, and only
    /// updates presentation (ADR 0008, ADR 0012). See
    /// [`GameObject::frame`](crate::GameObject::frame).
    fn frame(&mut self, ctx: &mut FrameContext) {
        let _ = ctx;
    }
}

impl GameState for () {}

/// What the game state's tick can see and do.
///
/// It is separate from the object [`TickContext`](crate::TickContext)
/// because game state has no transform (ADR 0012). For now it holds game
/// time and input.
pub struct GameStateTickContext<'a> {
    game_time: &'a GameTime,
    tick_input: &'a InputView,
}

impl<'a> GameStateTickContext<'a> {
    pub(crate) fn new(
        game_time: &'a GameTime,
        tick_input: &'a InputView,
    ) -> GameStateTickContext<'a> {
        GameStateTickContext {
            game_time,
            tick_input,
        }
    }

    /// The tick view of the input actions: "just pressed" means since the
    /// last tick (ADR 0008).
    pub fn tick_input(&self) -> &InputView {
        self.tick_input
    }

    /// Game time at the start of this tick.
    pub fn game_time(&self) -> &GameTime {
        self.game_time
    }
}
