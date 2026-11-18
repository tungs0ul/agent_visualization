//! The 3D models are compiled into the binary (`build.rs`) and served to Bevy from
//! memory as its default asset source, so load paths stay `models/...` and an installed
//! app needs no checkout (PLAN.md M21).

use std::path::Path;

use bevy::asset::io::memory::{Dir, MemoryAssetReader};
use bevy::asset::io::{AssetSourceBuilder, AssetSourceId};
use bevy::prelude::*;

const FILES: &[(&str, &[u8])] = include!(concat!(env!("OUT_DIR"), "/embedded_assets.rs"));

/// Add before `DefaultPlugins`: the default source must exist before `AssetPlugin` builds.
pub struct EmbeddedAssetsPlugin;

impl Plugin for EmbeddedAssetsPlugin {
    fn build(&self, app: &mut App) {
        let root = Dir::default();
        for (path, bytes) in FILES {
            root.insert_asset(Path::new(path), *bytes);
        }
        app.register_asset_source(
            AssetSourceId::Default,
            AssetSourceBuilder::new(move || Box::new(MemoryAssetReader { root: root.clone() })),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::FILES;

    #[test]
    fn every_model_the_app_loads_is_embedded() {
        for path in ["models/robot/robot.glb", "models/environment/floor.glb", "models/stations/teleporter.glb"] {
            assert!(FILES.iter().any(|(p, b)| *p == path && !b.is_empty()), "{path} missing");
        }
    }
}
