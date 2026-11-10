use std::any::Any;

use crate::assets::{
    AssetScope, AssetStore, ReleaseTextureError, SceneFileAsset, ScopeLoadCounts, TextureAsset,
    TextureHandle, TextureState,
};
use crate::frame::FrameContext;
use crate::game_object::GameObject;
use crate::game_state::GameState;
use crate::handle::{Handle, ObjectHandle};
use crate::input::InputView;
use crate::math::Transform;
use crate::scene::{Parent, Scene, TypedHandleError};
use crate::scene_change::{
    BeginNextSceneError, NoNextSceneError, QueueSceneChangeError, QueuedSceneChange, SceneChange,
};
use crate::scene_file::SceneFileError;
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
/// by handle, queues spawns and despawns, and changes scenes (ADR 0025).
/// It is separate from the object [`TickContext`](crate::TickContext)
/// because scene state has no transform of its own.
///
/// Objects can't change scenes. An object that ends a level, such as a
/// door, sets a value on the scene state, and the scene state switches.
pub struct SceneStateTickContext<'a> {
    game_time: &'a GameTime,
    tick_input: &'a InputView,
    game_state: &'a mut dyn Any,
    current_scene: &'a mut Scene,
    assets: &'a mut AssetStore,
    scene_change: &'a mut SceneChange,
}

impl<'a> SceneStateTickContext<'a> {
    pub(crate) fn new(
        game_time: &'a GameTime,
        tick_input: &'a InputView,
        game_state: &'a mut dyn Any,
        current_scene: &'a mut Scene,
        assets: &'a mut AssetStore,
        scene_change: &'a mut SceneChange,
    ) -> SceneStateTickContext<'a> {
        SceneStateTickContext {
            game_time,
            tick_input,
            game_state,
            current_scene,
            assets,
            scene_change,
        }
    }

    /// Starts loading `asset` and holds it until the game ends. Returns its
    /// handle right away, while the file loads in the background.
    ///
    /// Loading an asset that is already loaded or loading gives the same
    /// handle, and the game holds it too.
    pub fn load_texture_into_game_scope(&mut self, asset: TextureAsset) -> TextureHandle {
        self.assets.load_texture(asset, AssetScope::Game)
    }

    /// Starts loading `asset` and holds it until this scene ends. Returns
    /// its handle right away, while the file loads in the background.
    ///
    /// Loading an asset that is already loaded or loading gives the same
    /// handle, and this scene holds it too.
    pub fn load_texture_into_scene_scope(&mut self, asset: TextureAsset) -> TextureHandle {
        let scope = self.assets.current_scene_scope();
        self.assets.load_texture(asset, scope)
    }

    /// Queues one of the game's loads of `texture` to be released when the
    /// tick ends. See
    /// [`GameStateTickContext::release_texture_from_game_scope_deferred`](crate::GameStateTickContext::release_texture_from_game_scope_deferred).
    ///
    /// # Errors
    ///
    /// [`ReleaseTextureError::NotHeldByScope`] if the game has no load of
    /// it left to release, counting releases already queued.
    pub fn release_texture_from_game_scope_deferred(
        &mut self,
        texture: TextureHandle,
    ) -> Result<(), ReleaseTextureError> {
        self.assets.queue_release(texture, AssetScope::Game)
    }

    /// Queues one of this scene's loads of `texture` to be released when
    /// the tick ends (ADR 0025). Once every load by every scope is released
    /// or its scope has ended, the texture is freed.
    ///
    /// # Errors
    ///
    /// [`ReleaseTextureError::NotHeldByScope`] if this scene has no load of
    /// it left to release, counting releases already queued.
    pub fn release_texture_from_scene_scope_deferred(
        &mut self,
        texture: TextureHandle,
    ) -> Result<(), ReleaseTextureError> {
        let scope = self.assets.current_scene_scope();
        self.assets.queue_release(texture, scope)
    }

    /// Makes `scene` the next scene, right away. See
    /// [`GameStateTickContext::begin_next_scene`](crate::GameStateTickContext::begin_next_scene).
    ///
    /// # Errors
    ///
    /// [`BeginNextSceneError::NextSceneExists`] if there already is a next
    /// scene. `scene` is dropped.
    pub fn begin_next_scene(&mut self, scene: Scene) -> Result<(), BeginNextSceneError> {
        self.scene_change.begin_next_scene(scene, self.assets)
    }

    /// Begins the next scene from a scene file (ADR 0026). The file is
    /// read in the background while the current scene keeps running.
    ///
    /// The engine builds the scene when a tick ends after the file is read,
    /// and starts loading every asset it refers to into the next scene's
    /// scope. Until then, [`next_scene`](Self::next_scene) is `None` and
    /// the scene can't be switched to. Wait until
    /// [`next_scene_scope_load_counts`](Self::next_scene_scope_load_counts)
    /// reports [`all_finished`](ScopeLoadCounts::all_finished), which
    /// counts the file too, then switch.
    ///
    /// The engine never switches on its own. If the file fails to load or
    /// build, [`next_scene_file_error`](Self::next_scene_file_error) says
    /// why and the game discards it. The current scene keeps running.
    ///
    /// # Errors
    ///
    /// [`BeginNextSceneError::NextSceneExists`] if there already is a next
    /// scene, built or not. Nothing is read.
    pub fn begin_next_scene_from_file(
        &mut self,
        scene_file: SceneFileAsset,
    ) -> Result<(), BeginNextSceneError> {
        self.scene_change
            .begin_next_scene_from_file(scene_file, self.assets)
    }

    /// Whether there is a next scene, including one whose file is still
    /// loading or failed.
    pub fn has_next_scene(&self) -> bool {
        self.scene_change.has_next_scene()
    }

    /// The next scene, or `None` if there is none or its file isn't built
    /// yet.
    pub fn next_scene(&self) -> Option<&Scene> {
        self.scene_change.next_scene()
    }

    /// Why the next scene's file couldn't be loaded or built, or `None` if
    /// it hasn't failed (ADR 0026). The game discards a failed next scene.
    pub fn next_scene_file_error(&self) -> Option<&SceneFileError> {
        self.scene_change.next_scene_file_error()
    }

    /// The next scene, to build it, such as with
    /// [`Scene::spawn_immediate`], or `None` if there is none. It doesn't
    /// tick until it becomes the current scene.
    pub fn next_scene_mut(&mut self) -> Option<&mut Scene> {
        self.scene_change.next_scene_mut()
    }

    /// Starts loading `asset` and holds it until the next scene ends. See
    /// [`GameStateTickContext::load_texture_into_next_scene_scope`](crate::GameStateTickContext::load_texture_into_next_scene_scope).
    ///
    /// # Errors
    ///
    /// [`NoNextSceneError`] if there is no next scene. Nothing is loaded.
    pub fn load_texture_into_next_scene_scope(
        &mut self,
        asset: TextureAsset,
    ) -> Result<TextureHandle, NoNextSceneError> {
        let scope = self.assets.next_scene_scope().ok_or(NoNextSceneError)?;
        Ok(self.assets.load_texture(asset, scope))
    }

    /// Queues one of the next scene's loads of `texture` to be released
    /// when the tick ends.
    ///
    /// # Errors
    ///
    /// [`ReleaseTextureError::NotHeldByScope`] if the next scene has no
    /// load of it left to release, or there is no next scene.
    pub fn release_texture_from_next_scene_scope_deferred(
        &mut self,
        texture: TextureHandle,
    ) -> Result<(), ReleaseTextureError> {
        let scope = self
            .assets
            .next_scene_scope()
            .ok_or(ReleaseTextureError::NotHeldByScope)?;
        self.assets.queue_release(texture, scope)
    }

    /// How the assets the next scene holds are doing. A next scene from a
    /// scene file counts the file too.
    ///
    /// # Errors
    ///
    /// [`NoNextSceneError`] if there is no next scene.
    pub fn next_scene_scope_load_counts(&self) -> Result<ScopeLoadCounts, NoNextSceneError> {
        self.scene_change.next_scene_scope_load_counts(self.assets)
    }

    /// Queues a switch to the next scene for when the tick ends. This
    /// scene, its scene state, and its objects are dropped then. See
    /// [`GameStateTickContext::switch_to_next_scene_deferred`](crate::GameStateTickContext::switch_to_next_scene_deferred).
    ///
    /// # Errors
    ///
    /// - [`QueueSceneChangeError::NoNextScene`] if there is no next scene.
    /// - [`QueueSceneChangeError::ChangeAlreadyQueued`] if a switch or
    ///   discard is already queued this tick.
    /// - [`QueueSceneChangeError::NextSceneNotBuilt`] if the next scene's
    ///   file is still loading or failed.
    pub fn switch_to_next_scene_deferred(&mut self) -> Result<(), QueueSceneChangeError> {
        self.scene_change
            .queue(QueuedSceneChange::SwitchToNextScene)
    }

    /// Queues the next scene to be dropped when the tick ends, ending its
    /// scope.
    ///
    /// # Errors
    ///
    /// - [`QueueSceneChangeError::NoNextScene`] if there is no next scene.
    /// - [`QueueSceneChangeError::ChangeAlreadyQueued`] if a switch or
    ///   discard is already queued this tick.
    pub fn discard_next_scene_deferred(&mut self) -> Result<(), QueueSceneChangeError> {
        self.scene_change.queue(QueuedSceneChange::DiscardNextScene)
    }

    /// Where `texture` is in loading.
    pub fn texture_state(&self, texture: TextureHandle) -> TextureState<'_> {
        self.assets.texture_state(texture)
    }

    /// Textures whose loads finished, successfully or not, before this
    /// tick started (ADR 0015). Each load is reported in exactly one tick.
    pub fn texture_loads_finished_before_this_tick(&self) -> &[TextureHandle] {
        self.assets.finished_this_tick()
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
                asset_folder: std::path::PathBuf::new(),
                asset_manifest: crate::assets::AssetManifest::EMPTY,
                scene_types: crate::scene_file::SceneTypeRegistry::new(),
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
                asset_folder: std::path::PathBuf::new(),
                asset_manifest: crate::assets::AssetManifest::EMPTY,
                scene_types: crate::scene_file::SceneTypeRegistry::new(),
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
