//! Animated KayKit characters for the player and spirits.
//!
//! The character files contain no animations, so Bevy's glTF loader doesn't
//! mark their bones as animation targets. Once a character's scene is
//! spawned we do what the loader would have done: put an `AnimationPlayer`
//! on the `Rig_Medium` root and give every bone an `AnimationTargetId` built
//! from its name path. Clips from the shared Rig_Medium animation files then
//! drive any character.

use crate::sim_runner::Sim;
use crate::visuals::{Visuals, model};
use crate::world_view::{Spawned, WorldEntity, WorldLabel, spawn_label, tile_world};
use bevy::animation::{AnimatedBy, AnimationTargetId};
use bevy::gltf::Gltf;
use bevy::prelude::*;
use bevy::world_serialization::WorldInstanceReady;
use spirit_sim::entities::Actor;
use spirit_sim::tasks::{Goal, Task};
use std::time::Duration;

const RIG_ROOT: &str = "Rig_Medium";
const HAND_SLOT: &str = "handslot.r";

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Who {
    Player,
    Spirit(usize),
}

#[derive(Component)]
struct Character {
    who: Who,
    /// Entity holding the AnimationPlayer, once the rig is set up.
    rig: Option<Entity>,
    current: Option<AnimationNodeIndex>,
}

/// On the model entity: rig setup still to do once its scene has spawned.
#[derive(Component)]
struct PendingRig {
    hand_item: Option<String>,
    ghost: Option<Color>,
}

#[derive(Component)]
struct SceneReady;

#[derive(Resource)]
struct CharacterAnims {
    files: Vec<Handle<Gltf>>,
    ready: Option<AnimSet>,
}

#[derive(Clone)]
struct AnimSet {
    graph: Handle<AnimationGraph>,
    idle: AnimationNodeIndex,
    walk: AnimationNodeIndex,
    work: AnimationNodeIndex,
}

pub struct CharactersPlugin;

impl Plugin for CharactersPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, load_animations).add_systems(
            Update,
            (
                build_anim_graph,
                spawn_characters,
                setup_rigs,
                sync_characters,
            )
                .chain(),
        );
    }
}

fn load_animations(mut commands: Commands, visuals: Res<Visuals>, assets: Res<AssetServer>) {
    let files = visuals
        .characters
        .animations
        .iter()
        .map(|p| assets.load(p.clone()))
        .collect();
    commands.insert_resource(CharacterAnims { files, ready: None });
}

fn build_anim_graph(
    mut anims: ResMut<CharacterAnims>,
    gltfs: Res<Assets<Gltf>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    visuals: Res<Visuals>,
) {
    if anims.ready.is_some() {
        return;
    }
    let Some(loaded) = anims
        .files
        .iter()
        .map(|h| gltfs.get(h))
        .collect::<Option<Vec<_>>>()
    else {
        return;
    };
    let find = |name: &str| {
        loaded
            .iter()
            .find_map(|g| g.named_animations.get(name).cloned())
            .unwrap_or_else(|| {
                panic!(
                    "animation {name:?} not found in {:?}",
                    visuals.characters.animations
                )
            })
    };
    let c = &visuals.characters;
    let (graph, nodes) = AnimationGraph::from_clips([find(&c.idle), find(&c.walk), find(&c.work)]);
    anims.ready = Some(AnimSet {
        graph: graphs.add(graph),
        idle: nodes[0],
        walk: nodes[1],
        work: nodes[2],
    });
}

fn spawn_character(
    commands: &mut Commands,
    assets: &AssetServer,
    visuals: &Visuals,
    who: Who,
    model_def: &crate::visuals::CharacterModel,
    ghost: Option<Color>,
) {
    let scale = visuals.characters.scale;
    commands
        .spawn((
            WorldEntity,
            Character {
                who,
                rig: None,
                current: None,
            },
            Transform::default(),
            Visibility::default(),
        ))
        .with_children(|c| {
            c.spawn((
                model(assets, &model_def.model),
                Transform::from_scale(Vec3::splat(scale)),
                PendingRig {
                    hand_item: model_def.hand_item.clone(),
                    ghost,
                },
            ))
            .observe(|ev: On<WorldInstanceReady>, mut commands: Commands| {
                commands.entity(ev.entity).insert(SceneReady);
            });
        });
    let label = match who {
        Who::Player => WorldLabel::Player,
        Who::Spirit(id) => WorldLabel::Spirit(id),
    };
    spawn_label(commands, label, 12.0);
}

fn spawn_characters(
    mut commands: Commands,
    sim: Res<Sim>,
    visuals: Res<Visuals>,
    assets: Res<AssetServer>,
    mut spawned: ResMut<Spawned>,
    existing: Query<&Character>,
) {
    // The world view resets `spawned` on restart; the player is respawned then too.
    let w = &sim.world;
    if spawned.spirits == 0 && !existing.iter().any(|c| c.who == Who::Player) {
        spawn_character(
            &mut commands,
            &assets,
            &visuals,
            Who::Player,
            &visuals.characters.player,
            None,
        );
    }
    for s in &w.spirits[spawned.spirits..] {
        let role = w.defs.role(s.role);
        let Some(model_def) = visuals.characters.roles.get(&role.id) else {
            continue;
        };
        let [r, g, b] = role.color;
        spawn_character(
            &mut commands,
            &assets,
            &visuals,
            Who::Spirit(s.id),
            model_def,
            Some(Color::srgb(r, g, b)),
        );
    }
    spawned.spirits = w.spirits.len();
}

#[allow(clippy::too_many_arguments)]
fn setup_rigs(
    mut commands: Commands,
    anims: Res<CharacterAnims>,
    visuals: Res<Visuals>,
    assets: Res<AssetServer>,
    pending: Query<(Entity, &PendingRig, &ChildOf), With<SceneReady>>,
    mut characters: Query<&mut Character>,
    children: Query<&Children>,
    names: Query<&Name>,
    mesh_materials: Query<&MeshMaterial3d<StandardMaterial>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(set) = &anims.ready else { return };
    for (model_entity, rig, parent) in &pending {
        commands.entity(model_entity).remove::<PendingRig>();
        let descendants: Vec<Entity> = children.iter_descendants(model_entity).collect();
        let named = |n: &str| {
            descendants
                .iter()
                .copied()
                .find(|e| names.get(*e).is_ok_and(|x| x.as_str() == n))
        };

        if let Some(root) = named(RIG_ROOT) {
            let mut transitions = AnimationTransitions::new();
            let mut player = AnimationPlayer::default();
            transitions
                .play(&mut player, set.idle, Duration::ZERO)
                .repeat();
            commands.entity(root).insert((
                player,
                transitions,
                AnimationGraphHandle(set.graph.clone()),
            ));
            tag_bones(&mut commands, root, root, &mut vec![], &children, &names);
            if let Ok(mut c) = characters.get_mut(parent.parent()) {
                c.rig = Some(root);
                c.current = Some(set.idle);
            }
        }
        if let (Some(item), Some(slot)) = (&rig.hand_item, named(HAND_SLOT)) {
            commands.entity(slot).with_child(model(&assets, item));
        }
        if let Some(color) = rig.ghost {
            let look = &visuals.spirit_look;
            for e in &descendants {
                let Ok(handle) = mesh_materials.get(*e) else {
                    continue;
                };
                let Some(mut m) = materials.get(&handle.0).cloned() else {
                    continue;
                };
                m.base_color = m.base_color.mix(&color, 0.35).with_alpha(look.alpha);
                m.alpha_mode = AlphaMode::Blend;
                m.emissive = LinearRgba::from(color) * look.glow;
                commands.entity(*e).insert(MeshMaterial3d(materials.add(m)));
            }
        }
    }
}

/// Gives every node under the rig root the target id the glTF loader would
/// have assigned (a hash of the names from the root down to that node) and
/// points it at the root's `AnimationPlayer`.
fn tag_bones(
    commands: &mut Commands,
    entity: Entity,
    root: Entity,
    path: &mut Vec<Name>,
    children: &Query<&Children>,
    names: &Query<&Name>,
) {
    let Ok(name) = names.get(entity) else { return };
    path.push(name.clone());
    commands
        .entity(entity)
        .insert((AnimationTargetId::from_names(path.iter()), AnimatedBy(root)));
    if let Ok(kids) = children.get(entity) {
        for &kid in kids {
            tag_bones(commands, kid, root, path, children, names);
        }
    }
    path.pop();
}

/// Where an actor is working, if its current task is a timed action at a tile.
fn work_target(sim: &Sim, actor: &Actor) -> Option<(bool, bevy::math::IVec2)> {
    let w = &sim.world;
    let tile = match actor.queue.front()? {
        Task::Chop { tree } => (true, w.trees[*tree].pos),
        Task::Build { project } => (true, w.projects[*project].site),
        Task::Deposit { building } | Task::Withdraw { building, .. } => {
            (false, w.buildings[*building].pos)
        }
        Task::DeliverToSite { project } => (false, w.projects[*project].site),
        Task::Move {
            goal: Goal::Adjacent(p),
            ..
        } if actor.mover.path.is_empty() => (false, *p),
        Task::Move { .. } => return None,
    };
    Some((tile.0, bevy::math::IVec2::new(tile.1.x, tile.1.y)))
}

fn sync_characters(
    time: Res<Time>,
    sim: Res<Sim>,
    anims: Res<CharacterAnims>,
    mut characters: Query<(&mut Character, &mut Transform)>,
    mut players: Query<(&mut AnimationPlayer, &mut AnimationTransitions)>,
) {
    let Some(set) = &anims.ready else { return };
    let w = &sim.world;
    for (mut c, mut tf) in &mut characters {
        let actor = match c.who {
            Who::Player => &w.player.actor,
            Who::Spirit(id) => match w.spirits.get(id) {
                Some(s) => &s.actor,
                None => continue,
            },
        };
        let (x, y) = actor.mover.visual_pos();
        tf.translation = tile_world(x, y);

        let moving = actor.mover.progress > 0 || !actor.mover.path.is_empty();
        let target = work_target(&sim, actor);
        let look_at = if moving {
            actor
                .mover
                .path
                .front()
                .map(|n| Vec2::new(n.x as f32 - x, n.y as f32 - y))
        } else {
            target.map(|(_, t)| Vec2::new(t.x as f32 - x, t.y as f32 - y))
        };
        if let Some(dir) = look_at.filter(|d| d.length_squared() > 1e-4) {
            // KayKit characters face +z.
            let want = Quat::from_rotation_y(dir.x.atan2(dir.y));
            let t = (time.delta_secs() * 12.0).min(1.0);
            tf.rotation = tf.rotation.slerp(want, t);
        }

        let working = !moving && target.is_some_and(|(timed, _)| timed) && !sim.paused;
        let wanted = if moving && !sim.paused {
            set.walk
        } else if working {
            set.work
        } else {
            set.idle
        };
        if c.current == Some(wanted) {
            continue;
        }
        let Some(rig) = c.rig else { continue };
        if let Ok((mut player, mut transitions)) = players.get_mut(rig) {
            transitions
                .play(&mut player, wanted, Duration::from_millis(200))
                .repeat();
            c.current = Some(wanted);
        }
    }
}
