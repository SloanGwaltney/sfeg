//! A demo game built on SFEG.
//!
//! For now it runs headless: it ticks the engine a few times, greets each
//! tick, and moves a walker across the scene.

use sfeg::math::{Transform, Vector3};
use sfeg::{Core, GameObject, GameState, GameStateTickContext, Scene, TickContext, TickRate};

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

/// Builds the game and its first scene.
fn setup() -> (Core<HelloGame>, sfeg::Handle<Walker>) {
    let mut scene = Scene::new();
    let walker = scene.spawn_immediate(Walker { speed: 6.0 }, Transform::IDENTITY);
    let core = Core::new(HelloGame::default(), scene, TickRate::DEFAULT);
    (core, walker)
}

fn main() {
    let (mut core, walker) = setup();
    for _ in 0..3 {
        core.tick();
        if let Some(local) = core.current_scene().local_of(walker) {
            println!("  walker at x = {:.2}", local.position().x());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greets_once_per_tick() {
        let (mut core, _) = setup();
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
        let (mut core, walker) = setup();
        for _ in 0..60 {
            core.tick();
        }
        let x = core
            .current_scene()
            .local_of(walker)
            .unwrap()
            .position()
            .x();
        assert!((x - 6.0).abs() < 1e-4, "{x}");
    }
}
