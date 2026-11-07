use crate::game_state::{GameState, GameStateTickContext};
use crate::scene::Scene;
use crate::time::{GameTime, TickRate};

/// The window-free core that owns the game and advances it (ADR 0006).
///
/// Headless runs and tests drive it directly with [`Core::tick`]. The
/// windowed runner will drive it the same way.
pub struct Core<G: GameState> {
    game_state: G,
    scene: Scene,
    game_time: GameTime,
}

impl<G: GameState> Core<G> {
    /// Creates a core that runs `game_state` and the first `scene` at
    /// `tick_rate`.
    pub fn new(game_state: G, scene: Scene, tick_rate: TickRate) -> Core<G> {
        Core {
            game_state,
            scene,
            game_time: GameTime::new(tick_rate),
        }
    }

    /// Runs exactly one tick (ADR 0008): the game state, then every
    /// object, then the spawns and despawns they asked for (ADR 0012).
    pub fn tick(&mut self) {
        let mut ctx = GameStateTickContext::new(&self.game_time);
        self.game_state.tick(&mut ctx);
        self.scene.tick(&self.game_time);
        self.game_time.advance_one_tick();
    }

    /// Game time so far.
    pub fn game_time(&self) -> &GameTime {
        &self.game_time
    }

    /// The game state, read-only.
    pub fn game_state(&self) -> &G {
        &self.game_state
    }

    /// The current scene, read-only.
    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    /// The current scene, to change between ticks. [`Scene::spawn`] adds
    /// objects right away here, since no tick is running.
    pub fn scene_mut(&mut self) -> &mut Scene {
        &mut self.scene
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
        fn tick(&mut self, ctx: &mut GameStateTickContext) {
            self.seen.push(ctx.game_time().tick_count());
        }
    }

    #[test]
    fn each_tick_calls_game_state_once_with_its_index() {
        let mut core = Core::new(RecordTicks::default(), Scene::new(), TickRate::DEFAULT);
        for _ in 0..3 {
            core.tick();
        }
        assert_eq!(core.game_state().seen, vec![0, 1, 2]);
        assert_eq!(core.game_time().tick_count(), 3);
    }

    #[test]
    fn sixty_ticks_at_60_hz_is_one_second() {
        let mut core = Core::new((), Scene::new(), TickRate::DEFAULT);
        for _ in 0..60 {
            core.tick();
        }
        assert_eq!(core.game_time().elapsed_seconds(), 1.0);
    }
}
