use crate::game_state::{GameState, TickContext};
use crate::time::{TickRate, Time};

/// The window-free core that owns the game and advances it (ADR 0006).
///
/// Headless runs and tests drive it directly with [`Core::tick`]. The
/// windowed runner will drive it the same way.
pub struct Core<G: GameState> {
    game_state: G,
    time: Time,
}

impl<G: GameState> Core<G> {
    /// Creates a core that runs `game_state` at `tick_rate`.
    pub fn new(game_state: G, tick_rate: TickRate) -> Core<G> {
        Core {
            game_state,
            time: Time::new(tick_rate),
        }
    }

    /// Runs exactly one tick (ADR 0008).
    pub fn tick(&mut self) {
        let mut ctx = TickContext::new(&self.time);
        self.game_state.tick(&mut ctx);
        self.time.advance_one_tick();
    }

    /// Game time so far.
    pub fn time(&self) -> &Time {
        &self.time
    }

    /// The game state, read-only.
    pub fn game_state(&self) -> &G {
        &self.game_state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct RecordTicks {
        seen: Vec<u64>,
    }

    impl GameState for RecordTicks {
        fn tick(&mut self, ctx: &mut TickContext) {
            self.seen.push(ctx.time().tick_count());
        }
    }

    #[test]
    fn each_tick_calls_game_state_once_with_its_index() {
        let mut core = Core::new(RecordTicks::default(), TickRate::DEFAULT);
        for _ in 0..3 {
            core.tick();
        }
        assert_eq!(core.game_state().seen, vec![0, 1, 2]);
        assert_eq!(core.time().tick_count(), 3);
    }

    #[test]
    fn sixty_ticks_at_60_hz_is_one_second() {
        let mut core = Core::new((), TickRate::DEFAULT);
        for _ in 0..60 {
            core.tick();
        }
        assert_eq!(core.time().elapsed_seconds(), 1.0);
    }
}
