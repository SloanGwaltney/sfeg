use crate::time::GameTime;

/// The game-wide state: one game-defined struct that lasts for the whole run
/// (ADR 0012).
///
/// Game state is optional. A game without one uses `()`.
pub trait GameState {
    /// Runs once per fixed tick and changes the game (ADR 0008).
    fn tick(&mut self, ctx: &mut TickContext) {
        let _ = ctx;
    }
}

impl GameState for () {}

/// What a tick callback can see and do.
///
/// For now this is only game time. It grows to hold the command queue,
/// tick input, and access to other objects (ADR 0008).
pub struct TickContext<'a> {
    game_time: &'a GameTime,
}

impl<'a> TickContext<'a> {
    pub(crate) fn new(game_time: &'a GameTime) -> TickContext<'a> {
        TickContext { game_time }
    }

    /// Game time at the start of this tick.
    pub fn game_time(&self) -> &GameTime {
        self.game_time
    }
}
