//! A demo game built on SFEG.
//!
//! An orange cube walks back and forth across a floor, spinning as it goes,
//! with a smaller white cube riding on top as its hat. A fixed camera looks
//! on from above and in front. The game state says hello once per second
//! of game time.

use sfeg::math::{Rotation, Transform, Vector3};
use sfeg::{
    Color, Core, DrawContext, GameObject, GameState, GameStateTickContext, Handle,
    MaxTicksPerFrame, Parent, Perspective, Scene, TickContext, TickRate, WindowSettings,
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

/// Walks along x at a fixed speed, turning back at the edges of the floor,
/// and spins around its up axis.
struct Walker {
    /// Units per second.
    speed: f32,
    /// How far from the center it walks before turning back.
    turn_back_at: f32,
    /// +1 walking toward +x, -1 toward -x.
    direction: f32,
    /// Radians per second.
    spin_speed: f32,
}

impl GameObject for Walker {
    fn tick(&mut self, ctx: &mut TickContext) {
        let dt = ctx.game_time().dt();
        let mut position =
            ctx.local().position() + Vector3::UNIT_X * self.speed * self.direction * dt;
        if position.x().abs() >= self.turn_back_at {
            position = position.with_x(self.turn_back_at.copysign(position.x()));
            self.direction = -self.direction;
        }
        let spin = Rotation::from_rotation_y(self.spin_speed * dt).expect("finite spin");
        let rotation = ctx.local().rotation().then(spin);

        let local = ctx.local_mut();
        // Only fails for a non-finite position, which a clamped walk can't
        // produce.
        local
            .set_position(position)
            .expect("walker position stays finite");
        local.set_rotation(rotation);
    }

    fn draw(&self, ctx: &mut DrawContext) {
        ctx.draw_cube(Color::from_srgb_u8(255, 136, 0));
    }
}

/// Sits on whatever it is parented to. It has no behavior of its own: it
/// moves because its parent does.
struct Hat;

impl GameObject for Hat {
    fn draw(&self, ctx: &mut DrawContext) {
        ctx.draw_cube(Color::WHITE);
    }
}

/// A flat slab under the walker, so there is something to walk on.
struct Floor;

impl GameObject for Floor {
    fn draw(&self, ctx: &mut DrawContext) {
        ctx.draw_cube(Color::from_srgb_u8(70, 76, 90));
    }
}

/// Where the scene is seen from. It doesn't move: any object can be the
/// camera, and this one has no behavior.
struct FixedCamera;

impl GameObject for FixedCamera {}

/// Builds the game and its first scene.
fn setup() -> (Core<HelloGame>, Handle<Walker>, Handle<Hat>) {
    let mut scene = Scene::new();

    let floor = Transform::new(
        Vector3::new(0.0, -0.55, 0.0),
        Rotation::IDENTITY,
        Vector3::new(10.0, 0.1, 4.0),
    )
    .expect("finite");
    scene
        .spawn_immediate(Floor, Parent::Root, floor)
        .expect("the root is always there");

    let walker = scene
        .spawn_immediate(
            Walker {
                speed: 3.0,
                turn_back_at: 4.0,
                direction: 1.0,
                spin_speed: 1.5,
            },
            Parent::Root,
            Transform::IDENTITY,
        )
        .expect("the root is always there");
    let on_head = Transform::new(
        Vector3::new(0.0, 0.75, 0.0),
        Rotation::IDENTITY,
        Vector3::new(0.5, 0.5, 0.5),
    )
    .expect("finite");
    let hat = scene
        .spawn_immediate(Hat, walker, on_head)
        .expect("the walker was just spawned");

    // Above and in front, tilted down toward the middle of the floor.
    let camera_place = Transform::new(
        Vector3::new(0.0, 3.0, 10.0),
        Rotation::from_rotation_x(-(3.0f32 / 10.0).atan()).expect("finite"),
        Vector3::ONE,
    )
    .expect("finite");
    let camera = scene
        .spawn_immediate(FixedCamera, Parent::Root, camera_place)
        .expect("the root is always there");
    let perspective = Perspective::new(60f32.to_radians(), 0.1, 100.0).expect("valid perspective");
    scene
        .set_active_camera(camera, perspective)
        .expect("the camera was just spawned");

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
        assert!((x - 3.0).abs() < 1e-4, "{x}");
        // The hat rode along without moving itself.
        let hat_world = scene.world_position_of(hat).unwrap();
        assert!((hat_world.x() - 3.0).abs() < 1e-4, "{hat_world:?}");
        assert!((hat_world.y() - 0.75).abs() < 1e-6, "{hat_world:?}");
        assert_eq!(scene.local_of(hat).unwrap().position().x(), 0.0);
    }

    #[test]
    fn walker_turns_back_at_the_edge() {
        let (mut core, walker, _) = setup();
        // At 3 units per second it reaches the edge at 4 after 1⅓ seconds,
        // so two seconds in it has walked back to about 2. Rounding decides
        // which tick lands on the edge, so it can be one step (0.05) off.
        for _ in 0..120 {
            core.tick();
        }
        let x = core
            .current_scene()
            .local_of(walker)
            .unwrap()
            .position()
            .x();
        assert!((x - 2.0).abs() < 0.06, "{x}");
    }

    #[test]
    fn every_frame_draws_the_floor_walker_and_hat() {
        let (mut core, _, _) = setup();
        let draw_list = core.draw();
        assert!(draw_list.camera().is_some());
        assert_eq!(draw_list.draw_commands().len(), 3);
    }
}
