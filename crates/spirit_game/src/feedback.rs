//! What the world does to tell you something happened: sounds, and a burst of
//! light where it happened. It only reads the simulation, so it can't change a
//! game, and a machine with no sound device just stays quiet.

use crate::sim_runner::Sim;
use crate::world_view::{WorldEntity, tile_world};
use bevy::audio::{PlaybackMode, Volume};
use bevy::prelude::*;
use spirit_sim::world::World;

pub struct FeedbackPlugin;

impl Plugin for FeedbackPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Seen>()
            .init_resource::<Sound>()
            .add_systems(Update, (watch_world, move_sparks, toggle_sound));
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
