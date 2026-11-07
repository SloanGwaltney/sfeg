use std::time::Duration;

use crate::frame::FrameContext;
use crate::game_state::{GameState, GameStateTickContext};
use crate::scene::Scene;
use crate::time::{GameTime, MaxTicksPerFrame, TickRate};

/// Real time is owed in units of 1 / (10^9 * tick rate) seconds, so one
/// tick is exactly this many units at any tick rate.
const UNITS_PER_TICK: u128 = 1_000_000_000;

/// The largest `f32` below 1, so the interpolation factor never rounds up
/// to a whole tick.
const LARGEST_FACTOR: f32 = 1.0 - f32::EPSILON / 2.0;

/// The window-free core that owns the game and advances it (ADR 0006).
///
/// Headless runs and tests drive it one tick at a time with
/// [`Core::tick`]. The windowed runner drives it by real elapsed time with
/// [`Core::advance`].
pub struct Core<G: GameState> {
    game_state: G,
    current_scene: Scene,
    game_time: GameTime,
    max_ticks_per_frame: MaxTicksPerFrame,
    /// Real time that [`Core::advance`] hasn't turned into ticks yet, in
    /// units of [`UNITS_PER_TICK`]. Always less than one tick between calls.
    owed_real_time: u128,
}

impl<G: GameState> Core<G> {
    /// Creates a core that runs `game_state` at `tick_rate`, starting in
    /// `first_scene`. [`Core::advance`] runs at most `max_ticks_per_frame`
    /// ticks per call.
    ///
    /// A game can't start without a scene. Its setup always builds the
    /// first one (ADR 0006), even if it starts empty with
    /// [`Scene::new`].
    pub fn new(
        game_state: G,
        first_scene: Scene,
        tick_rate: TickRate,
        max_ticks_per_frame: MaxTicksPerFrame,
    ) -> Core<G> {
        Core {
            game_state,
            current_scene: first_scene,
            game_time: GameTime::new(tick_rate),
            max_ticks_per_frame,
            owed_real_time: 0,
        }
    }

    /// Runs one rendered frame's worth of game (ADR 0008): adds `real_dt`
    /// of real time, runs the ticks that fit, then runs `frame` on the game
    /// state and every object.
    ///
    /// Real time left over that doesn't fill a tick carries into the next
    /// call, and sets the frame's
    /// [`interpolation_factor`](FrameContext::interpolation_factor). Time
    /// is counted in whole nanoseconds, so no time is lost to rounding.
    ///
    /// If more than [`MaxTicksPerFrame`] ticks fit, only that many run and
    /// the rest are dropped: the game slows down instead of falling further
    /// behind. The returned summary says how many ran and how many were
    /// dropped.
    ///
    /// `frame` runs even when no tick does, such as on a display faster
    /// than the tick rate.
    pub fn advance(&mut self, real_dt: Duration) -> AdvanceSummary {
        let hz = u128::from(self.game_time.tick_rate().hz());
        // Can't overflow: owed time is under one tick, and even
        // `Duration::MAX` in nanoseconds times `u32::MAX` fits in a u128.
        self.owed_real_time += real_dt.as_nanos() * hz;
        let ticks_owed = self.owed_real_time / UNITS_PER_TICK;
        self.owed_real_time %= UNITS_PER_TICK;

        let ticks_run = self
            .max_ticks_per_frame
            .ticks()
            .min(u32::try_from(ticks_owed).unwrap_or(u32::MAX));
        let ticks_dropped = u64::try_from(ticks_owed - u128::from(ticks_run)).unwrap_or(u64::MAX);
        for _ in 0..ticks_run {
            self.tick();
        }

        let interpolation_factor =
            ((self.owed_real_time as f64 / UNITS_PER_TICK as f64) as f32).min(LARGEST_FACTOR);
        let mut ctx =
            FrameContext::new(&self.game_time, real_dt.as_secs_f32(), interpolation_factor);
        self.game_state.frame(&mut ctx);
        self.current_scene.frame(&mut ctx);

        AdvanceSummary {
            ticks_run,
            ticks_dropped,
        }
    }

    /// Runs exactly one tick (ADR 0008): the game state, then every
    /// object, then the spawns and despawns they asked for (ADR 0012).
    pub fn tick(&mut self) {
        let mut ctx = GameStateTickContext::new(&self.game_time);
        self.game_state.tick(&mut ctx);
        self.current_scene.tick(&self.game_time);
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

    /// The scene that is running now, read-only.
    pub fn current_scene(&self) -> &Scene {
        &self.current_scene
    }

    /// The scene that is running now, to change between ticks, such as
    /// with [`Scene::spawn_immediate`].
    pub fn current_scene_mut(&mut self) -> &mut Scene {
        &mut self.current_scene
    }
}

/// What one call to [`Core::advance`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdvanceSummary {
    ticks_run: u32,
    ticks_dropped: u64,
}

impl AdvanceSummary {
    /// How many ticks ran.
    pub fn ticks_run(&self) -> u32 {
        self.ticks_run
    }

    /// How many ticks' worth of real time was dropped because of the
    /// [`MaxTicksPerFrame`] cap. Non-zero means the game is running slower
    /// than real time.
    pub fn ticks_dropped(&self) -> u64 {
        self.ticks_dropped
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    use crate::GameObject;
    use crate::math::Transform;
    use crate::scene::Parent;

    fn new_core<G: GameState>(game_state: G) -> Core<G> {
        Core::new(
            game_state,
            Scene::new(),
            TickRate::DEFAULT,
            MaxTicksPerFrame::DEFAULT,
        )
    }

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
        let mut core = new_core(RecordTicks::default());
        for _ in 0..3 {
            core.tick();
        }
        assert_eq!(core.game_state().seen, vec![0, 1, 2]);
        assert_eq!(core.game_time().tick_count(), 3);
    }

    #[test]
    fn sixty_ticks_at_60_hz_is_one_second() {
        let mut core = new_core(());
        for _ in 0..60 {
            core.tick();
        }
        assert_eq!(core.game_time().elapsed_seconds(), 1.0);
    }

    #[test]
    fn advancing_one_second_in_steps_runs_exactly_sixty_ticks() {
        let mut core = new_core(());
        let mut ticks_run = 0;
        for _ in 0..10 {
            ticks_run += core.advance(Duration::from_millis(100)).ticks_run();
        }
        assert_eq!(ticks_run, 60);
        assert_eq!(core.game_time().tick_count(), 60);
    }

    #[test]
    fn leftover_time_carries_into_the_next_advance() {
        let mut core = new_core(());
        // 10 ms is 0.6 of a tick at 60 Hz.
        assert_eq!(core.advance(Duration::from_millis(10)).ticks_run(), 0);
        assert_eq!(core.advance(Duration::from_millis(10)).ticks_run(), 1);
        assert_eq!(core.game_time().tick_count(), 1);
    }

    #[test]
    fn advance_drops_ticks_past_the_cap() {
        let mut core = new_core(());
        let summary = core.advance(Duration::from_secs(1));
        assert_eq!(summary.ticks_run(), 8);
        assert_eq!(summary.ticks_dropped(), 52);
        assert_eq!(core.game_time().tick_count(), 8);
        // The dropped time is gone, not owed to the next frame.
        assert_eq!(core.advance(Duration::ZERO).ticks_run(), 0);
    }

    #[test]
    fn a_huge_real_dt_does_not_overflow() {
        let mut core = new_core(());
        let summary = core.advance(Duration::MAX);
        assert_eq!(summary.ticks_run(), 8);
        assert_eq!(summary.ticks_dropped(), u64::MAX);
    }

    /// Records each `frame` call, shared so the test can read it after the
    /// core takes ownership.
    type FrameLog = Rc<RefCell<Vec<String>>>;

    struct LogFrames {
        name: &'static str,
        log: FrameLog,
    }

    impl GameState for LogFrames {
        fn frame(&mut self, ctx: &mut FrameContext) {
            log_frame(&self.log, self.name, ctx);
        }
    }

    impl GameObject for LogFrames {
        fn frame(&mut self, ctx: &mut FrameContext) {
            log_frame(&self.log, self.name, ctx);
        }
    }

    fn log_frame(log: &FrameLog, name: &str, ctx: &FrameContext) {
        log.borrow_mut().push(format!(
            "{name}: tick {} real_dt {} factor {}",
            ctx.game_time().tick_count(),
            ctx.real_dt(),
            ctx.interpolation_factor()
        ));
    }

    #[test]
    fn frame_runs_once_per_advance_after_its_ticks_game_state_first() {
        let log = FrameLog::default();
        let mut core = new_core(LogFrames {
            name: "game state",
            log: log.clone(),
        });
        core.current_scene_mut()
            .spawn_immediate(
                LogFrames {
                    name: "object",
                    log: log.clone(),
                },
                Parent::Root,
                Transform::IDENTITY,
            )
            .expect("the root is always there");

        // 25 ms is 1.5 ticks at 60 Hz.
        core.advance(Duration::from_millis(25));
        assert_eq!(
            *log.borrow(),
            [
                "game state: tick 1 real_dt 0.025 factor 0.5",
                "object: tick 1 real_dt 0.025 factor 0.5",
            ]
        );
    }

    #[test]
    fn frame_runs_even_when_no_tick_does() {
        let log = FrameLog::default();
        let mut core = new_core(LogFrames {
            name: "game state",
            log: log.clone(),
        });
        let summary = core.advance(Duration::ZERO);
        assert_eq!(summary.ticks_run(), 0);
        assert_eq!(*log.borrow(), ["game state: tick 0 real_dt 0 factor 0"]);
    }

    #[test]
    fn tick_alone_never_runs_frame() {
        let log = FrameLog::default();
        let mut core = new_core(LogFrames {
            name: "game state",
            log: log.clone(),
        });
        core.tick();
        assert!(log.borrow().is_empty());
    }
}
