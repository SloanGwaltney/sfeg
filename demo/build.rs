//! Checks the asset manifest and generates a constant for each asset
//! (ADR 0015).

use sfeg_build::{AssetManifestSettings, generate_asset_constants};

fn main() {
    let settings = AssetManifestSettings {
        manifest_path: "assets/assets.ron".into(),
        asset_folder: "assets".into(),
    };
    if let Err(error) = generate_asset_constants(&settings) {
        panic!("{error}");
    }
}
