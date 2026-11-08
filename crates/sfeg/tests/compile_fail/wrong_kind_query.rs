use sfeg::input::Action;
use sfeg::{GameState, GameStateTickContext};

#[derive(Action)]
#[action(name = "move", kind = Axis2d)]
struct Move;

struct Game;

impl GameState for Game {
    fn tick(&mut self, ctx: &mut GameStateTickContext) {
        // A 2D axis isn't a button.
        let _ = ctx.tick_input().just_pressed::<Move>();
    }
}

fn main() {}
