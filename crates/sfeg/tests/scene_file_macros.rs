//! The scene file macros, used the way a game uses them: with no serde
//! dependency of its own.

use sfeg::assets::{AssetManifest, SceneFileAsset};
use sfeg::input::InputBindings;
use sfeg::scene_file::{SceneFileObject, SceneFileState, SceneTypeRegistry};
use sfeg::{
    Core, CoreSettings, GameObject, Handle, MaxTicksPerFrame, SceneState, TickRate,
    TransformInterpolation,
};

#[sfeg::scene_file_object(name = "Walker")]
struct Walker {
    speed: f32,
    /// Runtime state the file doesn't set.
    #[serde(skip)]
    steps_taken: u32,
}

impl GameObject for Walker {}

#[sfeg::scene_file_object(name = "Door")]
struct Door {
    walker: Handle<Walker>,
}

impl GameObject for Door {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[sfeg::scene_file_object(name = "Marker")]
enum Marker {
    Start,
    Finish,
}

impl GameObject for Marker {}

#[sfeg::scene_file_state(name = "Level")]
struct Level {
    player: Handle<Walker>,
    door: Handle<Door>,
    #[serde(default)]
    par_seconds: Option<u32>,
}

impl SceneState for Level {}

const LEVEL: SceneFileAsset = SceneFileAsset::from_manifest("level", "level.ron");

#[test]
fn the_macros_give_the_declared_names() {
    assert_eq!(Walker::NAME, "Walker");
    assert_eq!(Door::NAME, "Door");
    assert_eq!(Marker::NAME, "Marker");
    assert_eq!(<Level as SceneFileState>::NAME, "Level");
}

#[test]
fn a_scene_file_builds_the_macro_types() {
    let folder =
        std::env::temp_dir().join(format!("sfeg_scene_file_macros_{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join("level.ron"),
        r#"(
            scene_state: (type: "Level", data: (player: Object("player"), door: Object("door"), par_seconds: 30)),
            objects: [
                (type: "Marker", data: Start),
                (type: "Walker", id: "player", data: (speed: 2.5)),
                (type: "Door", id: "door", data: (walker: Object("player"))),
                (type: "Marker", data: Finish),
            ],
        )"#,
    )
    .unwrap();
    let mut scene_types = SceneTypeRegistry::new();
    scene_types.register_object::<Walker>().unwrap();
    scene_types.register_object::<Door>().unwrap();
    scene_types.register_object::<Marker>().unwrap();
    scene_types.register_scene_state::<Level>().unwrap();
    let settings = CoreSettings {
        tick_rate: TickRate::DEFAULT,
        max_ticks_per_frame: MaxTicksPerFrame::DEFAULT,
        transform_interpolation: TransformInterpolation::Enabled,
        input_bindings: InputBindings::none(),
        asset_folder: folder,
        asset_manifest: AssetManifest::EMPTY,
        scene_types,
    };
    let core = Core::new_with_first_scene_file((), LEVEL, settings).unwrap();

    let scene = core.current_scene();
    let level = scene.scene_state::<Level>().unwrap();
    assert_eq!(level.par_seconds, Some(30));
    let walker = scene.get(level.player).unwrap();
    assert_eq!((walker.speed, walker.steps_taken), (2.5, 0));
    assert_eq!(scene.get(level.door).unwrap().walker, level.player);
    assert_eq!(scene.len(), 4);
}
