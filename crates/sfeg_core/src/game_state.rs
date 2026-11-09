use std::any::Any;

use crate::assets::{
    AssetScope, AssetStore, ReleaseTextureError, ScopeLoadCounts, TextureAsset, TextureHandle,
    TextureState,
};
use crate::frame::FrameContext;
use crate::input::InputView;
use crate::scene::Scene;
use crate::scene_change::{
    BeginNextSceneError, NoNextSceneError, QueueSceneChangeError, QueuedSceneChange, SceneChange,
};
use crate::time::GameTime;

/// The game-wide state: one game-defined struct that lasts for the whole run
/// (ADR 0012).
///
/// What belongs to one scene, such as a level's score, belongs in the
/// [`SceneState`](crate::SceneState) instead. Objects and scene state look
/// game state up by type, such as `ctx.game_state::<MyGame>()`.
///
/// Game state is optional. A game without one uses `()`.
pub trait GameState: Any {
    /// Runs once per fixed tick, before the scene state and every object,
    /// and changes the game (ADR 0008, 0012).
    fn tick(&mut self, ctx: &mut GameStateTickContext) {
        let _ = ctx;
    }

    /// Runs once per rendered frame, before the scene state's and every
    /// object's `frame`, and only
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
/// because game state has no transform (ADR 0012). It holds game time,
/// input, and the asset store, and changes scenes (ADR 0025).
///
/// Game state lasts the whole run, so it loads into the game's scope, or
/// into the next scene's while it builds one. It never reaches the current
/// scene.
pub struct GameStateTickContext<'a> {
    game_time: &'a GameTime,
    tick_input: &'a InputView,
    assets: &'a mut AssetStore,
    scene_change: &'a mut SceneChange,
}

impl<'a> GameStateTickContext<'a> {
    pub(crate) fn new(
        game_time: &'a GameTime,
        tick_input: &'a InputView,
        assets: &'a mut AssetStore,
        scene_change: &'a mut SceneChange,
    ) -> GameStateTickContext<'a> {
        GameStateTickContext {
            game_time,
            tick_input,
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

    /// Queues one of the game's loads of `texture` to be released when the
    /// tick ends (ADR 0025). Once every load by every scope is released or
    /// its scope has ended, the texture is freed.
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

    /// Makes `scene` the next scene, right away (ADR 0025). The current
    /// scene keeps running while the next one is built: load assets into
    /// its scope with
    /// [`GameStateTickContext::load_texture_into_next_scene_scope`], and
    /// spawn objects into it through
    /// [`GameStateTickContext::next_scene_mut`]. Then switch to it or
    /// discard it.
    ///
    /// # Errors
    ///
    /// [`BeginNextSceneError::NextSceneExists`] if there already is a next
    /// scene. `scene` is dropped.
    pub fn begin_next_scene(&mut self, scene: Scene) -> Result<(), BeginNextSceneError> {
        self.scene_change.begin_next_scene(scene, self.assets)
    }

    /// The next scene, or `None` if there is none.
    pub fn next_scene(&self) -> Option<&Scene> {
        self.scene_change.next_scene()
    }

    /// The next scene, to build it, such as with
    /// [`Scene::spawn_immediate`], or `None` if there is none. It doesn't
    /// tick until it becomes the current scene.
    pub fn next_scene_mut(&mut self) -> Option<&mut Scene> {
        self.scene_change.next_scene_mut()
    }

    /// Starts loading `asset` and holds it until the next scene ends.
    /// Returns its handle right away, while the file loads in the
    /// background.
    ///
    /// Loading an asset that is already loaded or loading gives the same
    /// handle, and the next scene holds it too. Switching keeps what the
    /// next scene holds, even if the current scene held it as well.
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
    /// when the tick ends. See
    /// [`GameStateTickContext::release_texture_from_game_scope_deferred`].
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

    /// How the textures the next scene holds are doing, such as to wait
    /// until [`ScopeLoadCounts::all_finished`] before switching.
    ///
    /// # Errors
    ///
    /// [`NoNextSceneError`] if there is no next scene.
    pub fn next_scene_scope_load_counts(&self) -> Result<ScopeLoadCounts, NoNextSceneError> {
        let scope = self.assets.next_scene_scope().ok_or(NoNextSceneError)?;
        Ok(self.assets.scope_load_counts(scope))
    }

    /// Queues a switch to the next scene for when the tick ends (ADR 0025).
    ///
    /// Then the current scene, its scene state, and its objects are
    /// dropped, and its scope ends: textures no other scope holds are
    /// freed. The next scene becomes the current one and first ticks in the
    /// next tick. The game state is kept.
    ///
    /// The switch doesn't wait for loads. Textures still loading draw as
    /// placeholders until they are ready.
    ///
    /// # Errors
    ///
    /// - [`QueueSceneChangeError::NoNextScene`] if there is no next scene.
    /// - [`QueueSceneChangeError::ChangeAlreadyQueued`] if a switch or
    ///   discard is already queued this tick.
    pub fn switch_to_next_scene_deferred(&mut self) -> Result<(), QueueSceneChangeError> {
        self.scene_change
            .queue(QueuedSceneChange::SwitchToNextScene)
    }

    /// Queues the next scene to be dropped when the tick ends, ending its
    /// scope: textures no other scope holds are freed (ADR 0025).
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
