//! What the world does to tell you something happened: sounds, and a burst of
//! light where it happened. It only reads the simulation, so it can't change a
//! game, and a machine with no sound device just stays quiet.

use crate::sim_runner::Sim;
use crate::visuals::Visuals;
use crate::world_view::{WorldEntity, tile_world};
use bevy::audio::{PlaybackMode, Volume};
use bevy::input::InputSystems;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use spirit_sim::stories::MAX_NAME;
use spirit_sim::world::{Command, World};

pub struct FeedbackPlugin;

impl Plugin for FeedbackPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Seen>()
            .init_resource::<Sound>()
            .init_resource::<Closing>()
            .add_systems(Startup, spawn_closing_screen)
            .add_systems(PreUpdate, type_name.after(InputSystems))
            .add_systems(
                Update,
                (
                    watch_world,
                    move_sparks,
                    toggle_sound,
                    tint_seasons,
                    fall_snow,
                    keep_hearth,
                    show_closing_screen,
                ),
            );
    }
}

/// Whether sound is on. `M` toggles it.
#[derive(Resource)]
struct Sound {
    on: bool,
}

impl Default for Sound {
    fn default() -> Self {
        Self { on: true }
    }
}

/// What the world looked like last frame.
#[derive(Resource, Default)]
struct Seen {
    started: bool,
    levels: u32,
    built: usize,
    notices: u64,
    xp: Vec<u64>,
    season: Option<String>,
    ended: bool,
}

/// A spark rising from something that just happened.
#[derive(Component)]
struct Spark {
    velocity: Vec3,
    life: f32,
}

fn total_levels(w: &World) -> u32 {
    (0..w.defs.skills.len())
        .map(|i| spirit_sim::defs::SkillId(i as u16))
        .map(|s| w.player.level(s, &w.defs.xp_table))
        .sum()
}

fn skill_xp(w: &World) -> Vec<u64> {
    (0..w.defs.skills.len())
        .map(|i| w.player.xp(spirit_sim::defs::SkillId(i as u16)))
        .collect()
}

fn play(commands: &mut Commands, assets: &AssetServer, name: &str, volume: f32) {
    commands.spawn((
        AudioPlayer::new(assets.load(format!("audio/{name}.wav"))),
        PlaybackSettings {
            mode: PlaybackMode::Despawn,
            volume: Volume::Linear(volume),
            ..default()
        },
    ));
}

fn burst(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    at: Vec3,
    color: Color,
    count: u32,
) {
    let mesh = meshes.add(Cuboid::new(0.09, 0.09, 0.09));
    let material = materials.add(StandardMaterial {
        base_color: color,
        emissive: color.to_linear() * 3.0,
        ..default()
    });
    for i in 0..count {
        let a = i as f32 / count as f32 * std::f32::consts::TAU;
        commands.spawn((
            WorldEntity,
            Spark {
                velocity: Vec3::new(a.cos() * 0.7, 1.6 + (i % 3) as f32 * 0.5, a.sin() * 0.7),
                life: 1.2,
            },
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material.clone()),
            Transform::from_translation(at + Vec3::Y * 0.8),
        ));
    }
}

#[allow(clippy::too_many_arguments)]
fn watch_world(
    mut commands: Commands,
    sim: Res<Sim>,
    assets: Res<AssetServer>,
    sound: Res<Sound>,
    mut seen: ResMut<Seen>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut closing: ResMut<Closing>,
) {
    let w = &sim.world;
    let levels = total_levels(w);
    let built = w.buildings.iter().filter(|b| b.is_complete()).count();
    let xp = skill_xp(w);
    let season = w.season().map(|s| s.id.clone());
    let ended = w.settlement.ended_at.is_some();
    // A new or loaded game isn't news.
    if !seen.started || xp.iter().zip(&seen.xp).any(|(now, was)| now < was) || built < seen.built {
        *seen = Seen {
            started: true,
            levels,
            built,
            notices: w.notices.total,
            xp,
            season,
            ended,
        };
        return;
    }
    let (px, py) = w.player.actor.mover.visual_pos();
    let player = tile_world(px, py);
    if levels > seen.levels {
        burst(
            &mut commands,
            &mut meshes,
            &mut materials,
            player,
            Color::srgb(1.0, 0.8, 0.3),
            14,
        );
        if sound.on {
            play(&mut commands, &assets, "level", 0.6);
        }
    }
    if built > seen.built {
        let newest = w.buildings.iter().rev().find(|b| b.is_complete());
        if let Some(b) = newest {
            let at = tile_world(b.pos.x as f32, b.pos.y as f32);
            burst(
                &mut commands,
                &mut meshes,
                &mut materials,
                at,
                Color::srgb(0.7, 0.9, 1.0),
                18,
            );
        }
        if sound.on && !ended {
            play(&mut commands, &assets, "built", 0.5);
        }
    }
    if sound.on {
        let cut = |id: &str| {
            w.defs.skill_id(id).is_some_and(|s| {
                let i = s.0 as usize;
                xp.get(i).zip(seen.xp.get(i)).is_some_and(|(a, b)| a > b)
            })
        };
        if cut("woodcutting") {
            play(&mut commands, &assets, "chop", 0.3);
        }
        if cut("quarrying") {
            play(&mut commands, &assets, "stone", 0.3);
        }
        if cut("woodworking") || cut("masonry") {
            play(&mut commands, &assets, "saw", 0.25);
        }
        if season != seen.season && seen.season.is_some() {
            play(&mut commands, &assets, "season", 0.5);
        } else if w.notices.total > seen.notices && levels == seen.levels && built == seen.built {
            play(&mut commands, &assets, "notice", 0.25);
        }
        if ended && !seen.ended {
            play(&mut commands, &assets, "hearth", 0.8);
        }
    }
    if ended && !seen.ended {
        closing.waiting = true;
    }
    *seen = Seen {
        started: true,
        levels,
        built,
        notices: w.notices.total,
        xp,
        season,
        ended,
    };
}

fn move_sparks(
    mut commands: Commands,
    time: Res<Time>,
    mut sparks: Query<(Entity, &mut Spark, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (e, mut s, mut t) in &mut sparks {
        t.translation += s.velocity * dt;
        s.velocity.y -= 1.8 * dt;
        s.life -= dt;
        t.scale = Vec3::splat(s.life.clamp(0.0, 1.0));
        if s.life <= 0.0 {
            commands.entity(e).despawn();
        }
    }
}

fn toggle_sound(keys: Res<ButtonInput<KeyCode>>, mut sound: ResMut<Sound>, mut sim: ResMut<Sim>) {
    if keys.just_pressed(KeyCode::KeyM) {
        sound.on = !sound.on;
        sim.message = if sound.on { "Sound on" } else { "Sound off" }.into();
    }
}

/// The sheet of colour over the ground, which shows the season.
#[derive(Component)]
struct SeasonTint(Handle<StandardMaterial>);

fn tint_seasons(
    mut commands: Commands,
    sim: Res<Sim>,
    visuals: Res<Visuals>,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    sheet: Query<&SeasonTint>,
) {
    let w = &sim.world;
    let want = w
        .season()
        .and_then(|s| visuals.season_tints.get(&s.id))
        .copied()
        .unwrap_or([1.0, 1.0, 1.0, 0.0]);
    let Ok(tint) = sheet.single() else {
        let (width, height) = (w.grid.width as f32, w.grid.height as f32);
        let material = materials.add(StandardMaterial {
            base_color: Color::srgba(1.0, 1.0, 1.0, 0.0),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            ..default()
        });
        commands.spawn((
            SeasonTint(material.clone()),
            Mesh3d(meshes.add(Plane3d::default().mesh().size(width, height))),
            MeshMaterial3d(material),
            Transform::from_xyz(width / 2.0 - 0.5, 0.04, height / 2.0 - 0.5),
            Pickable::IGNORE,
        ));
        return;
    };
    let Some(mut material) = materials.get_mut(&tint.0) else {
        return;
    };
    let now = material.base_color.to_srgba();
    let step = (time.delta_secs() * 0.5).min(1.0);
    let mix = |a: f32, b: f32| a + (b - a) * step;
    material.base_color = Color::srgba(
        mix(now.red, want[0]),
        mix(now.green, want[1]),
        mix(now.blue, want[2]),
        mix(now.alpha, want[3]),
    );
}

/// Time since the last flake, a running number, and the look they share.
type SnowState = (f32, u32, Option<(Handle<Mesh>, Handle<StandardMaterial>)>);

/// A flake of snow on its way down.
#[derive(Component)]
struct Flake;

fn fall_snow(
    mut commands: Commands,
    sim: Res<Sim>,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut flakes: Query<(Entity, &mut Transform), With<Flake>>,
    mut state: Local<SnowState>,
) {
    let dt = time.delta_secs();
    for (e, mut t) in &mut flakes {
        t.translation.y -= 1.4 * dt;
        t.translation.x += 0.3 * dt;
        if t.translation.y < 0.05 {
            commands.entity(e).despawn();
        }
    }
    let w = &sim.world;
    if w.season().is_none_or(|s| s.id != "winter") {
        return;
    }
    state.0 += dt;
    if state.0 < 0.04 {
        return;
    }
    state.0 = 0.0;
    state.1 = state.1.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    let (mesh, material) = state
        .2
        .get_or_insert_with(|| {
            (
                meshes.add(Cuboid::new(0.05, 0.05, 0.05)),
                materials.add(StandardMaterial {
                    base_color: Color::WHITE,
                    unlit: true,
                    ..default()
                }),
            )
        })
        .clone();
    // This is only how it looks, so its numbers don't touch the simulation.
    let r = state.1;
    let (px, py) = w.player.actor.mover.visual_pos();
    let dx = (r >> 8 & 0xff) as f32 / 255.0 * 26.0 - 13.0;
    let dz = (r >> 16 & 0xff) as f32 / 255.0 * 18.0 - 9.0;
    commands.spawn((
        Flake,
        Mesh3d(mesh),
        MeshMaterial3d(material),
        Transform::from_xyz(px + dx, 7.0, py + dz),
        Pickable::IGNORE,
    ));
}

/// The light that stays on at the Hearth.
#[derive(Component)]
struct HearthLight;

fn keep_hearth(
    mut commands: Commands,
    sim: Res<Sim>,
    time: Res<Time>,
    lights: Query<Entity, With<HearthLight>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut since: Local<f32>,
) {
    let Some(p) = sim.world.hearth() else {
        for e in &lights {
            commands.entity(e).despawn();
        }
        return;
    };
    let at = tile_world(p.x as f32, p.y as f32);
    if lights.is_empty() {
        commands.spawn((
            HearthLight,
            PointLight {
                color: Color::srgb(1.0, 0.72, 0.38),
                intensity: 900_000.0,
                range: 16.0,
                ..default()
            },
            Transform::from_translation(at + Vec3::Y * 2.2),
        ));
    }
    *since += time.delta_secs();
    if *since > 0.6 {
        *since = 0.0;
        let glow = Color::srgb(1.0, 0.7, 0.3);
        burst(&mut commands, &mut meshes, &mut materials, at, glow, 3);
    }
}

/// Whether the closing screen is waiting to be read.
#[derive(Resource, Default)]
struct Closing {
    waiting: bool,
}

#[derive(Component)]
struct ClosingScreen;

#[derive(Component)]
struct ClosingText;

fn spawn_closing_screen(mut commands: Commands) {
    commands
        .spawn((
            ClosingScreen,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.03, 0.03, 0.05, 0.86)),
            GlobalZIndex(40),
            Visibility::Hidden,
            Pickable::IGNORE,
        ))
        .with_children(|p| {
            p.spawn((
                ClosingText,
                Text::new(""),
                TextFont::from_font_size(21.0),
                TextColor(Color::srgb(0.98, 0.90, 0.72)),
                TextLayout::justify(Justify::Center),
                Node {
                    max_width: Val::Px(820.0),
                    ..default()
                },
            ));
        });
}

fn show_closing_screen(
    sim: Res<Sim>,
    keys: Res<ButtonInput<KeyCode>>,
    mut closing: ResMut<Closing>,
    mut screen: Query<&mut Visibility, With<ClosingScreen>>,
    mut words: Query<&mut Text, With<ClosingText>>,
) {
    if closing.waiting && keys.just_pressed(KeyCode::Enter) {
        closing.waiting = false;
    }
    let shown = if closing.waiting {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    for mut v in &mut screen {
        if *v != shown {
            *v = shown;
        }
    }
    if closing.waiting
        && let (Ok(mut t), Some(story)) = (words.single_mut(), sim.world.ending_story())
    {
        let text = story.join("\n\n") + "\n\nPress Enter to carry on.";
        if t.0 != text {
            t.0 = text;
        }
    }
}

/// While a spirit is being named, the keyboard belongs to the name. Nothing
/// else in the game sees those key presses.
fn type_name(
    mut sim: ResMut<Sim>,
    mut typed: MessageReader<KeyboardInput>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
) {
    let Some(mut name) = sim.naming.clone() else {
        typed.clear();
        return;
    };
    let mut done = None;
    for e in typed.read() {
        if !e.state.is_pressed() {
            continue;
        }
        match &e.logical_key {
            Key::Character(c) if name.chars().count() < MAX_NAME => {
                name.extend(c.chars().filter(|c| c.is_alphanumeric() || *c == ' '));
            }
            Key::Space if name.chars().count() < MAX_NAME => name.push(' '),
            Key::Backspace => {
                name.pop();
            }
            Key::Enter => done = Some(true),
            Key::Escape => done = Some(false),
            _ => {}
        }
    }
    keys.reset_all();
    match done {
        Some(true) => {
            let id = sim.selected_spirit;
            sim.naming = None;
            sim.command(Command::RenameSpirit { id, name });
        }
        Some(false) => sim.naming = None,
        None => sim.naming = Some(name),
    }
}
