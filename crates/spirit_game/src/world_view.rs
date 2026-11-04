//! The 3D world: terrain tiles, trees and buildings built from glTF models,
//! plus floating labels and gizmo overlays. Everything here only reads the
//! simulation.

use crate::camera::HoveredTile;
use crate::cards::{BAD, GOOD, INFO, MUTED, WARN};
use crate::script::Cursor;
use crate::sim_runner::Sim;
use crate::status::spirit_status;
use crate::visuals::{Visuals, model, tile_hash};
use bevy::picking::Pickable;
use bevy::prelude::*;
use bevy::world_serialization::WorldInstanceReady;
use spirit_sim::defs::SiteId;
use spirit_sim::entities::{BuildingState, ProjectState, TreeState};
use spirit_sim::grid::{Terrain, TilePos};
use spirit_sim::metrics::WaitKind;
use std::collections::HashMap;

/// Despawned and rebuilt whenever the simulation restarts.
#[derive(Component)]
pub struct WorldEntity;

/// What has been spawned for the current simulation generation.
#[derive(Resource, Default)]
pub struct Spawned {
    pub generation: Option<u32>,
    pub buildings: usize,
    pub spirits: usize,
}

#[derive(Component)]
struct TreeModel(usize);
#[derive(Component)]
struct TreeStump(usize);
#[derive(Component)]
struct BuildingParts(usize);
#[derive(Component)]
struct BuildingSite(usize);
#[derive(Component)]
struct Stockpile(usize);
/// Finished goods stacked beside a building.
#[derive(Component)]
struct GoodsPile(usize);

/// Leaf colour to apply to a tree model once its scene has spawned.
#[derive(Component)]
struct CrownTint(Color);
#[derive(Component)]
struct TintReady;

/// Parts of a site banner (see `spawn_site_banner`).
#[derive(Component)]
struct SiteTitle(usize);
#[derive(Component)]
struct SiteFill(usize);
#[derive(Component)]
struct SiteNote(usize);

/// Description of whatever is under the mouse cursor.
#[derive(Component)]
struct HoverTip;

/// Locked trees within this many tiles of the player get a red ring.
const LOCK_RING_RANGE: i32 = 7;

/// A screen-space label that follows a point in the world.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub enum WorldLabel {
    Player,
    Spirit(usize),
    Building(usize),
    /// Name and availability of a gathering site.
    Site(usize),
}

/// Tile centre in world space.
pub fn tile_world(x: f32, y: f32) -> Vec3 {
    Vec3::new(x, 0.0, y)
}

pub struct WorldViewPlugin;

impl Plugin for WorldViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Spawned>()
            .add_systems(Startup, spawn_hover_tip)
            .add_systems(
                Update,
                (
                    rebuild_on_restart,
                    spawn_new_buildings,
                    (
                        sync_trees,
                        tint_crowns,
                        sync_buildings,
                        sync_goods_piles,
                        update_site_banners,
                        update_labels,
                        update_hover_tip,
                        draw_overlays,
                    ),
                )
                    .chain(),
            );
    }
}

pub fn spawn_label(commands: &mut Commands, label: WorldLabel, size: f32) {
    commands.spawn((
        WorldEntity,
        label,
        Text::new(""),
        TextFont::from_font_size(size),
        TextColor(Color::WHITE),
        Node {
            position_type: PositionType::Absolute,
            padding: UiRect::axes(Val::Px(5.0), Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(4.0)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.04, 0.05, 0.07, 0.72)),
        Pickable::IGNORE,
    ));
}

/// A site's name, a bar of how many of its trees are standing, and a note
/// (reserved, or spirits waiting for regrowth).
fn spawn_site_banner(commands: &mut Commands, i: usize) {
    commands
        .spawn((
            WorldEntity,
            WorldLabel::Site(i),
            Node {
                position_type: PositionType::Absolute,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
                row_gap: Val::Px(3.0),
                border_radius: BorderRadius::all(Val::Px(5.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.05, 0.06, 0.08, 0.72)),
            Pickable::IGNORE,
        ))
        .with_children(|b| {
            b.spawn((
                SiteTitle(i),
                Text::new(""),
                TextFont::from_font_size(14.0),
                TextColor(Color::WHITE),
                Pickable::IGNORE,
            ));
            b.spawn((
                Node {
                    width: Val::Px(120.0),
                    height: Val::Px(6.0),
                    border_radius: BorderRadius::all(Val::Px(3.0)),
                    overflow: Overflow::clip(),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.2, 0.22, 0.26)),
                Pickable::IGNORE,
            ))
            .with_children(|t| {
                t.spawn((
                    SiteFill(i),
                    Node {
                        height: Val::Percent(100.0),
                        ..default()
                    },
                    BackgroundColor(GOOD),
                    Pickable::IGNORE,
                ));
            });
            b.spawn((
                SiteNote(i),
                Text::new(""),
                TextFont::from_font_size(11.0),
                TextColor(MUTED),
                Pickable::IGNORE,
            ));
        });
}

fn spawn_hover_tip(mut commands: Commands) {
    commands.spawn((
        HoverTip,
        Text::new(""),
        TextFont::from_font_size(13.0),
        TextColor(Color::WHITE),
        Node {
            position_type: PositionType::Absolute,
            padding: UiRect::axes(Val::Px(8.0), Val::Px(5.0)),
            border_radius: BorderRadius::all(Val::Px(5.0)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.05, 0.06, 0.08, 0.88)),
        GlobalZIndex(10),
        Visibility::Hidden,
        Pickable::IGNORE,
    ));
}

fn rebuild_on_restart(
    mut commands: Commands,
    sim: Res<Sim>,
    visuals: Res<Visuals>,
    assets: Res<AssetServer>,
    mut spawned: ResMut<Spawned>,
    existing: Query<Entity, With<WorldEntity>>,
) {
    if spawned.generation == Some(sim.generation) {
        return;
    }
    for e in &existing {
        commands.entity(e).despawn();
    }
    *spawned = Spawned {
        generation: Some(sim.generation),
        buildings: 0,
        spirits: 0,
    };
    let w = &sim.world;
    let t = &visuals.terrain;
    let is_water = |p: TilePos| matches!(w.grid.terrain(p), Some(Terrain::Water | Terrain::Bridge));

    for y in 0..w.grid.height {
        for x in 0..w.grid.width {
            let p = TilePos::new(x, y);
            let h = tile_hash(x, y);
            let at = tile_world(x as f32, y as f32);
            let mut spawn = |path: &str, transform: Transform| {
                commands.spawn((WorldEntity, model(&assets, path), transform));
            };
            match w.grid.terrain(p).unwrap() {
                Terrain::Grass => {
                    spawn(&t.grass, Transform::from_translation(at));
                    let free = !w.grid.is_occupied(p);
                    if free && !t.decorations.is_empty() && h % 100 < t.decoration_chance_pct {
                        let deco = &t.decorations[(h / 100) as usize % t.decorations.len()];
                        let offset = Vec3::new(
                            ((h >> 8) % 5) as f32 * 0.12 - 0.24,
                            0.0,
                            ((h >> 12) % 5) as f32 * 0.12 - 0.24,
                        );
                        spawn(
                            deco,
                            Transform::from_translation(at + offset)
                                .with_rotation(Quat::from_rotation_y((h % 360) as f32))
                                .with_scale(Vec3::splat(0.8)),
                        );
                    }
                }
                Terrain::Rock => {
                    spawn(&t.grass, Transform::from_translation(at));
                    let rock = &t.rocks[h as usize % t.rocks.len()];
                    let rot =
                        Quat::from_rotation_y(((h >> 4) % 4) as f32 * std::f32::consts::FRAC_PI_2);
                    spawn(
                        rock,
                        Transform::from_translation(at)
                            .with_rotation(rot)
                            .with_scale(Vec3::splat(1.1)),
                    );
                }
                Terrain::Water | Terrain::Bridge => {
                    // Banks face whichever side borders land; the model's bank is on its north edge.
                    let west_land = !is_water(TilePos::new(x - 1, y));
                    let east_land = !is_water(TilePos::new(x + 1, y));
                    let (path, rot) = match (west_land, east_land) {
                        (true, false) => (&t.water_bank, 90.0f32),
                        (false, true) => (&t.water_bank, -90.0),
                        _ => (&t.water, 0.0),
                    };
                    spawn(
                        path,
                        Transform::from_translation(at)
                            .with_rotation(Quat::from_rotation_y(rot.to_radians())),
                    );
                    if w.grid.terrain(p) == Some(Terrain::Bridge) {
                        let rot = Quat::from_rotation_y(t.bridge_rot.to_radians());
                        spawn(
                            &t.bridge,
                            Transform::from_translation(at).with_rotation(rot),
                        );
                    }
                }
            }
        }
    }

    for i in 0..w.defs.sites.len() {
        spawn_site_banner(&mut commands, i);
    }

    for tree in &w.trees {
        let kind = &w.defs.tree(tree.kind).id;
        let Some(tv) = visuals.trees.get(kind) else {
            continue;
        };
        let h = tile_hash(tree.pos.x, tree.pos.y);
        let at = tile_world(tree.pos.x as f32, tree.pos.y as f32);
        let rot = Quat::from_rotation_y((h % 360) as f32);
        let mut entity = commands.spawn((
            WorldEntity,
            TreeModel(tree.id),
            model(&assets, &tv.models[h as usize % tv.models.len()]),
            Transform::from_translation(at)
                .with_rotation(rot)
                .with_scale(Vec3::splat(tv.scale)),
        ));
        if let Some([r, g, b]) = tv.crown_tint {
            entity.insert(CrownTint(Color::srgb(r, g, b))).observe(
                |ev: On<WorldInstanceReady>, mut commands: Commands| {
                    commands.entity(ev.entity).insert(TintReady);
                },
            );
        }
        commands.spawn((
            WorldEntity,
            TreeStump(tree.id),
            model(&assets, &tv.stump),
            Transform::from_translation(at)
                .with_rotation(rot)
                .with_scale(Vec3::splat(1.6)),
            Visibility::Hidden,
        ));
    }
}

fn spawn_new_buildings(
    mut commands: Commands,
    sim: Res<Sim>,
    visuals: Res<Visuals>,
    assets: Res<AssetServer>,
    mut spawned: ResMut<Spawned>,
) {
    let w = &sim.world;
    for b in &w.buildings[spawned.buildings..] {
        let kind = &w.defs.building(b.kind).id;
        let at = tile_world(b.pos.x as f32, b.pos.y as f32);
        let Some(bv) = visuals.buildings.get(kind) else {
            continue;
        };
        commands
            .spawn((
                WorldEntity,
                BuildingParts(b.id),
                Transform::from_translation(at),
                Visibility::Hidden,
            ))
            .with_children(|c| {
                for part in &bv.parts {
                    c.spawn((model(&assets, &part.model), part.transform()));
                }
            });
        commands
            .spawn((
                WorldEntity,
                BuildingSite(b.id),
                Transform::from_translation(at),
                Visibility::Hidden,
            ))
            .with_children(|c| {
                for part in &bv.site {
                    c.spawn((model(&assets, &part.model), part.transform()));
                }
            });
        let [ox, oz] = bv.stockpile_offset;
        commands.spawn((
            WorldEntity,
            Stockpile(b.id),
            model(&assets, &bv.stockpile),
            Transform::from_translation(at + Vec3::new(ox, 0.0, oz)),
            Visibility::Hidden,
        ));
        if let Some(pile) = &bv.goods_pile {
            let [ox, oz] = bv.goods_pile_offset;
            commands.spawn((
                WorldEntity,
                GoodsPile(b.id),
                model(&assets, pile),
                Transform::from_translation(at + Vec3::new(ox, 0.0, oz)),
                Visibility::Hidden,
            ));
        }
        spawn_label(&mut commands, WorldLabel::Building(b.id), 13.0);
    }
    spawned.buildings = w.buildings.len();
}

fn sync_trees(
    sim: Res<Sim>,
    visuals: Res<Visuals>,
    mut trees: Query<(&TreeModel, &mut Visibility, &mut Transform), Without<TreeStump>>,
    mut stumps: Query<(&TreeStump, &mut Visibility), Without<TreeModel>>,
) {
    let w = &sim.world;
    for (tm, mut vis, mut tf) in &mut trees {
        let t = &w.trees[tm.0];
        let base = visuals
            .trees
            .get(&w.defs.tree(t.kind).id)
            .map_or(1.0, |v| v.scale);
        // Trees shrink a little as they are cut, then disappear into a stump.
        let fullness = t.logs_left() as f32 / w.defs.tree(t.kind).logs as f32;
        tf.scale = Vec3::splat(base * (0.7 + 0.3 * fullness));
        vis.set_if_neq(if t.is_grown() {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
    for (ts, mut vis) in &mut stumps {
        let grown = w.trees[ts.0].is_grown();
        vis.set_if_neq(if grown {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        });
    }
}

/// Recolours the leaves (the greenish materials) of tinted tree models.
/// Trees of one kind share their tinted materials.
fn tint_crowns(
    mut commands: Commands,
    trees: Query<(Entity, &CrownTint), With<TintReady>>,
    children: Query<&Children>,
    mesh_materials: Query<&MeshMaterial3d<StandardMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut tinted: Local<HashMap<AssetId<StandardMaterial>, Handle<StandardMaterial>>>,
) {
    for (tree, tint) in &trees {
        commands.entity(tree).remove::<(CrownTint, TintReady)>();
        for e in children.iter_descendants(tree) {
            let Ok(handle) = mesh_materials.get(e) else {
                continue;
            };
            let new = match tinted.get(&handle.0.id()) {
                Some(h) => h.clone(),
                None => {
                    let Some(m) = materials.get(&handle.0).cloned() else {
                        continue;
                    };
                    let c = m.base_color.to_srgba();
                    let leafy = c.green > c.red && c.green > c.blue;
                    let h = if leafy {
                        let mut m = m;
                        m.base_color = m.base_color.mix(&tint.0, 0.75);
                        materials.add(m)
                    } else {
                        handle.0.clone()
                    };
                    tinted.insert(handle.0.id(), h.clone());
                    h
                }
            };
            commands.entity(e).insert(MeshMaterial3d(new));
        }
    }
}

#[allow(clippy::type_complexity)]
fn sync_buildings(
    sim: Res<Sim>,
    mut parts: Query<
        (&BuildingParts, &mut Visibility, &mut Transform),
        (Without<BuildingSite>, Without<Stockpile>),
    >,
    mut sites: Query<
        (&BuildingSite, &mut Visibility),
        (Without<BuildingParts>, Without<Stockpile>),
    >,
    mut piles: Query<
        (&Stockpile, &mut Visibility, &mut Transform),
        (Without<BuildingParts>, Without<BuildingSite>),
    >,
) {
    let w = &sim.world;
    for (bp, mut vis, mut tf) in &mut parts {
        let b = &w.buildings[bp.0];
        // While being built, the structure rises out of the ground.
        let (show, height) = match b.state {
            BuildingState::Complete => (true, 1.0),
            BuildingState::UnderConstruction { project } => {
                let p = &w.projects[project];
                let progress = p.build_progress as f32 / p.build_required.max(1) as f32;
                (p.state == ProjectState::Building, 0.1 + 0.9 * progress)
            }
        };
        tf.scale = Vec3::new(1.0, height, 1.0);
        vis.set_if_neq(if show {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
    for (bs, mut vis) in &mut sites {
        let site = !w.buildings[bs.0].is_complete();
        vis.set_if_neq(if site {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
    for (sp, mut vis, mut tf) in &mut piles {
        let b = &w.buildings[sp.0];
        let amount = match b.state {
            BuildingState::Complete => b.storage.get(w.defs.objective.resource),
            BuildingState::UnderConstruction { project } => {
                w.projects[project].delivered.values().sum()
            }
        };
        let fill = (amount as f32 / 40.0).min(1.0);
        tf.scale = Vec3::splat(0.6 + 0.7 * fill);
        vis.set_if_neq(if amount > 0 {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
}

fn sync_goods_piles(
    sim: Res<Sim>,
    mut piles: Query<(&GoodsPile, &mut Visibility, &mut Transform)>,
) {
    let w = &sim.world;
    for (pile, mut vis, mut tf) in &mut piles {
        let amount = w.buildings[pile.0].storage.get(w.settlement.order.output);
        tf.scale = Vec3::splat(0.4 + 0.4 * (amount as f32 / 30.0).min(1.0));
        vis.set_if_neq(if amount > 0 {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
}

/// Spirits waiting for regrowth at a site.
fn waiting_at(w: &spirit_sim::World, site: SiteId) -> usize {
    w.spirits
        .iter()
        .filter(|s| s.home_site == Some(site) && s.wait == Some(WaitKind::Recovery))
        .count()
}

fn update_site_banners(
    sim: Res<Sim>,
    mut titles: Query<(&SiteTitle, &mut Text), Without<SiteNote>>,
    mut notes: Query<(&SiteNote, &mut Text, &mut TextColor), Without<SiteTitle>>,
    mut fills: Query<(&SiteFill, &mut Node, &mut BackgroundColor)>,
) {
    let w = &sim.world;
    let counts: Vec<(u32, u32)> = (0..w.defs.sites.len())
        .map(|i| {
            let site = SiteId(i as u16);
            w.trees
                .iter()
                .filter(|t| t.site == Some(site))
                .fold((0, 0), |(g, n), t| (g + t.is_grown() as u32, n + 1))
        })
        .collect();
    for (t, mut text) in &mut titles {
        let (grown, total) = counts[t.0];
        let content = format!("{}  {grown}/{total} trees", w.defs.sites[t.0].name);
        if text.0 != content {
            text.0 = content;
        }
    }
    for (f, mut node, mut bg) in &mut fills {
        let (grown, total) = counts[f.0];
        let frac = grown as f32 / total.max(1) as f32;
        node.width = Val::Percent(frac * 100.0);
        bg.0 = if frac >= 0.6 {
            GOOD
        } else if frac >= 0.25 {
            WARN
        } else {
            BAD
        };
    }
    for (n, mut text, mut color) in &mut notes {
        let waiting = waiting_at(w, SiteId(n.0 as u16));
        let (content, c) = if !w.settlement.site_open[n.0] {
            ("Reserved for you - no spirits".to_string(), INFO)
        } else if waiting > 0 {
            (
                format!("{waiting} spirit(s) waiting for trees to regrow"),
                WARN,
            )
        } else {
            ("Spirits allowed".to_string(), MUTED)
        };
        if text.0 != content {
            text.0 = content;
        }
        color.0 = c;
    }
}

/// Positions name/status labels over characters, buildings and sites, then
/// nudges character labels upwards so none overlap.
#[allow(clippy::type_complexity)]
fn update_labels(
    sim: Res<Sim>,
    camera: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    mut labels: Query<(
        Entity,
        &WorldLabel,
        Option<&mut Text>,
        Option<&mut TextColor>,
        &mut Node,
        &ComputedNode,
        &mut Visibility,
    )>,
) {
    let Ok((camera, cam_t)) = camera.single() else {
        return;
    };
    let w = &sim.world;
    // (priority, entity, top-left, size); lower priority is placed first and never moves.
    let mut placed: Vec<(u8, Entity, Vec2, Vec2)> = Vec::new();
    for (entity, label, text, color, _, computed, mut vis) in &mut labels {
        let (priority, pos, height, content) = match *label {
            WorldLabel::Player => {
                let (x, y) = w.player.actor.mover.visual_pos();
                (
                    2,
                    tile_world(x, y),
                    1.05,
                    Some(("You".to_string(), Color::WHITE)),
                )
            }
            WorldLabel::Spirit(id) => {
                let s = &w.spirits[id];
                let (x, y) = s.actor.mover.visual_pos();
                let (status, word) = spirit_status(s);
                let content = format!("{} Lv{} - {word}", s.name, s.level());
                (3, tile_world(x, y), 1.05, Some((content, status.color())))
            }
            WorldLabel::Building(id) => {
                let b = &w.buildings[id];
                let def = w.defs.building(b.kind);
                let content = match b.state {
                    BuildingState::Complete if def.capacity == 0 => b.name.clone(),
                    BuildingState::Complete if b.storage.total() == 0 => {
                        format!("{}: empty", b.name)
                    }
                    BuildingState::Complete => format!(
                        "{}: {}",
                        b.name,
                        w.defs.describe_amounts(b.storage.contents())
                    ),
                    BuildingState::UnderConstruction { project } => {
                        let p = &w.projects[project];
                        let cost: u32 = w.defs.building(p.kind).cost.iter().map(|c| c.1).sum();
                        let delivered: u32 = p.delivered.values().sum();
                        match p.state {
                            ProjectState::Building => format!(
                                "{} (building {}%)",
                                b.name,
                                p.build_progress * 100 / p.build_required.max(1)
                            ),
                            _ => format!("{} (materials {delivered}/{cost})", b.name),
                        }
                    }
                };
                (
                    0,
                    tile_world(b.pos.x as f32, b.pos.y as f32),
                    1.9,
                    Some((content, Color::srgb(1.0, 0.92, 0.7))),
                )
            }
            WorldLabel::Site(i) => {
                let def = &w.defs.sites[i];
                let (x, _) = def.center();
                (1, tile_world(x, def.area[1] as f32 - 0.5), 0.3, None)
            }
        };
        if let (Some((content, c)), Some(mut text), Some(mut color)) = (content, text, color) {
            if text.0 != content {
                text.0 = content;
            }
            if color.0 != c {
                color.0 = c;
            }
        }
        match camera.world_to_viewport(cam_t, pos + Vec3::Y * height) {
            Ok(screen) => {
                let size = computed.size() * computed.inverse_scale_factor();
                let top_left = Vec2::new(screen.x - size.x / 2.0, screen.y - size.y);
                placed.push((priority, entity, top_left, size));
                vis.set_if_neq(Visibility::Inherited);
            }
            Err(_) => {
                vis.set_if_neq(Visibility::Hidden);
            }
        }
    }

    placed.sort_by_key(|(priority, entity, ..)| (*priority, *entity));
    let mut done: Vec<Rect> = Vec::new();
    for (priority, entity, mut top_left, size) in placed {
        // Building labels stay put; banners and characters move out of the way.
        if priority >= 1 {
            for _ in 0..8 {
                let r = Rect::from_corners(top_left, top_left + size);
                let Some(other) = done.iter().find(|o| !o.intersect(r).is_empty()) else {
                    break;
                };
                top_left.y = other.min.y - size.y - 1.0;
            }
        }
        done.push(Rect::from_corners(top_left, top_left + size));
        if let Ok((.., mut node, _, _)) = labels.get_mut(entity) {
            node.left = Val::Px(top_left.x);
            node.top = Val::Px(top_left.y);
        }
    }
}

/// Explains the tile under the cursor: tree tier and whether you can cut it,
/// regrowth time, or storage contents.
fn update_hover_tip(
    sim: Res<Sim>,
    hovered: Res<HoveredTile>,
    cursor: Res<Cursor>,
    mut tip: Query<(&mut Text, &mut TextColor, &mut Node, &mut Visibility), With<HoverTip>>,
) {
    let Ok((mut text, mut color, mut node, mut vis)) = tip.single_mut() else {
        return;
    };
    let w = &sim.world;
    let tps = w.defs.ticks_per_second as u64;
    let described = hovered.0.and_then(|p| {
        if let Some(t) = w.tree_at(p) {
            let def = w.defs.tree(t.kind);
            let site = t
                .site
                .map(|s| format!("\n{}", w.defs.site(s).name))
                .unwrap_or_default();
            return Some(match t.state {
                TreeState::Depleted { regrow_at } => (
                    format!(
                        "{} stump\nRegrows in {}s{site}",
                        def.name,
                        regrow_at.saturating_sub(w.tick).div_ceil(tps)
                    ),
                    MUTED,
                ),
                _ => {
                    let skill = &w.defs.skill(def.skill).name;
                    let mut s = format!(
                        "{} - needs {skill} Lv {}\n{}/{} logs, {} XP each",
                        def.name,
                        def.level_required,
                        t.logs_left(),
                        def.logs,
                        def.xp_per_log
                    );
                    if let Some(sid) = t.claimed_by {
                        s += &format!("\nBeing cut by {}", w.spirits[sid].name);
                    }
                    let level = w.player.level(def.skill, &w.defs.xp_table);
                    let (line, c) = if w.player_can_cut(t.kind) {
                        ("You can cut this".to_string(), GOOD)
                    } else {
                        (format!("Locked: you are Lv {level}"), BAD)
                    };
                    (format!("{s}\n{line}{site}"), c)
                }
            });
        }
        let b = w.building_at(p)?;
        let def = w.defs.building(b.kind);
        let stored = w.defs.describe_amounts(b.storage.contents());
        if !b.is_complete() {
            return Some((format!("{}\nUnder construction", b.name), MUTED));
        }
        if def.is_station {
            let level = w.player_station_level(b);
            let recipes: Vec<String> = w
                .defs
                .station_recipes(b.kind)
                .into_iter()
                .map(|r| {
                    let d = w.defs.recipe(r);
                    let inputs: Vec<_> = d
                        .inputs
                        .iter()
                        .map(|(res, n)| format!("{n} {}", w.defs.resource(*res).name))
                        .collect();
                    let lock = if level >= d.level_required {
                        String::new()
                    } else {
                        format!(" - needs Lv {}", d.level_required)
                    };
                    format!(
                        "{}: {} -> {} {}{lock}",
                        d.name,
                        inputs.join(", "),
                        d.output.1,
                        w.defs.resource(d.output.0).name
                    )
                })
                .collect();
            return Some((
                format!(
                    "{}\nHolds {stored}\n{}\n{}",
                    b.name,
                    w.workshop_status().label(),
                    recipes.join("\n")
                ),
                Color::WHITE,
            ));
        }
        if def.capacity == 0 {
            let text = format!("{}\n+{} Main Storage capacity", b.name, def.capacity_bonus);
            return Some((text, Color::WHITE));
        }
        Some((
            format!(
                "{}\nHolds {stored} ({}/{})",
                b.name,
                b.storage.total(),
                b.storage.capacity
            ),
            Color::WHITE,
        ))
    });
    match (described, cursor.0) {
        (Some((content, c)), Some(cursor)) => {
            if text.0 != content {
                text.0 = content;
            }
            color.0 = c;
            node.left = Val::Px(cursor.x + 18.0);
            node.top = Val::Px(cursor.y + 12.0);
            vis.set_if_neq(Visibility::Inherited);
        }
        _ => {
            vis.set_if_neq(Visibility::Hidden);
        }
    }
}

fn tile_square(gizmos: &mut Gizmos, p: TilePos, half: f32, y: f32, color: Color) {
    let c = tile_world(p.x as f32, p.y as f32) + Vec3::Y * y;
    let corners = [
        c + Vec3::new(-half, 0.0, -half),
        c + Vec3::new(half, 0.0, -half),
        c + Vec3::new(half, 0.0, half),
        c + Vec3::new(-half, 0.0, half),
    ];
    for i in 0..4 {
        gizmos.line(corners[i], corners[(i + 1) % 4], color);
    }
}

/// Paths, claimed trees and construction sites, drawn just above the ground.
fn draw_overlays(sim: Res<Sim>, mut gizmos: Gizmos) {
    let w = &sim.world;
    let lift = Vec3::Y * 0.06;
    draw_site_outlines(w, &mut gizmos);
    let mut draw_path = |mover: &spirit_sim::entities::Mover, color: Color| {
        let (x, y) = mover.visual_pos();
        let mut prev = tile_world(x, y) + lift;
        for p in &mover.path {
            let next = tile_world(p.x as f32, p.y as f32) + lift;
            gizmos.line(prev, next, color);
            prev = next;
        }
    };
    draw_path(&w.player.actor.mover, Color::srgba(1.0, 1.0, 1.0, 0.7));
    for s in &w.spirits {
        let c = w.defs.role(s.role).color;
        draw_path(&s.actor.mover, Color::srgba(c[0], c[1], c[2], 0.8));
    }
    // Status ring under each spirit, matching its label colour.
    let flat = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
    for s in &w.spirits {
        let (x, y) = s.actor.mover.visual_pos();
        let color = spirit_status(s).0.color();
        let at = tile_world(x, y) + Vec3::Y * 0.05;
        gizmos.circle(Isometry3d::new(at, flat), 0.32, color);
        gizmos.circle(Isometry3d::new(at, flat), 0.28, color);
    }
    // Red rings on nearby trees the player is not high enough level to cut.
    let me = w.player.actor.pos();
    for t in &w.trees {
        let near = (t.pos.x - me.x).abs().max((t.pos.y - me.y).abs()) <= LOCK_RING_RANGE;
        if near && t.is_grown() && !w.player_can_cut(t.kind) {
            let at = tile_world(t.pos.x as f32, t.pos.y as f32) + Vec3::Y * 0.05;
            gizmos.circle(Isometry3d::new(at, flat), 0.42, BAD.with_alpha(0.8));
        }
    }
    for t in &w.trees {
        if let Some(sid) = t.claimed_by {
            let c = w.defs.role(w.spirits[sid].role).color;
            tile_square(
                &mut gizmos,
                t.pos,
                0.45,
                0.04,
                Color::srgb(c[0], c[1], c[2]),
            );
        }
    }
    for p in w.projects.iter().filter(|p| p.is_active()) {
        let color = if p.blocked.is_some() {
            Color::srgb(1.0, 0.3, 0.2)
        } else {
            Color::srgb(1.0, 0.9, 0.2)
        };
        tile_square(&mut gizmos, p.site, 0.55, 0.05, color);
    }
}

/// Faint outline of each gathering site (blue when reserved for the player).
fn draw_site_outlines(w: &spirit_sim::World, gizmos: &mut Gizmos) {
    for (i, site) in w.defs.sites.iter().enumerate() {
        let [x0, y0, x1, y1] = site.area;
        let (a, b) = (
            tile_world(x0 as f32 - 0.5, y0 as f32 - 0.5),
            tile_world(x1 as f32 + 0.5, y1 as f32 + 0.5),
        );
        let color = if w.settlement.site_open[i] {
            Color::srgba(0.9, 0.95, 0.6, 0.35)
        } else {
            Color::srgba(0.6, 0.8, 1.0, 0.5)
        };
        let y = 0.03;
        let corners = [
            Vec3::new(a.x, y, a.z),
            Vec3::new(b.x, y, a.z),
            Vec3::new(b.x, y, b.z),
            Vec3::new(a.x, y, b.z),
        ];
        for k in 0..4 {
            gizmos.line(corners[k], corners[(k + 1) % 4], color);
        }
    }
}
