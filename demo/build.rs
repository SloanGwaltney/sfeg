//! Checks the asset manifest and generates a constant for each asset
//! (ADR 0015).

use sfeg_build::{AssetManifestSettings, SharedFileCheck, generate_asset_constants};

fn main() {
    let settings = AssetManifestSettings {
        manifest_path: "assets/assets.ron".into(),
        asset_folder: "assets".into(),
        files_shared_by_names: SharedFileCheck::Warn,
    };
    if let Err(error) = generate_asset_constants(&settings) {
        panic!("{error}");
    }
}
