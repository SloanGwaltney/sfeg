//! A demo game built on SFEG.
//!
//! For now it runs headless: it ticks the engine a few times and greets
//! each tick.

use sfeg::{Core, GameState, TickContext, TickRate};

/// The game-wide state.
#[derive(Default)]
struct HelloGame {
    greetings: Vec<String>,
}

impl GameState for HelloGame {
    fn tick(&mut self, ctx: &mut TickContext) {
        let time = ctx.time();
        let greeting = format!(
            "Hello from tick {} ({:.3}s)",
            time.tick_count(),
            time.elapsed_seconds()
        );
        println!("{greeting}");
        self.greetings.push(greeting);
    }
}

fn main() {
    let mut core = Core::new(HelloGame::default(), TickRate::DEFAULT);
    for _ in 0..3 {
        core.tick();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greets_once_per_tick() {
        let mut core = Core::new(HelloGame::default(), TickRate::DEFAULT);
        for _ in 0..2 {
            core.tick();
        }
        assert_eq!(
            core.game_state().greetings,
            ["Hello from tick 0 (0.000s)", "Hello from tick 1 (0.017s)"]
        );
    }
}
