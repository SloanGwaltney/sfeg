use sfeg::input::InputBindings;
use sfeg::{CoreSettings, MaxTicksPerFrame, TickRate};

fn main() {
    let _ = CoreSettings {
        tick_rate: TickRate::DEFAULT,
        max_ticks_per_frame: MaxTicksPerFrame::DEFAULT,
        input_bindings: InputBindings::none(),
        asset_folder: std::path::PathBuf::new(),
        asset_manifest: sfeg::assets::AssetManifest::EMPTY,
        scene_types: sfeg::scene_file::SceneTypeRegistry::new(),
    };
}
