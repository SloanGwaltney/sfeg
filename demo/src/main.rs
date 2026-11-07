//! A demo game built on SFEG.
//!
//! For now it runs headless: it ticks the engine a few times, greets each
//! tick, and moves a walker across the scene with a hat on its head.

use sfeg::math::{Transform, Vector3};
use sfeg::{
    Core, GameObject, GameState, GameStateTickContext, Parent, Scene, TickContext, TickRate,
};

/// The game-wide state.
#[derive(Default)]
struct HelloGame {
    greetings: Vec<String>,
}

impl GameState for HelloGame {
    fn tick(&mut self, ctx: &mut GameStateTickContext) {
        let game_time = ctx.game_time();
        let greeting = format!(
            "Hello from tick {} ({:.3}s)",
            game_time.tick_count(),
            game_time.elapsed_seconds()
        );
        println!("{greeting}");
        self.greetings.push(greeting);
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
    let core = Core::new(HelloGame::default(), scene, TickRate::DEFAULT);
    (core, walker, hat)
}

fn main() {
    let (mut core, walker, hat) = setup();
    for _ in 0..3 {
        core.tick();
        let scene = core.current_scene();
        if let (Some(walker), Some(hat)) = (
            scene.world_position_of(walker),
            scene.world_position_of(hat),
        ) {
            println!(
                "  walker at ({:.2}, {:.2}), hat at ({:.2}, {:.2})",
                walker.x(),
                walker.y(),
                hat.x(),
                hat.y()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greets_once_per_tick() {
        let (mut core, _, _) = setup();
        for _ in 0..2 {
            core.tick();
        }
        assert_eq!(
            core.game_state().greetings,
            ["Hello from tick 0 (0.000s)", "Hello from tick 1 (0.017s)"]
        );
    }

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
