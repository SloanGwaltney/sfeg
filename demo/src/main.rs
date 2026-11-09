//! A demo game built on SFEG.
//!
//! The player steers an orange cube around a floor with WASD or the arrow
//! keys, and jumps with Space. A smaller white cube rides on top as its
//! hat. A fixed camera looks on from above and in front. The game state
//! says hello once per second of game time.
//!
//! With no arguments it opens a window. To run headless and save one frame
//! as a PNG instead, pass both capture flags:
//!
//! ```text
//! cargo run -p demo -- --capture-after-ticks 60 --capture-path frame.png
//! ```

use std::error::Error;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::process::ExitCode;

use sfeg::capture::OffscreenRenderer;
use sfeg::input::{Action, ActionRegistry, InputBindings};
use sfeg::math::{Rotation, Transform, Vector3};
use sfeg::{
    Color, Core, CoreSettings, DrawContext, GameObject, GameState, GameStateTickContext, Handle,
    MaxTicksPerFrame, Parent, Perspective, Scene, TickContext, TickRate, TransformInterpolation,
    WindowSettings,
};

/// Jumps.
#[derive(Action)]
#[action(name = "jump", kind = Button)]
struct Jump;

/// Moves across the floor. Up is away from the camera.
#[derive(Action)]
#[action(name = "move", kind = Axis2d)]
struct Move;

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

/// How far from the center the walker can go along x and z: the floor's
/// half-size, less half the walker.
const FLOOR_HALF_X: f32 = 4.5;
const FLOOR_HALF_Z: f32 = 2.5;

/// The player's cube: walks where [`Move`] points, jumps on [`Jump`], and
/// spins around its up axis.
struct Walker {
    /// Units per second.
    speed: f32,
    /// Upward speed when a jump starts, in units per second.
    jump_speed: f32,
    /// Downward acceleration, in units per second squared.
    gravity: f32,
    /// Current upward speed, in units per second.
    vertical_speed: f32,
    /// Radians per second.
    spin_speed: f32,
}

impl GameObject for Walker {
    fn tick(&mut self, ctx: &mut TickContext) {
        let dt = ctx.game_time().dt();
        let input = ctx.tick_input();
        let direction = input.axis_2d::<Move>();
        let on_ground = ctx.local().position().y() <= 0.0;
        if on_ground && input.just_pressed::<Jump>() {
            self.vertical_speed = self.jump_speed;
        }

        // Up on the axis is -z, away from the camera.
        let step = Vector3::new(direction.x(), 0.0, -direction.y()) * self.speed * dt;
        self.vertical_speed -= self.gravity * dt;
        let mut position =
            ctx.local().position() + step + Vector3::UNIT_Y * self.vertical_speed * dt;
        if position.y() <= 0.0 {
            position = position.with_y(0.0);
            self.vertical_speed = 0.0;
        }
        let position = position
            .with_x(position.x().clamp(-FLOOR_HALF_X, FLOOR_HALF_X))
            .with_z(position.z().clamp(-FLOOR_HALF_Z, FLOOR_HALF_Z));
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

/// The demo's actions bound by its default bindings file.
fn input_bindings() -> InputBindings {
    let mut actions = ActionRegistry::new();
    actions.register::<Jump>().expect("jump is registered once");
    actions.register::<Move>().expect("move is registered once");
    // Checked by the tests, so it can't fail in a released build.
    InputBindings::from_toml(include_str!("bindings.toml"), &actions)
        .expect("the default bindings match the actions")
}

/// Builds the game and its first scene.
fn setup() -> (Core<HelloGame>, Handle<Walker>, Handle<Hat>) {
    let mut scene = Scene::new(());

    let floor = Transform::new(
        Vector3::new(0.0, -0.55, 0.0),
        Rotation::IDENTITY,
        Vector3::new(10.0, 0.1, 6.0),
    )
    .expect("finite");
    scene
        .spawn_immediate(Floor, Parent::Root, floor)
        .expect("the root is always there");

    let walker = scene
        .spawn_immediate(
            Walker {
                speed: 3.0,
                jump_speed: 6.0,
                gravity: 20.0,
                vertical_speed: 0.0,
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
        CoreSettings {
            tick_rate: TickRate::DEFAULT,
            max_ticks_per_frame: MaxTicksPerFrame::DEFAULT,
            transform_interpolation: TransformInterpolation::Enabled,
            input_bindings: input_bindings(),
        },
    );
    (core, walker, hat)
}

/// The window's drawable area, in logical pixels. Captures use the same
/// numbers as physical pixels, as on a display at 100% scale.
const WINDOW_WIDTH: u32 = 1280;
const WINDOW_HEIGHT: u32 = 720;

/// What the command line asked for.
#[derive(Debug, PartialEq, Eq)]
enum Mode {
    /// Open a window and play.
    Windowed,
    /// Run headless, then save one frame.
    Capture {
        /// How many ticks to run before drawing the frame. Zero captures
        /// the scene as set up.
        after_ticks: u64,
        /// Where to write the PNG.
        path: PathBuf,
    },
}

const USAGE: &str = "usage: demo [--capture-after-ticks <TICKS> --capture-path <FILE.png>]";

/// Reads the arguments after the program name. The capture flags must be
/// given together: there is no default tick count or path.
fn parse_mode(args: impl IntoIterator<Item = String>) -> Result<Mode, String> {
    let mut after_ticks = None;
    let mut path = None;
    let mut args = args.into_iter();
    while let Some(flag) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{flag} needs a value"));
        match flag.as_str() {
            "--capture-after-ticks" => {
                let text = value()?;
                let ticks = text.parse::<u64>().map_err(|_| {
                    format!("--capture-after-ticks needs a whole number of ticks, not {text:?}")
                })?;
                if after_ticks.replace(ticks).is_some() {
                    return Err("--capture-after-ticks was given twice".to_string());
                }
            }
            "--capture-path" => {
                if path.replace(PathBuf::from(value()?)).is_some() {
                    return Err("--capture-path was given twice".to_string());
                }
            }
            _ => return Err(format!("unknown argument {flag:?}")),
        }
    }
    match (after_ticks, path) {
        (None, None) => Ok(Mode::Windowed),
        (Some(after_ticks), Some(path)) => Ok(Mode::Capture { after_ticks, path }),
        (Some(_), None) => Err("--capture-after-ticks needs --capture-path too".to_string()),
        (None, Some(_)) => Err("--capture-path needs --capture-after-ticks too".to_string()),
    }
}

/// Runs `after_ticks` ticks with no input, draws one frame offscreen, and
/// writes it to `path`.
///
/// It draws right after the last tick, as a window would on a frame that
/// ran exactly that many ticks.
fn capture(after_ticks: u64, path: &std::path::Path) -> Result<(), Box<dyn Error>> {
    sfeg::install_default_tracing_subscriber();
    let (mut core, _, _) = setup();
    for _ in 0..after_ticks {
        core.tick();
    }
    let width_px = NonZeroU32::new(WINDOW_WIDTH).expect("non-zero width");
    let height_px = NonZeroU32::new(WINDOW_HEIGHT).expect("non-zero height");
    let mut renderer = OffscreenRenderer::new(width_px, height_px)?;
    let image = renderer.render_to_image(&core.draw())?;
    image.write_png(path)?;
    println!(
        "Captured {width_px}x{height_px} after {after_ticks} ticks to {}",
        path.display()
    );
    Ok(())
}

fn main() -> ExitCode {
    let mode = match parse_mode(std::env::args().skip(1)) {
        Ok(mode) => mode,
        Err(message) => {
            eprintln!("{message}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let result = match mode {
        Mode::Windowed => {
            let (core, _, _) = setup();
            sfeg::run(
                core,
                WindowSettings {
                    title: "SFEG demo".to_string(),
                    logical_width: WINDOW_WIDTH,
                    logical_height: WINDOW_HEIGHT,
                },
            )
            .map_err(Box::<dyn Error>::from)
        }
        Mode::Capture { after_ticks, path } => capture(after_ticks, &path),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            // Print the whole chain of causes, not only the top one.
            let mut message = error.to_string();
            let mut source = error.source();
            while let Some(cause) = source {
                message.push_str(&format!(": {cause}"));
                source = cause.source();
            }
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use sfeg::input::KeyCode;
    use sfeg::platform::InputEvent;

    use super::*;

    fn ticks(core: &mut Core<HelloGame>, count: u32) {
        for _ in 0..count {
            core.tick();
        }
    }

    #[test]
    fn the_default_bindings_load() {
        input_bindings();
    }

    #[test]
    fn walker_covers_its_speed_in_one_second() {
        let (mut core, walker, hat) = setup();
        core.apply_input_event(InputEvent::KeyPressed(KeyCode::KeyD));
        ticks(&mut core, 60);
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
    fn up_walks_away_from_the_camera_and_stops_at_the_edge() {
        let (mut core, walker, _) = setup();
        core.apply_input_event(InputEvent::KeyPressed(KeyCode::ArrowUp));
        ticks(&mut core, 600);
        let z = core
            .current_scene()
            .local_of(walker)
            .unwrap()
            .position()
            .z();
        assert_eq!(z, -FLOOR_HALF_Z);
    }

    #[test]
    fn jumping_goes_up_and_lands() {
        let (mut core, walker, _) = setup();
        core.apply_input_event(InputEvent::KeyPressed(KeyCode::Space));
        ticks(&mut core, 10);
        let height = |core: &Core<HelloGame>| {
            core.current_scene()
                .local_of(walker)
                .unwrap()
                .position()
                .y()
        };
        assert!(height(&core) > 0.5, "{}", height(&core));
        // Holding Space doesn't jump again: it lands and stays down.
        ticks(&mut core, 120);
        assert_eq!(height(&core), 0.0);
    }

    fn args(text: &str) -> Vec<String> {
        text.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn no_arguments_opens_a_window() {
        assert_eq!(parse_mode(args("")), Ok(Mode::Windowed));
    }

    #[test]
    fn both_capture_flags_capture() {
        assert_eq!(
            parse_mode(args("--capture-path out.png --capture-after-ticks 0")),
            Ok(Mode::Capture {
                after_ticks: 0,
                path: PathBuf::from("out.png"),
            })
        );
    }

    #[test]
    fn capture_flags_have_no_defaults() {
        assert!(parse_mode(args("--capture-after-ticks 60")).is_err());
        assert!(parse_mode(args("--capture-path out.png")).is_err());
        assert!(parse_mode(args("--capture-after-ticks")).is_err());
    }

    #[test]
    fn bad_capture_arguments_are_rejected() {
        assert!(parse_mode(args("--capture-after-ticks -1 --capture-path a.png")).is_err());
        assert!(
            parse_mode(args(
                "--capture-after-ticks 1 --capture-after-ticks 2 --capture-path a.png"
            ))
            .is_err()
        );
        assert!(parse_mode(args("--fullscreen")).is_err());
    }

    /// Needs a GPU driver. A CPU one, such as Mesa's lavapipe, is enough.
    #[test]
    fn a_capture_shows_the_walker_and_hat_on_the_floor() {
        let (mut core, _, _) = setup();
        let width_px = NonZeroU32::new(320).unwrap();
        let height_px = NonZeroU32::new(180).unwrap();
        let mut renderer = OffscreenRenderer::new(width_px, height_px).unwrap();
        let image = renderer.render_to_image(&core.draw()).unwrap();
        assert_eq!((image.width_px(), image.height_px()), (width_px, height_px));
        assert_eq!(image.rgba8().len(), 320 * 180 * 4);

        // The camera looks at the origin, where the walker stands, so the
        // middle of the image is orange. Faces are shaded, so only the
        // hue is checked.
        let [r, g, b, a] = image.pixel_rgba8(160, 90).unwrap();
        assert!(
            r > 150 && g > 60 && g < r && b < 30 && a == 255,
            "{r} {g} {b} {a}"
        );
        // The hat sits above it: shaded white.
        let [r, g, b, _] = image.pixel_rgba8(160, 79).unwrap();
        assert!(r > 150 && r == g && g == b, "{r} {g} {b}");
        // The floor is below, gray-blue and lighter than the background.
        let [r, g, b, _] = image.pixel_rgba8(160, 110).unwrap();
        assert!(b > r && r > 50, "{r} {g} {b}");
        // The top corner is only the clear color.
        let [r, g, b, _] = image.pixel_rgba8(0, 0).unwrap();
        assert!(b > g && g > r && b < 80, "{r} {g} {b}");
        assert_eq!(image.pixel_rgba8(320, 0), None);
    }

    #[test]
    fn every_frame_draws_the_floor_walker_and_hat() {
        let (mut core, _, _) = setup();
        let draw_list = core.draw();
        assert!(draw_list.camera().is_some());
        assert_eq!(draw_list.draw_commands().len(), 3);
    }
}
