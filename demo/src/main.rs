//! A demo game built on SFEG.
//!
//! It opens an empty window and runs the game in it. Nothing is drawn yet:
//! a walker with a hat on its head moves across the scene unseen, and the
//! game state says hello once per second of game time.

use sfeg::math::{Transform, Vector3};
use sfeg::{
    Core, GameObject, GameState, GameStateTickContext, MaxTicksPerFrame, Parent, Scene,
    TickContext, TickRate, WindowSettings,
};

/// The game-wide state.
struct HelloGame;

impl GameState for HelloGame {
    fn tick(&mut self, ctx: &mut GameStateTickContext) {
        let game_time = ctx.game_time();
        if game_time.tick_count() % u64::from(game_time.tick_rate().hz()) == 0 {
            println!("Hello at {:.0}s of game time", game_time.elapsed_seconds());
        }
    }
}

/// Walks along +x at a fixed speed.
struct Walker {
    /// Units per second.
    speed: f32,
}

impl GameObject for Walker {
    fn tick(&mut self, ctx: &mut TickContext) {
        let step = Vector3::UNIT_X * self.speed * ctx.game_time().dt();
        let position = ctx.local().position() + step;
        // Only fails for a non-finite position, which a finite speed can't
        // produce in a demo this short.
        ctx.local_mut()
            .set_position(position)
            .expect("walker position stays finite");
    }
}

/// Sits on whatever it is parented to. It has no behavior of its own: it
/// moves because its parent does.
struct Hat;

impl GameObject for Hat {}

/// Builds the game and its first scene.
fn setup() -> (Core<HelloGame>, sfeg::Handle<Walker>, sfeg::Handle<Hat>) {
    let mut scene = Scene::new();
    let walker = scene
        .spawn_immediate(Walker { speed: 6.0 }, Parent::Root, Transform::IDENTITY)
        .expect("the root is always there");
    let on_head = Transform::from_position(Vector3::new(0.0, 1.0, 0.0)).expect("finite");
    let hat = scene
        .spawn_immediate(Hat, walker, on_head)
        .expect("the walker was just spawned");
    let core = Core::new(
        HelloGame,
        scene,
        TickRate::DEFAULT,
        MaxTicksPerFrame::DEFAULT,
    );
    (core, walker, hat)
}

fn main() -> Result<(), sfeg::RunError> {
    let (core, _, _) = setup();
    sfeg::run(
        core,
        WindowSettings {
            title: "SFEG demo".to_string(),
            logical_width: 1280,
            logical_height: 720,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walker_covers_its_speed_in_one_second() {
        let (mut core, walker, hat) = setup();
        for _ in 0..60 {
            core.tick();
        }
        let scene = core.current_scene();
        let x = scene.local_of(walker).unwrap().position().x();
        assert!((x - 6.0).abs() < 1e-4, "{x}");
        // The hat rode along without moving itself.
        let hat_world = scene.world_position_of(hat).unwrap();
        assert!((hat_world.x() - 6.0).abs() < 1e-4, "{hat_world:?}");
        assert_eq!(hat_world.y(), 1.0);
        assert_eq!(scene.local_of(hat).unwrap().position().x(), 0.0);
    }
}
