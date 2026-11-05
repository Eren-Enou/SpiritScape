//! 2.5D view: a tilted perspective camera that follows the player over the
//! tile grid, lighting, and player input (WASD steps, clicks, E to use).

use crate::script::Cursor;
use crate::sim_runner::Sim;
use crate::ui::{LOG_HEIGHT, PANEL_WIDTH, UiVisible, cursor_over_panel};
use crate::world_view::tile_world;
use bevy::input::mouse::AccumulatedMouseScroll;
use bevy::prelude::*;
use spirit_sim::Command;
use spirit_sim::grid::TilePos;

/// Camera viewing direction: from the focus point up and toward the viewer.
const VIEW_DIR: Vec3 = Vec3::new(0.0, 1.25, 1.0);
const MIN_DISTANCE: f32 = 5.0;
const MAX_DISTANCE: f32 = 45.0;

#[derive(Resource)]
struct CameraRig {
    focus: Vec3,
    distance: f32,
    overview: bool,
}

/// Tile under the mouse cursor, if any.
#[derive(Resource, Default)]
pub struct HoveredTile(pub Option<TilePos>);

/// Last direction the player walked, used by `E` to pick what to use.
#[derive(Resource)]
struct Facing(IVec2);

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(CameraRig {
            focus: Vec3::ZERO,
            distance: 13.0,
            overview: false,
        })
        .insert_resource(Facing(IVec2::new(0, 1)))
        .init_resource::<HoveredTile>()
        .insert_resource(GlobalAmbientLight {
            brightness: 250.0,
            ..default()
        })
        .insert_resource(ClearColor(Color::srgb(0.55, 0.75, 0.90)))
        .add_systems(Startup, spawn_camera_and_light)
        .add_systems(
            Update,
            (
                keyboard_movement,
                camera_controls,
                follow_camera,
                hover_tile,
                map_clicks,
            )
                .chain(),
        );
    }
}

fn spawn_camera_and_light(mut commands: Commands, sim: Res<Sim>, mut rig: ResMut<CameraRig>) {
    // The demo's action spans the whole map, so it starts zoomed out.
    rig.overview = sim.scenario == spirit_sim::ScenarioKind::Demo;
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 12.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 4_500.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(-8.0, 20.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

/// Held WASD / arrow keys walk the player one tile at a time.
fn keyboard_movement(
    keys: Res<ButtonInput<KeyCode>>,
    mut sim: ResMut<Sim>,
    mut facing: ResMut<Facing>,
) {
    if sim.menu {
        return;
    }
    const DIRS: [([KeyCode; 2], IVec2); 4] = [
        ([KeyCode::KeyW, KeyCode::ArrowUp], IVec2::new(0, -1)),
        ([KeyCode::KeyS, KeyCode::ArrowDown], IVec2::new(0, 1)),
        ([KeyCode::KeyA, KeyCode::ArrowLeft], IVec2::new(-1, 0)),
        ([KeyCode::KeyD, KeyCode::ArrowRight], IVec2::new(1, 0)),
    ];
    if let Some((_, dir)) = DIRS.iter().find(|(k, _)| keys.any_pressed(*k)) {
        facing.0 = *dir;
        // Repeated every frame while held; the sim ignores repeats of the same step.
        let _ = sim.world.apply(Command::PlayerStep {
            dx: dir.x,
            dy: dir.y,
        });
    }

    if keys.just_pressed(KeyCode::KeyX) {
        sim.command(Command::PlayerStop);
    }

    if keys.just_pressed(KeyCode::KeyE) {
        let w = &sim.world;
        let pos = w.player.actor.pos();
        let usable = |p: TilePos| {
            w.tree_at(p).is_some_and(|t| t.is_grown())
                || w.building_at(p).is_some_and(|b| b.is_complete())
        };
        let facing_tile = TilePos::new(pos.x + facing.0.x, pos.y + facing.0.y);
        let target = std::iter::once(facing_tile)
            .chain(pos.neighbors4())
            .find(|p| usable(*p));
        match target {
            Some(p) => sim.command(Command::PlayerInteract(p)),
            None => sim.message = "Nothing to use here - stand next to a tree or storage".into(),
        }
    }
}

fn camera_controls(
    keys: Res<ButtonInput<KeyCode>>,
    scroll: Res<AccumulatedMouseScroll>,
    ui: Res<UiVisible>,
    cursor: Res<Cursor>,
    windows: Query<&Window>,
    mut rig: ResMut<CameraRig>,
) {
    if keys.just_pressed(KeyCode::KeyC) {
        rig.overview = !rig.overview;
    }
    let over_panel = windows
        .single()
        .is_ok_and(|w| cursor_over_panel(w, &cursor, ui.0));
    if scroll.delta.y != 0.0 && !over_panel {
        rig.distance =
            (rig.distance * (1.0 - scroll.delta.y * 0.1)).clamp(MIN_DISTANCE, MAX_DISTANCE);
    }
}

fn follow_camera(
    time: Res<Time>,
    sim: Res<Sim>,
    ui: Res<UiVisible>,
    mut rig: ResMut<CameraRig>,
    windows: Query<&Window>,
    mut camera: Query<(&mut Transform, &Projection), With<Camera3d>>,
) {
    let Ok((mut tf, projection)) = camera.single_mut() else {
        return;
    };
    let g = &sim.world.grid;
    let (target, distance) = if rig.overview {
        let center = tile_world((g.width - 1) as f32 / 2.0, (g.height - 1) as f32 / 2.0);
        (center, 46.0)
    } else {
        let (x, y) = sim.world.player.actor.mover.visual_pos();
        (tile_world(x, y), rig.distance)
    };
    let t = (time.delta_secs() * 6.0).min(1.0);
    rig.focus = rig.focus.lerp(target, t);
    let eye = rig.focus + VIEW_DIR.normalize() * distance;
    *tf = Transform::from_translation(eye).looking_at(rig.focus, Vec3::Y);

    // Keep the focus centred in the area not covered by the UI panels.
    if ui.0
        && let (Ok(window), Projection::Perspective(p)) = (windows.single(), projection)
    {
        let world_per_px = 2.0 * distance * (p.fov / 2.0).tan() / window.height();
        let shift = tf.right() * (PANEL_WIDTH / 2.0 * world_per_px)
            - tf.up() * (LOG_HEIGHT / 2.0 * world_per_px);
        tf.translation += shift;
    }
}

fn cursor_tile(
    window: &Window,
    cursor: &Cursor,
    camera: &Camera,
    cam_t: &GlobalTransform,
    ui_visible: bool,
    world: &spirit_sim::World,
) -> Option<TilePos> {
    let cursor = cursor.0?;
    if ui_visible
        && (cursor.x > window.width() - PANEL_WIDTH || cursor.y > window.height() - LOG_HEIGHT)
    {
        return None; // Over the UI.
    }
    let ray = camera.viewport_to_world(cam_t, cursor).ok()?;
    if ray.direction.y.abs() < 1e-4 {
        return None;
    }
    // The tile where the ray crosses height `h`.
    let tile_at = |h: f32| {
        let t = (h - ray.origin.y) / ray.direction.y;
        (t >= 0.0).then(|| {
            let hit = ray.origin + *ray.direction * t;
            TilePos::new(hit.x.round() as i32, hit.z.round() as i32)
        })
    };
    // Walk down the ray so pointing at a tree crown or a roof picks that
    // object's tile, not the ground tile behind it.
    let mut h = OBJECT_HEIGHT_MAX;
    while h > 0.0 {
        if let Some(p) = tile_at(h)
            && object_height(world, p) >= h
        {
            return Some(p);
        }
        h -= 0.1;
    }
    tile_at(0.0)
}

/// Tallest thing that can stand on a tile, in world units.
const OBJECT_HEIGHT_MAX: f32 = 2.0;

/// Rough height of what stands on a tile, for mouse picking.
fn object_height(world: &spirit_sim::World, p: TilePos) -> f32 {
    if let Some(t) = world.tree_at(p) {
        return if t.is_grown() { 1.5 } else { 0.3 };
    }
    if world.building_at(p).is_some() {
        return 1.8;
    }
    0.0
}

fn hover_tile(
    windows: Query<&Window>,
    camera: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    ui: Res<UiVisible>,
    cursor: Res<Cursor>,
    sim: Res<Sim>,
    mut hovered: ResMut<HoveredTile>,
    mut gizmos: Gizmos,
) {
    let (Ok(window), Ok((camera, cam_t))) = (windows.single(), camera.single()) else {
        return;
    };
    hovered.0 = cursor_tile(window, &cursor, camera, cam_t, ui.0, &sim.world)
        .filter(|p| sim.world.grid.in_bounds(*p));
    if let Some(p) = hovered.0 {
        let c = tile_world(p.x as f32, p.y as f32) + Vec3::Y * 0.03;
        let h = 0.48;
        let corners = [
            Vec3::new(-h, 0.0, -h),
            Vec3::new(h, 0.0, -h),
            Vec3::new(h, 0.0, h),
            Vec3::new(-h, 0.0, h),
        ];
        for i in 0..4 {
            gizmos.line(
                c + corners[i],
                c + corners[(i + 1) % 4],
                Color::srgba(1.0, 1.0, 1.0, 0.8),
            );
        }
    }
}

fn map_clicks(
    mouse: Res<ButtonInput<MouseButton>>,
    hovered: Res<HoveredTile>,
    mut sim: ResMut<Sim>,
) {
    if !sim.menu
        && mouse.just_pressed(MouseButton::Left)
        && let Some(tile) = hovered.0
    {
        if let Some(kind) = sim.placing {
            sim.command(Command::RequestBuildingAt { kind, site: tile });
            if !sim.message.starts_with("Can't") {
                sim.placing = None;
            }
            return;
        }
        sim.command(Command::PlayerInteract(tile));
    }
}
