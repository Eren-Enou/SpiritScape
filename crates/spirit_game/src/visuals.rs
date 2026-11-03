//! Presentation data (`data/visuals.toml`): which model represents which
//! simulation object. The simulation itself never reads this.

use bevy::gltf::GltfAssetLabel;
use bevy::prelude::*;
use bevy::world_serialization::WorldAssetRoot;
use serde::Deserialize;
use std::collections::BTreeMap;

const BUILTIN_VISUALS: &str = include_str!("../../../data/visuals.toml");

#[derive(Resource, Deserialize)]
pub struct Visuals {
    pub terrain: TerrainVisuals,
    pub trees: BTreeMap<String, TreeVisuals>,
    pub buildings: BTreeMap<String, BuildingVisuals>,
    pub characters: CharacterVisuals,
    pub spirit_look: SpiritLook,
}

#[derive(Deserialize)]
pub struct TerrainVisuals {
    pub grass: String,
    pub water: String,
    pub water_bank: String,
    pub bridge: String,
    pub bridge_rot: f32,
    pub rocks: Vec<String>,
    pub decorations: Vec<String>,
    pub decoration_chance_pct: u32,
}

#[derive(Deserialize)]
pub struct TreeVisuals {
    pub models: Vec<String>,
    pub stump: String,
    pub scale: f32,
    /// Recolours the leaves so tiers are told apart at a glance.
    #[serde(default)]
    pub crown_tint: Option<[f32; 3]>,
}

#[derive(Deserialize, Clone)]
pub struct Part {
    pub model: String,
    #[serde(default)]
    pub rot: f32,
    #[serde(default)]
    pub y: f32,
}

impl Part {
    pub fn transform(&self) -> Transform {
        Transform::from_xyz(0.0, self.y, 0.0)
            .with_rotation(Quat::from_rotation_y(self.rot.to_radians()))
    }
}

#[derive(Deserialize)]
pub struct BuildingVisuals {
    pub parts: Vec<Part>,
    #[serde(default)]
    pub site: Vec<Part>,
    pub stockpile: String,
    /// Offset of the log pile from the tile centre, as [x, z].
    #[serde(default)]
    pub stockpile_offset: [f32; 2],
}

#[derive(Deserialize)]
pub struct CharacterModel {
    pub model: String,
    #[serde(default)]
    pub hand_item: Option<String>,
}

#[derive(Deserialize)]
pub struct CharacterVisuals {
    pub scale: f32,
    pub animations: Vec<String>,
    pub idle: String,
    pub walk: String,
    pub work: String,
    pub player: CharacterModel,
    pub roles: BTreeMap<String, CharacterModel>,
}

#[derive(Deserialize)]
pub struct SpiritLook {
    pub alpha: f32,
    pub glow: f32,
}

impl Visuals {
    pub fn builtin() -> Self {
        toml::from_str(BUILTIN_VISUALS).expect("data/visuals.toml must be valid")
    }
}

/// Spawnable root for a glTF file's first scene.
pub fn model(assets: &AssetServer, path: &str) -> WorldAssetRoot {
    WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(path.to_string())))
}

/// Small deterministic hash for visual variety (model choice, rotation).
pub fn tile_hash(x: i32, y: i32) -> u32 {
    let h = (x as u32).wrapping_mul(73_856_093) ^ (y as u32).wrapping_mul(19_349_663);
    h ^ (h >> 13)
}
