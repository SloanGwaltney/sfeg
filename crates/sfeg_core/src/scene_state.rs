use std::any::Any;

use crate::frame::FrameContext;
use crate::game_object::GameObject;
use crate::game_state::GameState;
use crate::handle::{Handle, ObjectHandle};
use crate::input::InputView;
use crate::math::Transform;
use crate::scene::{Parent, Scene, TypedHandleError};
use crate::time::GameTime;

/// The scene-wide state: one game-defined struct per scene, created with
/// its scene and dropped with it (ADR 0012).
///
/// It holds what belongs to one scene but isn't a thing in it, such as a
/// level's score, objectives, or wave spawner. What lasts the whole run
/// belongs in the [`GameState`] instead.
///
/// Scene state is optional. A scene without one uses `()`.
pub trait SceneState: Any {
    /// Runs once per fixed tick, after the game state and before every
    /// object, and changes the game (ADR 0008, 0012).
    ///
    /// Spawns and despawns it queues apply when the tick ends, like an
    /// object's, so objects it spawns first tick in the next tick.
    fn tick(&mut self, ctx: &mut SceneStateTickContext) {
        let _ = ctx;
    }

    /// Runs once per rendered frame, after the game state's `frame` and
    /// before every object's, and only updates presentation (ADR 0008,
    /// 0012). See [`GameObject::frame`].
    fn frame(&mut self, ctx: &mut FrameContext) {
        let _ = ctx;
    }
}

impl SceneState for () {}

/// What the scene state's tick can see and do (ADR 0012).
///
/// It reaches the game state by type, objects and their local transforms
/// by handle, and queues spawns and despawns. It is separate from the
/// object [`TickContext`](crate::TickContext) because scene state has no
/// transform of its own.
pub struct SceneStateTickContext<'a> {
    game_time: &'a GameTime,
    tick_input: &'a InputView,
    game_state: &'a mut dyn Any,
    current_scene: &'a mut Scene,
}

impl<'a> SceneStateTickContext<'a> {
    pub(crate) fn new(
        game_time: &'a GameTime,
        tick_input: &'a InputView,
        game_state: &'a mut dyn Any,
        current_scene: &'a mut Scene,
    ) -> SceneStateTickContext<'a> {
        SceneStateTickContext {
            game_time,
            tick_input,
            game_state,
            current_scene,
        }
    }

    /// Game time at the start of this tick.
    pub fn game_time(&self) -> &GameTime {
        self.game_time
    }

    /// The tick view of the input actions: "just pressed" means since the
    /// last tick (ADR 0008).
    pub fn tick_input(&self) -> &InputView {
        self.tick_input
    }

    /// The game state, or `None` if it isn't a `G`.
    pub fn game_state<G: GameState>(&self) -> Option<&G> {
        self.game_state.downcast_ref()
    }

    /// The game state, to change it, or `None` if it isn't a `G`.
    pub fn game_state_mut<G: GameState>(&mut self) -> Option<&mut G> {
        self.game_state.downcast_mut()
    }

    /// An object, or `None` if it is gone or hasn't spawned yet.
    pub fn get<T: GameObject>(&self, handle: Handle<T>) -> Option<&T> {
        self.current_scene.get(handle)
    }

    /// An object, or `None` if it is gone or hasn't spawned yet.
    pub fn get_mut<T: GameObject>(&mut self, handle: Handle<T>) -> Option<&mut T> {
        self.current_scene.get_mut(handle)
    }

    /// An object's local transform, or `None` if it is gone or hasn't
    /// spawned yet.
    pub fn local_of(&self, handle: impl Into<ObjectHandle>) -> Option<&Transform> {
        self.current_scene.local_of(handle)
    }

    /// An object's local transform, to change it, or `None` if it is gone
    /// or hasn't spawned yet. The change is interpolated, as with
    /// [`TickContext::local_mut`](crate::TickContext::local_mut).
    pub fn local_of_mut(&mut self, handle: impl Into<ObjectHandle>) -> Option<&mut Transform> {
        self.current_scene.local_of_mut(handle)
    }

    /// Whether the object `handle` refers to is in the scene. An object
    /// despawned this tick is still alive until the tick ends.
    pub fn is_alive(&self, handle: impl Into<ObjectHandle>) -> bool {
        self.current_scene.is_alive(handle)
    }

    /// Checks that `handle` refers to a live `T`. See [`Scene::typed`].
    ///
    /// # Errors
    ///
    /// - [`TypedHandleError::Gone`] if the object is gone or hasn't spawned
    ///   yet.
    /// - [`TypedHandleError::WrongType`] if the object isn't a `T`.
    pub fn typed<T: GameObject>(
        &self,
        handle: ObjectHandle,
    ) -> Result<Handle<T>, TypedHandleError> {
        self.current_scene.typed(handle)
    }

    /// Queues `object` to spawn under `parent`, with the transform `local`
    /// relative to it, when the tick ends, and returns its handle now. See
    /// [`TickContext::spawn_deferred`](crate::TickContext::spawn_deferred).
    pub fn spawn_deferred<T: GameObject>(
        &mut self,
        object: T,
        parent: impl Into<Parent>,
        local: Transform,
    ) -> Handle<T> {
        self.current_scene.queue_spawn(object, parent.into(), local)
    }

    /// Queues the object `handle` refers to to despawn with all of its
    /// children when the tick ends. See
    /// [`TickContext::despawn_deferred`](crate::TickContext::despawn_deferred).
    pub fn despawn_deferred(&mut self, handle: impl Into<ObjectHandle>) {
        self.current_scene
            .queue_despawn_with_children(handle.into());
    }

    /// Like [`SceneStateTickContext::despawn_deferred`], but first moves
    /// the object's children to the root of the scene, keeping where they
    /// are in the world. See
    /// [`TickContext::despawn_deferred_keep_children`](crate::TickContext::despawn_deferred_keep_children).
    pub fn despawn_deferred_keep_children(&mut self, handle: impl Into<ObjectHandle>) {
        self.current_scene
            .queue_despawn_keep_children(handle.into());
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::input::InputBindings;
    use crate::scene::TransformInterpolation;
    use crate::time::{MaxTicksPerFrame, TickRate};
    use crate::{Core, CoreSettings, GameStateTickContext, TickContext};

    /// Game state that records what ran, in order.
    #[derive(Default)]
    struct Log(Vec<&'static str>);

    impl GameState for Log {
        fn tick(&mut self, _ctx: &mut GameStateTickContext) {
            self.0.push("game state tick");
        }

        fn frame(&mut self, _ctx: &mut FrameContext) {
            self.0.push("game state frame");
        }
    }

    fn log(game_state: Option<&mut Log>, entry: &'static str) {
        game_state.expect("the game state is a Log").0.push(entry);
    }

    #[derive(Default)]
    struct Level {
        score: u32,
        /// Spawns one `Spawned` on its first tick.
        spawned: bool,
    }

    impl SceneState for Level {
        fn tick(&mut self, ctx: &mut SceneStateTickContext) {
            log(ctx.game_state_mut::<Log>(), "scene state tick");
            self.score += 10;
            if !self.spawned {
                self.spawned = true;
                let spawned = ctx.spawn_deferred(Spawned, Parent::Root, Transform::IDENTITY);
                assert!(!ctx.is_alive(spawned));
            }
        }
    }

    /// Scores a point every tick.
    struct Coin;

    impl GameObject for Coin {
        fn tick(&mut self, ctx: &mut TickContext) {
            log(ctx.game_state_mut::<Log>(), "object tick");
            let level = ctx
                .scene_state_mut::<Level>()
                .expect("the scene state is a Level");
            level.score += 1;
        }
    }

    struct Spawned;

    impl GameObject for Spawned {
        fn tick(&mut self, ctx: &mut TickContext) {
            log(ctx.game_state_mut::<Log>(), "spawned tick");
        }
    }

    fn level_core() -> Core<Log> {
        let mut scene = Scene::new(Level::default());
        scene
            .spawn_immediate(Coin, Parent::Root, Transform::IDENTITY)
            .unwrap();
        Core::new(
            Log::default(),
            scene,
            CoreSettings {
                tick_rate: TickRate::DEFAULT,
                max_ticks_per_frame: MaxTicksPerFrame::DEFAULT,
                transform_interpolation: TransformInterpolation::Enabled,
                input_bindings: InputBindings::none(),
            },
        )
    }

    #[test]
    fn each_tick_runs_game_state_then_scene_state_then_objects() {
        let mut core = level_core();
        core.tick();
        core.tick();
        assert_eq!(
            core.game_state().0,
            [
                "game state tick",
                "scene state tick",
                "object tick",
                "game state tick",
                "scene state tick",
                "object tick",
                // Spawned by the scene state in the first tick.
                "spawned tick",
            ]
        );
    }

    #[test]
    fn objects_change_the_scene_state_in_the_same_tick() {
        let mut core = level_core();
        core.tick();
        core.tick();
        // 10 from the scene state and 1 from the coin, each tick.
        let level = core.current_scene().scene_state::<Level>().unwrap();
        assert_eq!(level.score, 22);
    }

    #[test]
    fn each_frame_runs_game_state_then_scene_state_then_objects() {
        struct FrameLevel;
        impl SceneState for FrameLevel {
            fn frame(&mut self, _ctx: &mut FrameContext) {
                FRAMES.with(|frames| frames.borrow_mut().push("scene state frame"));
            }
        }
        struct FrameObject;
        impl GameObject for FrameObject {
            fn frame(&mut self, _ctx: &mut FrameContext) {
                FRAMES.with(|frames| frames.borrow_mut().push("object frame"));
            }
        }
        struct FrameGame;
        impl GameState for FrameGame {
            fn frame(&mut self, _ctx: &mut FrameContext) {
                FRAMES.with(|frames| frames.borrow_mut().push("game state frame"));
            }
        }
        thread_local! {
            static FRAMES: std::cell::RefCell<Vec<&'static str>> = const { std::cell::RefCell::new(Vec::new()) };
        }

        let mut scene = Scene::new(FrameLevel);
        scene
            .spawn_immediate(FrameObject, Parent::Root, Transform::IDENTITY)
            .unwrap();
        let mut core = Core::new(
            FrameGame,
            scene,
            CoreSettings {
                tick_rate: TickRate::DEFAULT,
                max_ticks_per_frame: MaxTicksPerFrame::DEFAULT,
                transform_interpolation: TransformInterpolation::Enabled,
                input_bindings: InputBindings::none(),
            },
        );
        core.advance(Duration::ZERO);
        FRAMES.with(|frames| {
            assert_eq!(
                *frames.borrow(),
                ["game state frame", "scene state frame", "object frame"]
            );
        });
    }

    #[test]
    fn lookups_of_the_wrong_type_are_none() {
        struct Checker;
        impl GameObject for Checker {
            fn tick(&mut self, ctx: &mut TickContext) {
                assert!(ctx.game_state::<()>().is_none());
                assert!(ctx.game_state::<Log>().is_some());
                assert!(ctx.scene_state::<()>().is_none());
                assert!(ctx.scene_state_mut::<Level>().is_some());
            }
        }
        let mut core = level_core();
        core.current_scene_mut()
            .spawn_immediate(Checker, Parent::Root, Transform::IDENTITY)
            .unwrap();
        core.tick();
        assert!(core.current_scene().scene_state::<()>().is_none());
    }
}
