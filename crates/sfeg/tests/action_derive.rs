//! The `Action` derive, used the way a game uses it.

use sfeg::input::{Action, ActionRegistry, InputBindings, KeyCode};
use sfeg::platform::InputEvent;
use sfeg::{
    Core, CoreSettings, GameState, GameStateTickContext, MaxTicksPerFrame, Scene, TickRate,
    TransformInterpolation,
};

#[derive(Action)]
#[action(name = "jump", kind = Button)]
struct Jump;

#[derive(Action)]
#[action(name = "move_2d", kind = Axis2d)]
struct Move;

#[test]
fn the_derive_gives_the_declared_name() {
    assert_eq!(Jump::NAME, "jump");
    assert_eq!(Move::NAME, "move_2d");
}

#[derive(Default)]
struct Player {
    jumps: u32,
    last_direction: Option<(f32, f32)>,
}

impl GameState for Player {
    fn tick(&mut self, ctx: &mut GameStateTickContext) {
        if ctx.tick_input().just_pressed::<Jump>() {
            self.jumps += 1;
        }
        let direction = ctx.tick_input().axis_2d::<Move>();
        self.last_direction = Some((direction.x(), direction.y()));
    }
}

#[test]
fn derived_actions_drive_a_game() {
    let mut actions = ActionRegistry::new();
    actions.register::<Jump>().unwrap();
    actions.register::<Move>().unwrap();
    let bindings = InputBindings::from_toml(
        r#"
            [button.jump]
            keys = ["Space"]

            [axis_2d.move_2d]
            up = ["KeyW"]
            down = ["KeyS"]
            left = ["KeyA"]
            right = ["KeyD"]
        "#,
        &actions,
    )
    .unwrap();
    let mut core = Core::new(
        Player::default(),
        Scene::new(),
        CoreSettings {
            tick_rate: TickRate::DEFAULT,
            max_ticks_per_frame: MaxTicksPerFrame::DEFAULT,
            transform_interpolation: TransformInterpolation::Enabled,
            input_bindings: bindings,
        },
    );
    core.apply_input_event(InputEvent::KeyPressed(KeyCode::Space));
    core.apply_input_event(InputEvent::KeyPressed(KeyCode::KeyA));
    core.tick();
    core.tick();
    assert_eq!(core.game_state().jumps, 1);
    assert_eq!(core.game_state().last_direction, Some((-1.0, 0.0)));
}
