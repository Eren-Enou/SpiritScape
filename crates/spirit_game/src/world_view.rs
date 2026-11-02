//! The 3D world: terrain tiles, trees and buildings built from glTF models,
//! plus floating labels and gizmo overlays. Everything here only reads the
//! simulation.

use crate::sim_runner::Sim;
use crate::visuals::{Visuals, model, tile_hash};
use bevy::prelude::*;
use spirit_sim::entities::{BuildingState, ProjectState};
use spirit_sim::grid::{Terrain, TilePos};

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

/// A screen-space label that follows a point in the world.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub enum WorldLabel {
    Player,
    Spirit(usize),
    Building(usize),
}

/// Tile centre in world space.
pub fn tile_world(x: f32, y: f32) -> Vec3 {
    Vec3::new(x, 0.0, y)
}

pub struct WorldViewPlugin;

impl Plugin for WorldViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Spawned>().add_systems(
            Update,
            (
                rebuild_on_restart,
                spawn_new_buildings,
                (sync_trees, sync_buildings, update_labels, draw_overlays),
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
        TextShadow {
            offset: Vec2::splat(1.5),
            color: Color::srgba(0.0, 0.0, 0.0, 0.85),
        },
        Node {
            position_type: PositionType::Absolute,
            ..default()
        },
        bevy::picking::Pickable::IGNORE,
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

    for tree in &w.trees {
        let kind = &w.defs.tree(tree.kind).id;
        let Some(tv) = visuals.trees.get(kind) else {
            continue;
        };
        let h = tile_hash(tree.pos.x, tree.pos.y);
        let at = tile_world(tree.pos.x as f32, tree.pos.y as f32);
        let rot = Quat::from_rotation_y((h % 360) as f32);
        commands.spawn((
            WorldEntity,
            TreeModel(tree.id),
            model(&assets, &tv.models[h as usize % tv.models.len()]),
            Transform::from_translation(at)
                .with_rotation(rot)
                .with_scale(Vec3::splat(tv.scale)),
        ));
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
            BuildingState::Complete => b.storage.total(),
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

/// Positions name/status labels over characters and buildings.
fn update_labels(
    sim: Res<Sim>,
    camera: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    mut labels: Query<(
        &WorldLabel,
        &mut Text,
        &mut Node,
        &ComputedNode,
        &mut Visibility,
    )>,
) {
    let Ok((camera, cam_t)) = camera.single() else {
        return;
    };
    let w = &sim.world;
    let logs = w.defs.objective.resource;
    for (label, mut text, mut node, computed, mut vis) in &mut labels {
        let (pos, height, content) = match *label {
            WorldLabel::Player => {
                let (x, y) = w.player.actor.mover.visual_pos();
                (tile_world(x, y), 1.05, "You".to_string())
            }
            WorldLabel::Spirit(id) => {
                let s = &w.spirits[id];
                let (x, y) = s.actor.mover.visual_pos();
                let carried = s.actor.inventory.total();
                let content = if carried > 0 {
                    format!("{} L{} [{carried}]", s.name, s.level())
                } else {
                    format!("{} L{}", s.name, s.level())
                };
                (tile_world(x, y), 1.05, content)
            }
            WorldLabel::Building(id) => {
                let b = &w.buildings[id];
                let content = match b.state {
                    BuildingState::Complete => format!("{}: {}", b.name, b.storage.get(logs)),
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
                            _ => format!("{} (site {delivered}/{cost})", b.name),
                        }
                    }
                };
                (tile_world(b.pos.x as f32, b.pos.y as f32), 1.9, content)
            }
        };
        if text.0 != content {
            text.0 = content;
        }
        match camera.world_to_viewport(cam_t, pos + Vec3::Y * height) {
            Ok(screen) => {
                let size = computed.size() * computed.inverse_scale_factor();
                node.left = Val::Px(screen.x - size.x / 2.0);
                node.top = Val::Px(screen.y - size.y);
                vis.set_if_neq(Visibility::Inherited);
            }
            Err(_) => {
                vis.set_if_neq(Visibility::Hidden);
            }
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
