use std::any::Any;

use crate::assets::{AssetScope, AssetStore, TextureAsset, TextureHandle, TextureState};
use crate::frame::FrameContext;
use crate::input::InputView;
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
/// input, and the asset store. Game state lasts the whole run, so it loads
/// into the game's scope only.
pub struct GameStateTickContext<'a> {
    game_time: &'a GameTime,
    tick_input: &'a InputView,
    assets: &'a mut AssetStore,
}

impl<'a> GameStateTickContext<'a> {
    pub(crate) fn new(
        game_time: &'a GameTime,
        tick_input: &'a InputView,
        assets: &'a mut AssetStore,
    ) -> GameStateTickContext<'a> {
        GameStateTickContext {
            game_time,
            tick_input,
            assets,
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
