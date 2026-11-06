use crate::time::Time;

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
    time: &'a Time,
}

impl<'a> TickContext<'a> {
    pub(crate) fn new(time: &'a Time) -> TickContext<'a> {
        TickContext { time }
    }

    /// Game time at the start of this tick.
    pub fn time(&self) -> &Time {
        self.time
    }
}
