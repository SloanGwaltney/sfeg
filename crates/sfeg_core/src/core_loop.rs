use std::path::PathBuf;
use std::time::Duration;

use crate::assets::{
    AssetManifest, AssetScope, AssetStore, SceneFileAsset, TextureAsset, TextureHandle,
    TextureState,
};

use crate::frame::FrameContext;
use crate::game_state::{GameState, GameStateTickContext};
use crate::input::{InputBindings, InputState};
use crate::platform::{DrawList, InputEvent, TextureChange};
use crate::scene::{Blend, Scene, TransformInterpolation};
use crate::scene_change::{SceneChange, SceneChangeToApply};
use crate::scene_file::{SceneFileError, SceneFileTree, SceneTypeRegistry, build_scene};
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
    /// The next scene, if the game is building one, and the switch or
    /// discard queued for it (ADR 0025).
    scene_change: SceneChange,
    game_time: GameTime,
    max_ticks_per_frame: MaxTicksPerFrame,
    /// Real time that [`Core::advance`] hasn't turned into ticks yet, in
    /// units of [`UNITS_PER_TICK`]. Always less than one tick between calls.
    owed_real_time: u128,
    /// How far real time is past the last tick, as of the last
    /// [`Core::advance`]. Zero right after a [`Core::tick`].
    interpolation_factor: f32,
    transform_interpolation: TransformInterpolation,
    input: InputState,
    assets: AssetStore,
    /// The types scene files can name (ADR 0026).
    scene_types: SceneTypeRegistry,
    /// The assets scene files can name (ADR 0026).
    asset_manifest: AssetManifest,
}

impl<G: GameState> Core<G> {
    /// Creates a core that runs `game_state`, starting in `first_scene`,
    /// with `settings`.
    ///
    /// A game can't start without a scene. Its setup always builds the
    /// first one (ADR 0006), even if it starts empty with
    /// [`Scene::new`].
    pub fn new(game_state: G, first_scene: Scene, settings: CoreSettings) -> Core<G> {
        let CoreSettings {
            tick_rate,
            max_ticks_per_frame,
            transform_interpolation,
            input_bindings,
            asset_folder,
            asset_manifest,
            scene_types,
        } = settings;
        Core {
            game_state,
            current_scene: first_scene,
            scene_change: SceneChange::new(),
            game_time: GameTime::new(tick_rate),
            max_ticks_per_frame,
            owed_real_time: 0,
            interpolation_factor: 0.0,
            transform_interpolation,
            input: InputState::new(input_bindings),
            assets: AssetStore::new(asset_folder),
            scene_types,
            asset_manifest,
        }
    }

    /// Creates a core that runs `game_state`, starting in the scene
    /// `first_scene_file` describes, with `settings` (ADR 0026).
    ///
    /// It blocks while the file is read and built, which happens before the
    /// window opens. The textures the file refers to load into the first
    /// scene's scope in the background, like any other load, and draw as
    /// placeholders until they are ready.
    ///
    /// # Errors
    ///
    /// The [`SceneFileError`] that stopped the file from loading, such as
    /// a type it names that isn't in [`CoreSettings::scene_types`].
    pub fn new_with_first_scene_file(
        game_state: G,
        first_scene_file: SceneFileAsset,
        settings: CoreSettings,
    ) -> Result<Core<G>, SceneFileError> {
        let mut core = Core::new(game_state, Scene::new(()), settings);
        let path = core.assets.asset_folder().join(first_scene_file.path());
        let tree = SceneFileTree::read(&path)?;
        let scope = core.assets.current_scene_scope();
        core.current_scene = build_scene(
            tree,
            &core.scene_types,
            &core.asset_manifest,
            &mut core.assets,
            scope,
        )?;
        Ok(core)
    }

    /// Runs one rendered frame's worth of game (ADR 0008): adds `real_dt`
    /// of real time, runs the ticks that fit, then runs `frame` on the game
    /// state, the scene state, and every object.
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
        self.interpolation_factor = interpolation_factor;
        self.input.apply_pending_to_frame_view();
        let mut ctx = FrameContext::new(
            &self.game_time,
            real_dt.as_secs_f32(),
            interpolation_factor,
            self.input.frame_view(),
        );
        self.game_state.frame(&mut ctx);
        self.current_scene.frame(&mut ctx);

        AdvanceSummary {
            ticks_run,
            ticks_dropped,
        }
    }

    /// Runs exactly one tick (ADR 0008): the game state, then the scene
    /// state, then every object, then the spawns and despawns they asked
    /// for (ADR 0012). Then the texture releases they queued are applied,
    /// and last the queued switch to the next scene, or discard of it
    /// (ADR 0025). Textures are only ever freed here.
    ///
    /// Input that arrived since the last tick is seen by this one, in
    /// [`TickContext::tick_input`](crate::TickContext::tick_input). So are
    /// texture loads that finished since then, in
    /// [`TickContext::texture_loads_finished_before_this_tick`](crate::TickContext::texture_loads_finished_before_this_tick).
    pub fn tick(&mut self) {
        self.assets.start_tick();
        self.input.apply_pending_to_tick_view();
        let tick_input = self.input.tick_view();
        let mut ctx = GameStateTickContext::new(
            &self.game_time,
            tick_input,
            &mut self.assets,
            &mut self.scene_change,
        );
        self.game_state.tick(&mut ctx);
        self.current_scene.tick(
            &self.game_time,
            tick_input,
            &mut self.game_state,
            &mut self.assets,
            &mut self.scene_change,
        );
        self.assets.apply_queued_releases();
        self.apply_queued_scene_change();
        self.scene_change.build_next_scene_if_file_read(
            &self.scene_types,
            &self.asset_manifest,
            &mut self.assets,
        );
        self.game_time.advance_one_tick();
        // No real time has passed since this tick.
        self.interpolation_factor = 0.0;
    }

    /// Starts loading `asset` and holds it until the game ends, such as
    /// while setting up the first scene. Returns its handle right away,
    /// while the file loads in the background (ADR 0015).
    ///
    /// Loading an asset that is already loaded or loading gives the same
    /// handle, and the game holds it too.
    pub fn load_texture_into_game_scope(&mut self, asset: TextureAsset) -> TextureHandle {
        self.assets.load_texture(asset, AssetScope::Game)
    }

    /// Starts loading `asset` and holds it until the current scene ends.
    /// Returns its handle right away, while the file loads in the
    /// background (ADR 0015).
    ///
    /// Loading an asset that is already loaded or loading gives the same
    /// handle, and the current scene holds it too.
    pub fn load_texture_into_current_scene_scope(&mut self, asset: TextureAsset) -> TextureHandle {
        let scope = self.assets.current_scene_scope();
        self.assets.load_texture(asset, scope)
    }

    /// Where `texture` is in loading.
    pub fn texture_state(&self, texture: TextureHandle) -> TextureState<'_> {
        self.assets.texture_state(texture)
    }

    /// Blocks until every texture load and scene file read in flight has
    /// finished, successfully or not. A next scene whose file was read is
    /// built when the next tick ends, and its textures start loading then.
    ///
    /// The windowed runner never calls this: loading must not freeze the
    /// game (ADR 0015). Headless runs and tests call it so a load finishes
    /// at a known point. The next tick reports the loads as finished, as
    /// if they had finished on their own.
    pub fn block_until_pending_loads_finish(&mut self) {
        self.assets.block_until_pending_loads_finish();
    }

    /// Takes the textures the renderer must upload or free since the last
    /// call, oldest first. The runner calls it before each frame and passes
    /// the changes to the renderer (ADR 0006).
    ///
    /// Each change is handed out once, so only one renderer can follow a
    /// core.
    pub fn drain_texture_changes_for_renderer(&mut self) -> Vec<TextureChange> {
        self.assets.drain_texture_changes_for_renderer()
    }

    /// Applies one input event from the platform, such as a key press
    /// (ADR 0016). The runner calls it for each event as it arrives.
    ///
    /// The next tick sees it in the tick view, and the next frame in the
    /// frame view. Tests can call it to press keys before a [`Core::tick`].
    pub fn apply_input_event(&mut self, event: InputEvent) {
        self.input.apply_event(event);
    }

    /// Runs every object's `draw` and returns what they drew, seen from the
    /// current scene's active camera (ADR 0008, 0024). The runner calls it
    /// after [`Core::advance`], and tests can call it to check what a frame
    /// would show.
    ///
    /// Transforms are blended by the interpolation factor of the last
    /// [`Core::advance`], unless interpolation is
    /// [disabled](TransformInterpolation::Disabled). Right after a
    /// [`Core::tick`] the factor is 0, which draws the transforms from
    /// before that tick.
    ///
    /// If the scene has no active camera, nothing is drawn and an error is
    /// logged, once each time the scene loses its camera.
    pub fn draw(&mut self) -> DrawList {
        let blend = match self.transform_interpolation {
            TransformInterpolation::Enabled => Blend::Between(self.interpolation_factor),
            TransformInterpolation::Disabled => Blend::Current,
        };
        self.current_scene.draw(blend, self.interpolation_factor)
    }

    /// Turns transform interpolation on or off for every scene. It is
    /// [enabled](TransformInterpolation::Enabled) unless set otherwise.
    pub fn set_transform_interpolation(&mut self, transform_interpolation: TransformInterpolation) {
        self.transform_interpolation = transform_interpolation;
    }

    /// Whether transforms are interpolated when drawing.
    pub fn transform_interpolation(&self) -> TransformInterpolation {
        self.transform_interpolation
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

    /// The scene the game is building to switch to, read-only, or `None`
    /// if there is none (ADR 0025).
    pub fn next_scene(&self) -> Option<&Scene> {
        self.scene_change.next_scene()
    }

    fn apply_queued_scene_change(&mut self) {
        let Some(change) = self.scene_change.take_queued() else {
            return;
        };
        match change {
            SceneChangeToApply::SwitchTo(next_scene) => {
                // Drops the old scene, its scene state, and its objects.
                self.current_scene = next_scene;
                self.assets.switch_to_next_scene_scope();
            }
            SceneChangeToApply::Discarded => self.assets.end_next_scene_scope(),
        }
    }
}

/// How a new [`Core`] runs, passed to [`Core::new`].
///
/// Every field must be set. There is deliberately no `Default`, so each
/// setting is a choice written where the game creates its core, even when
/// the choice is a default such as [`TickRate::DEFAULT`]. A new setting
/// added in a later SFEG version is a compile error in every game until
/// the game sets it.
///
/// Each field's type checks its own value, such as [`TickRate::new`]
/// rejecting zero, so a `CoreSettings` is always valid once built.
///
/// ```ignore
/// let settings = CoreSettings {
///     tick_rate: TickRate::DEFAULT,
///     max_ticks_per_frame: MaxTicksPerFrame::DEFAULT,
///     transform_interpolation: TransformInterpolation::Enabled,
///     input_bindings: InputBindings::none(),
///     asset_folder: PathBuf::from("assets"),
///     asset_manifest: AssetManifest::EMPTY,
///     scene_types: SceneTypeRegistry::new(),
/// };
/// ```
#[derive(Debug, Clone)]
pub struct CoreSettings {
    /// How many ticks run per second of game time (ADR 0008).
    pub tick_rate: TickRate,
    /// The most ticks one [`Core::advance`] runs. Past that, the game slows
    /// down instead of falling further behind.
    pub max_ticks_per_frame: MaxTicksPerFrame,
    /// Whether drawing blends transforms between the last two ticks
    /// (ADR 0013). [`Core::set_transform_interpolation`] can change it
    /// later.
    pub transform_interpolation: TransformInterpolation,
    /// Which keys drive each input action (ADR 0016). A game that takes no
    /// input sets [`InputBindings::none`].
    pub input_bindings: InputBindings,
    /// The folder the asset manifest's paths are relative to (ADR 0015),
    /// usually the folder the manifest is in. A game that loads no assets
    /// can set an empty path.
    pub asset_folder: PathBuf,
    /// Every asset in the manifest, so scene files can refer to them as
    /// `Asset("name")` (ADR 0026). Games pass the `MANIFEST` constant
    /// `sfeg_build` generates, or [`AssetManifest::EMPTY`].
    pub asset_manifest: AssetManifest,
    /// The object and scene state types scene files can name (ADR 0014,
    /// 0026). A game without scene files sets [`SceneTypeRegistry::new`].
    pub scene_types: SceneTypeRegistry,
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
    use crate::input::test_actions::Jump;
    use crate::math::Transform;
    use crate::scene::Parent;

    fn settings(input_bindings: InputBindings) -> CoreSettings {
        CoreSettings {
            tick_rate: TickRate::DEFAULT,
            max_ticks_per_frame: MaxTicksPerFrame::DEFAULT,
            transform_interpolation: TransformInterpolation::Enabled,
            input_bindings,
            asset_folder: PathBuf::new(),
            asset_manifest: AssetManifest::EMPTY,
            scene_types: SceneTypeRegistry::new(),
        }
    }

    fn new_core<G: GameState>(game_state: G) -> Core<G> {
        Core::new(game_state, Scene::new(()), settings(InputBindings::none()))
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

    /// Moves one unit along +x each tick and draws a cube.
    struct DrawnWalker;

    impl GameObject for DrawnWalker {
        fn tick(&mut self, ctx: &mut crate::TickContext) {
            let position = ctx.local().position() + crate::math::Vector3::UNIT_X;
            ctx.local_mut().set_position(position).unwrap();
        }

        fn draw(&self, ctx: &mut crate::DrawContext) {
            ctx.draw_cube(crate::Color::WHITE);
        }
    }

    fn core_with_walker() -> Core<()> {
        let mut core = new_core(());
        let scene = core.current_scene_mut();
        scene
            .spawn_immediate(DrawnWalker, Parent::Root, Transform::IDENTITY)
            .unwrap();
        let camera = scene
            .spawn_immediate(DrawnWalker, Parent::Root, Transform::IDENTITY)
            .unwrap();
        let perspective = crate::Perspective::new(1.0, 0.1, 100.0).unwrap();
        scene.set_active_camera(camera, perspective).unwrap();
        core
    }

    fn first_drawn_x(core: &mut Core<()>) -> f32 {
        match core.draw().draw_commands()[0] {
            crate::platform::DrawCommand::FlatMesh { world_matrix, .. }
            | crate::platform::DrawCommand::Sprite { world_matrix, .. } => {
                world_matrix.translation().x()
            }
        }
    }

    #[test]
    fn draw_uses_the_last_advances_interpolation_factor() {
        let mut core = core_with_walker();
        // 25 ms is 1.5 ticks at 60 Hz: one tick from 0 to 1, then halfway.
        core.advance(Duration::from_millis(25));
        assert_eq!(first_drawn_x(&mut core), 0.5);
        // A plain tick leaves no real time past it.
        core.tick();
        assert_eq!(first_drawn_x(&mut core), 1.0);
    }

    #[test]
    fn disabled_interpolation_draws_the_last_tick() {
        let mut core = core_with_walker();
        core.set_transform_interpolation(TransformInterpolation::Disabled);
        core.advance(Duration::from_millis(25));
        assert_eq!(first_drawn_x(&mut core), 1.0);
    }

    /// Records whether jump was just pressed in each tick and frame.
    #[derive(Default)]
    struct RecordJumps {
        log: Vec<String>,
    }

    impl GameState for RecordJumps {
        fn tick(&mut self, ctx: &mut GameStateTickContext) {
            let pressed = ctx.tick_input().just_pressed::<Jump>();
            self.log.push(format!("tick {pressed}"));
        }

        fn frame(&mut self, ctx: &mut FrameContext) {
            let pressed = ctx.frame_input().just_pressed::<Jump>();
            self.log.push(format!("frame {pressed}"));
        }
    }

    #[test]
    fn a_key_press_reaches_one_tick_and_one_frame() {
        let mut actions = crate::input::ActionRegistry::new();
        actions.register::<Jump>().unwrap();
        let bindings =
            InputBindings::from_toml("[button.jump]\nkeys = [\"Space\"]", &actions).unwrap();
        let mut core = Core::new(RecordJumps::default(), Scene::new(()), settings(bindings));
        core.apply_input_event(InputEvent::KeyPressed(crate::input::KeyCode::Space));
        // No tick fits in this frame, so the tick sees the press next frame.
        core.advance(Duration::from_millis(10));
        // Two ticks fit here; only the first sees it.
        core.advance(Duration::from_millis(30));
        assert_eq!(
            core.game_state().log,
            ["frame true", "tick true", "tick false", "frame false"]
        );
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

    /// Loads a missing texture on its first tick, then records what each
    /// tick reports as finished.
    #[derive(Default)]
    struct LoadOnFirstTick {
        texture: Option<TextureHandle>,
        finished_per_tick: Vec<Vec<TextureHandle>>,
    }

    impl GameState for LoadOnFirstTick {
        fn tick(&mut self, ctx: &mut GameStateTickContext) {
            if self.texture.is_none() {
                self.texture = Some(
                    ctx.load_texture_into_game_scope(TextureAsset::from_manifest(
                        "missing",
                        "missing.png",
                    )),
                );
            }
            self.finished_per_tick
                .push(ctx.texture_loads_finished_before_this_tick().to_vec());
        }
    }

    #[test]
    fn a_load_started_in_a_tick_is_reported_by_a_later_tick() {
        let mut core = new_core(LoadOnFirstTick::default());
        core.tick();
        let texture = core.game_state().texture.unwrap();
        assert!(matches!(core.texture_state(texture), TextureState::Loading));
        core.block_until_pending_loads_finish();
        assert!(matches!(
            core.texture_state(texture),
            TextureState::Failed(_)
        ));
        core.tick();
        core.tick();
        assert_eq!(
            core.game_state().finished_per_tick,
            [vec![], vec![texture], vec![]]
        );
    }

    /// Runs one step per tick, in order, then nothing.
    type Step = Box<dyn FnMut(&mut GameStateTickContext)>;

    struct ScriptedGameState {
        steps: std::collections::VecDeque<Step>,
    }

    impl ScriptedGameState {
        fn new(steps: Vec<Step>) -> ScriptedGameState {
            ScriptedGameState {
                steps: steps.into(),
            }
        }
    }

    impl GameState for ScriptedGameState {
        fn tick(&mut self, ctx: &mut GameStateTickContext) {
            if let Some(mut step) = self.steps.pop_front() {
                step(ctx);
            }
        }
    }

    /// Names a scene, so a test can tell which one is current.
    struct Level(u32);

    impl crate::SceneState for Level {}

    fn current_level<G: GameState>(core: &Core<G>) -> u32 {
        core.current_scene().scene_state::<Level>().unwrap().0
    }

    const SHARED: TextureAsset = TextureAsset::from_manifest("shared", "shared.png");
    const OLD_ONLY: TextureAsset = TextureAsset::from_manifest("old_only", "old_only.png");
    const NEW_ONLY: TextureAsset = TextureAsset::from_manifest("new_only", "new_only.png");

    fn is_unloaded(state: TextureState) -> bool {
        matches!(state, TextureState::Unloaded)
    }

    #[test]
    fn a_switch_applies_when_the_tick_ends_and_frees_only_what_the_new_scene_doesnt_hold() {
        let loaded = Rc::new(RefCell::new(Vec::new()));
        let loaded_in_step = loaded.clone();
        let steps: Vec<Step> = vec![
            Box::new(move |ctx| {
                ctx.begin_next_scene(Scene::new(Level(2))).unwrap();
                let shared = ctx.load_texture_into_next_scene_scope(SHARED).unwrap();
                let new_only = ctx.load_texture_into_next_scene_scope(NEW_ONLY).unwrap();
                ctx.next_scene_mut()
                    .unwrap()
                    .spawn_immediate(DrawnWalker, Parent::Root, Transform::IDENTITY)
                    .unwrap();
                loaded_in_step.borrow_mut().extend([shared, new_only]);
            }),
            Box::new(|ctx| ctx.switch_to_next_scene_deferred().unwrap()),
        ];
        let mut core = Core::new(
            ScriptedGameState::new(steps),
            Scene::new(Level(1)),
            settings(InputBindings::none()),
        );
        let shared = core.load_texture_into_current_scene_scope(SHARED);
        let old_only = core.load_texture_into_current_scene_scope(OLD_ONLY);

        core.tick();
        // The next scene is built, but the old one is still current.
        assert_eq!(current_level(&core), 1);
        assert_eq!(core.next_scene().unwrap().len(), 1);
        assert_eq!(loaded.borrow()[0], shared);

        core.tick();
        assert_eq!(current_level(&core), 2);
        assert_eq!(core.current_scene().len(), 1);
        assert!(core.next_scene().is_none());
        assert!(is_unloaded(core.texture_state(old_only)));
        assert!(!is_unloaded(core.texture_state(shared)));
        assert!(!is_unloaded(core.texture_state(loaded.borrow()[1])));
    }

    #[test]
    fn a_discard_drops_the_next_scene_and_frees_what_only_it_held() {
        let new_only = Rc::new(RefCell::new(None));
        let new_only_in_step = new_only.clone();
        let steps: Vec<Step> = vec![Box::new(move |ctx| {
            ctx.begin_next_scene(Scene::new(Level(2))).unwrap();
            ctx.load_texture_into_next_scene_scope(SHARED).unwrap();
            *new_only_in_step.borrow_mut() =
                Some(ctx.load_texture_into_next_scene_scope(NEW_ONLY).unwrap());
            ctx.discard_next_scene_deferred().unwrap();
            // Still there until the tick ends.
            assert!(ctx.next_scene().is_some());
        })];
        let mut core = Core::new(
            ScriptedGameState::new(steps),
            Scene::new(Level(1)),
            settings(InputBindings::none()),
        );
        let shared = core.load_texture_into_current_scene_scope(SHARED);
        core.tick();
        assert_eq!(current_level(&core), 1);
        assert!(core.next_scene().is_none());
        assert!(!is_unloaded(core.texture_state(shared)));
        assert!(is_unloaded(core.texture_state(new_only.borrow().unwrap())));
    }

    #[test]
    fn scene_change_mistakes_are_typed_errors() {
        use crate::{BeginNextSceneError, NoNextSceneError, QueueSceneChangeError};
        let steps: Vec<Step> = vec![Box::new(|ctx| {
            assert_eq!(
                ctx.switch_to_next_scene_deferred(),
                Err(QueueSceneChangeError::NoNextScene)
            );
            assert_eq!(
                ctx.load_texture_into_next_scene_scope(SHARED),
                Err(NoNextSceneError)
            );
            assert_eq!(ctx.next_scene_scope_load_counts(), Err(NoNextSceneError));
            ctx.begin_next_scene(Scene::new(())).unwrap();
            assert_eq!(
                ctx.begin_next_scene(Scene::new(())),
                Err(BeginNextSceneError::NextSceneExists)
            );
            ctx.switch_to_next_scene_deferred().unwrap();
            assert_eq!(
                ctx.discard_next_scene_deferred(),
                Err(QueueSceneChangeError::ChangeAlreadyQueued)
            );
        })];
        let mut core = new_core(ScriptedGameState::new(steps));
        core.tick();
        assert!(core.next_scene().is_none());
    }

    /// A door the player walked through: it tells the scene state, which
    /// switches scenes.
    struct Door;

    impl GameObject for Door {
        fn tick(&mut self, ctx: &mut crate::TickContext) {
            ctx.scene_state_mut::<LevelWithExit>().unwrap().exit_reached = true;
        }
    }

    struct LevelWithExit {
        exit_reached: bool,
    }

    impl crate::SceneState for LevelWithExit {
        fn tick(&mut self, ctx: &mut crate::SceneStateTickContext) {
            if self.exit_reached && ctx.next_scene().is_none() {
                ctx.begin_next_scene(Scene::new(Level(2))).unwrap();
                ctx.switch_to_next_scene_deferred().unwrap();
            }
        }
    }

    #[test]
    fn an_object_changes_scenes_through_the_scene_state() {
        let mut core = Core::new(
            RecordTicks::default(),
            Scene::new(LevelWithExit {
                exit_reached: false,
            }),
            settings(InputBindings::none()),
        );
        core.current_scene_mut()
            .spawn_immediate(Door, Parent::Root, Transform::IDENTITY)
            .unwrap();
        // The door ticks after the scene state, so the scene state sees it
        // the tick after.
        core.tick();
        assert!(
            core.current_scene()
                .scene_state::<LevelWithExit>()
                .is_some()
        );
        core.tick();
        assert_eq!(current_level(&core), 2);
        // The game state survived the change.
        core.tick();
        assert_eq!(core.game_state().seen, [0, 1, 2]);
    }

    /// Shows one texture, swapping it for another on its first tick.
    struct PictureFrame {
        showing: TextureHandle,
        swapped: bool,
    }

    impl GameObject for PictureFrame {
        fn tick(&mut self, ctx: &mut crate::TickContext) {
            if !self.swapped {
                let next = ctx.load_texture_into_scene_scope(NEW_ONLY);
                ctx.release_texture_from_scene_scope_deferred(self.showing)
                    .unwrap();
                self.showing = next;
                self.swapped = true;
            }
        }
    }

    #[test]
    fn an_object_swapping_textures_frees_only_its_own_load_when_the_tick_ends() {
        let mut core = new_core(());
        // Two frames show the same photo; one swaps it.
        let photo = core.load_texture_into_current_scene_scope(OLD_ONLY);
        core.load_texture_into_current_scene_scope(OLD_ONLY);
        core.current_scene_mut()
            .spawn_immediate(
                PictureFrame {
                    showing: photo,
                    swapped: false,
                },
                Parent::Root,
                Transform::IDENTITY,
            )
            .unwrap();
        core.tick();
        // The other frame's load keeps it.
        assert!(!is_unloaded(core.texture_state(photo)));

        // A frame that held the only load frees it.
        let mut core = new_core(());
        let photo = core.load_texture_into_current_scene_scope(OLD_ONLY);
        core.current_scene_mut()
            .spawn_immediate(
                PictureFrame {
                    showing: photo,
                    swapped: false,
                },
                Parent::Root,
                Transform::IDENTITY,
            )
            .unwrap();
        core.tick();
        assert!(is_unloaded(core.texture_state(photo)));
    }
}
