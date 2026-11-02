//! Draws the simulation with plain colored sprites and gizmos, and turns
//! map clicks into player commands.

use crate::sim_runner::Sim;
use bevy::prelude::*;
use spirit_sim::Command;
use spirit_sim::entities::{BuildingState, ProjectState};
use spirit_sim::grid::{Terrain, TilePos};

pub const TILE: f32 = 24.0;
/// Screen space reserved by the UI: right-hand panel and bottom event log.
pub const PANEL_WIDTH: f32 = 430.0;
pub const LOG_HEIGHT: f32 = 230.0;

#[derive(Component)]
struct WorldEntity;
#[derive(Component)]
struct TreeVis(usize);
#[derive(Component)]
struct BuildingVis(usize);
#[derive(Component)]
struct BuildingLabel(usize);
#[derive(Component)]
struct SpiritVis(usize);
#[derive(Component)]
struct SpiritLabel(usize);
#[derive(Component)]
struct PlayerVis;

#[derive(Resource, Default)]
struct Spawned {
    generation: Option<u32>,
    buildings: usize,
    spirits: usize,
}

fn to_world(x: f32, y: f32) -> Vec2 {
    Vec2::new(x * TILE, -y * TILE)
}

fn rgb(c: [f32; 3]) -> Color {
    Color::srgb(c[0], c[1], c[2])
}

pub struct RenderPlugin;

impl Plugin for RenderPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Spawned>()
            .add_systems(Startup, spawn_camera)
            .add_systems(
                Update,
                (
                    rebuild_on_restart,
                    spawn_new_entities,
                    (
                        sync_trees,
                        sync_buildings,
                        sync_actors,
                        draw_overlays,
                        position_camera,
                        map_clicks,
                    ),
                )
                    .chain(),
            );
    }
}

fn spawn_camera(mut commands: Commands) {
    commands.spawn(Camera2d);
}

fn label(text: String, size: f32, offset: Vec2) -> impl Bundle {
    (
        Text2d::new(text),
        TextFont::from_font_size(size),
        TextColor(Color::WHITE),
        Transform::from_translation(offset.extend(5.0)),
    )
}

fn rebuild_on_restart(
    mut commands: Commands,
    sim: Res<Sim>,
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

    for y in 0..w.grid.height {
        for x in 0..w.grid.width {
            let color = match w.grid.terrain(TilePos::new(x, y)).unwrap() {
                Terrain::Grass if (x + y) % 2 == 0 => Color::srgb(0.20, 0.36, 0.18),
                Terrain::Grass => Color::srgb(0.22, 0.38, 0.19),
                Terrain::Water => Color::srgb(0.15, 0.30, 0.55),
                Terrain::Rock => Color::srgb(0.42, 0.42, 0.44),
                Terrain::Bridge => Color::srgb(0.50, 0.36, 0.20),
            };
            commands.spawn((
                WorldEntity,
                Sprite::from_color(color, Vec2::splat(TILE)),
                Transform::from_translation(to_world(x as f32, y as f32).extend(0.0)),
            ));
        }
    }
    for t in &w.trees {
        commands.spawn((
            WorldEntity,
            TreeVis(t.id),
            Sprite::from_color(Color::WHITE, Vec2::splat(TILE * 0.8)),
            Transform::from_translation(to_world(t.pos.x as f32, t.pos.y as f32).extend(1.0)),
        ));
    }
    let p = w.player.actor.pos();
    commands
        .spawn((
            WorldEntity,
            PlayerVis,
            Sprite::from_color(Color::srgb(0.95, 0.95, 0.95), Vec2::splat(TILE * 0.55)),
            Transform::from_translation(to_world(p.x as f32, p.y as f32).extend(3.0)),
        ))
        .with_children(|c| {
            c.spawn(label("You".into(), 11.0, Vec2::new(0.0, 14.0)));
        });
}

fn spawn_new_entities(mut commands: Commands, sim: Res<Sim>, mut spawned: ResMut<Spawned>) {
    let w = &sim.world;
    for b in &w.buildings[spawned.buildings..] {
        commands
            .spawn((
                WorldEntity,
                BuildingVis(b.id),
                Sprite::from_color(Color::WHITE, Vec2::splat(TILE * 0.95)),
                Transform::from_translation(to_world(b.pos.x as f32, b.pos.y as f32).extend(1.0)),
            ))
            .with_children(|c| {
                c.spawn((
                    BuildingLabel(b.id),
                    label(b.name.clone(), 11.0, Vec2::new(0.0, -18.0)),
                ));
            });
    }
    spawned.buildings = w.buildings.len();

    for s in &w.spirits[spawned.spirits..] {
        let color = rgb(w.defs.role(s.role).color);
        commands
            .spawn((
                WorldEntity,
                SpiritVis(s.id),
                Sprite::from_color(color, Vec2::splat(TILE * 0.6)),
                Transform::default(),
            ))
            .with_children(|c| {
                c.spawn((
                    SpiritLabel(s.id),
                    label(s.name.clone(), 11.0, Vec2::new(0.0, 15.0)),
                ));
            });
    }
    spawned.spirits = w.spirits.len();
}

fn sync_trees(sim: Res<Sim>, mut trees: Query<(&TreeVis, &mut Sprite)>) {
    let w = &sim.world;
    for (vis, mut sprite) in &mut trees {
        let t = &w.trees[vis.0];
        let max = w.defs.tree(t.kind).logs as f32;
        if t.is_grown() {
            let fullness = t.logs_left() as f32 / max;
            sprite.color = Color::srgb(0.05, 0.30 + 0.35 * fullness, 0.08);
            sprite.custom_size = Some(Vec2::splat(TILE * (0.55 + 0.35 * fullness)));
        } else {
            sprite.color = Color::srgb(0.45, 0.30, 0.15);
            sprite.custom_size = Some(Vec2::splat(TILE * 0.35));
        }
    }
}

fn sync_buildings(
    sim: Res<Sim>,
    mut buildings: Query<(&BuildingVis, &mut Sprite)>,
    mut labels: Query<(&BuildingLabel, &mut Text2d)>,
) {
    let w = &sim.world;
    let logs = w.defs.objective.resource;
    for (vis, mut sprite) in &mut buildings {
        let b = &w.buildings[vis.0];
        let c = w.defs.building(b.kind).color;
        sprite.color = match b.state {
            BuildingState::Complete => rgb(c),
            BuildingState::UnderConstruction { .. } => Color::srgba(c[0], c[1], c[2], 0.35),
        };
    }
    for (vis, mut text) in &mut labels {
        let b = &w.buildings[vis.0];
        text.0 = match b.state {
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
    }
}

fn sync_actors(
    sim: Res<Sim>,
    mut player: Query<&mut Transform, (With<PlayerVis>, Without<SpiritVis>)>,
    mut spirits: Query<(&SpiritVis, &mut Transform), Without<PlayerVis>>,
    mut labels: Query<(&SpiritLabel, &mut Text2d)>,
) {
    let w = &sim.world;
    if let Ok(mut t) = player.single_mut() {
        let (x, y) = w.player.actor.mover.visual_pos();
        t.translation = to_world(x, y).extend(3.0);
    }
    for (vis, mut t) in &mut spirits {
        let (x, y) = w.spirits[vis.0].actor.mover.visual_pos();
        t.translation = to_world(x, y).extend(3.0 + vis.0 as f32 * 0.01);
    }
    for (vis, mut text) in &mut labels {
        let s = &w.spirits[vis.0];
        let carried = s.actor.inventory.total();
        text.0 = if carried > 0 {
            format!("{} L{} [{carried}]", s.name, s.level())
        } else {
            format!("{} L{}", s.name, s.level())
        };
    }
}

/// Paths, tree claims and construction sites drawn as gizmos.
fn draw_overlays(sim: Res<Sim>, mut gizmos: Gizmos) {
    let w = &sim.world;
    let mut draw_path = |mover: &spirit_sim::entities::Mover, color: Color| {
        let (x, y) = mover.visual_pos();
        let mut prev = to_world(x, y);
        for p in &mover.path {
            let next = to_world(p.x as f32, p.y as f32);
            gizmos.line_2d(prev, next, color);
            prev = next;
        }
    };
    draw_path(&w.player.actor.mover, Color::srgba(1.0, 1.0, 1.0, 0.5));
    for s in &w.spirits {
        let c = w.defs.role(s.role).color;
        draw_path(&s.actor.mover, Color::srgba(c[0], c[1], c[2], 0.6));
    }
    for t in &w.trees {
        if let Some(sid) = t.claimed_by {
            let c = w.defs.role(w.spirits[sid].role).color;
            gizmos.circle_2d(
                to_world(t.pos.x as f32, t.pos.y as f32),
                TILE * 0.55,
                rgb(c),
            );
        }
    }
    for p in w.projects.iter().filter(|p| p.is_active()) {
        let color = if p.blocked.is_some() {
            Color::srgb(1.0, 0.3, 0.2)
        } else {
            Color::srgb(1.0, 0.9, 0.2)
        };
        gizmos.rect_2d(
            to_world(p.site.x as f32, p.site.y as f32),
            Vec2::splat(TILE * 1.1),
            color,
        );
    }
}

fn position_camera(sim: Res<Sim>, mut camera: Query<&mut Transform, With<Camera2d>>) {
    let Ok(mut t) = camera.single_mut() else {
        return;
    };
    let g = &sim.world.grid;
    let center = to_world((g.width - 1) as f32 / 2.0, (g.height - 1) as f32 / 2.0);
    t.translation.x = center.x + PANEL_WIDTH / 2.0;
    t.translation.y = center.y - LOG_HEIGHT / 2.0;
}

fn map_clicks(
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    camera: Query<(&Camera, &GlobalTransform)>,
    mut sim: ResMut<Sim>,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let (Ok(window), Ok((camera, cam_t))) = (windows.single(), camera.single()) else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    if cursor.x > window.width() - PANEL_WIDTH || cursor.y > window.height() - LOG_HEIGHT {
        return; // Click landed on the UI.
    }
    let Ok(world) = camera.viewport_to_world_2d(cam_t, cursor) else {
        return;
    };
    let tile = TilePos::new(
        (world.x / TILE).round() as i32,
        (-world.y / TILE).round() as i32,
    );
    sim.command(Command::PlayerInteract(tile));
}
