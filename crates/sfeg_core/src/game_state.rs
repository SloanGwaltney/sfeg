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
}

impl GameState for () {}

/// What the game state's tick can see and do.
///
/// It is separate from the object [`TickContext`](crate::TickContext)
/// because game state has no transform (ADR 0012). For now it holds only
/// game time.
pub struct GameStateTickContext<'a> {
    game_time: &'a GameTime,
}

impl<'a> GameStateTickContext<'a> {
    pub(crate) fn new(game_time: &'a GameTime) -> GameStateTickContext<'a> {
        GameStateTickContext { game_time }
    }

    /// Game time at the start of this tick.
    pub fn game_time(&self) -> &GameTime {
        self.game_time
    }
}
