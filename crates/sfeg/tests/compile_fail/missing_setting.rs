use sfeg::input::InputBindings;
use sfeg::{CoreSettings, MaxTicksPerFrame, TickRate};

fn main() {
    let _ = CoreSettings {
        tick_rate: TickRate::DEFAULT,
        max_ticks_per_frame: MaxTicksPerFrame::DEFAULT,
        input_bindings: InputBindings::none(),
        asset_folder: std::path::PathBuf::new(),
    };
}
