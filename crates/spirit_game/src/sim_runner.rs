//! Owns the simulation and advances it at a fixed tick rate scaled by the
//! chosen speed. Rendering never changes simulation state.

use bevy::prelude::*;
use spirit_sim::defs::SkillId;
use spirit_sim::{Command, ScenarioKind, World};

pub const SPEEDS: [u32; 4] = [1, 2, 4, 8];
/// Upper bound on ticks per frame so a slow frame can't snowball.
const MAX_TICKS_PER_FRAME: u32 = 200;

#[derive(Resource)]
pub struct Sim {
    pub world: World,
    pub scenario: ScenarioKind,
    pub paused: bool,
    pub speed: u32,
    accumulator: f32,
    /// XP amount selected in the Soul Separation panel.
    pub soul_amount: u64,
    /// Feedback from the last player command.
    pub message: String,
    /// Bumped on restart so the renderer rebuilds its entities.
    pub generation: u32,
    /// Large on-screen notice and its remaining seconds.
    pub banner: Option<(String, f32)>,
}

impl Sim {
    pub fn new(scenario: ScenarioKind) -> Self {
        let world = World::from_scenario(scenario);
        Self {
            soul_amount: world.defs.soul_min_xp,
            world,
            scenario,
            paused: false,
            speed: match scenario {
                ScenarioKind::Demo
                | ScenarioKind::GroveSustainable
                | ScenarioKind::GroveOversubscribed => 4,
                _ => 1,
            },
            accumulator: 0.0,
            message: String::new(),
            generation: 0,
            banner: None,
        }
    }

    pub fn restart(&mut self, scenario: ScenarioKind) {
        let generation = self.generation + 1;
        *self = Sim::new(scenario);
        self.generation = generation;
        self.message = "Simulation restarted".into();
    }

    /// Performs a Soul Separation and, on success, shows what was given up.
    pub fn separate_soul(&mut self, skill: SkillId, xp: u64) {
        let preview = self.world.soul_preview(skill, xp);
        let spirits = self.world.spirits.len();
        self.command(Command::SoulSeparate { skill, xp });
        if self.world.spirits.len() == spirits {
            return;
        }
        let w = &self.world;
        let s = &w.spirits[spirits];
        let skill_name = &w.defs.skill(skill).name;
        let mut text = format!(
            "SOUL SEPARATION
{skill_name} level {} -> {}   (-{xp} XP)
{} awakens: permanent {skill_name} level {}",
            preview.current_level,
            preview.after_level,
            s.name,
            s.level()
        );
        if !preview.lost.is_empty() {
            let lost: Vec<String> = preview
                .lost
                .iter()
                .map(|k| {
                    let d = w.defs.tree(*k);
                    format!("{} (level {})", d.name, d.level_required)
                })
                .collect();
            text += &format!(
                "
You can no longer cut {} - retrain to regain it",
                lost.join(", ")
            );
        }
        self.banner = Some((text, 6.0));
    }

    pub fn command(&mut self, cmd: Command) {
        self.message = match self.world.apply(cmd) {
            Ok(msg) => msg,
            Err(err) => format!("Can't do that: {err}"),
        };
    }
}

pub struct SimRunnerPlugin;

impl Plugin for SimRunnerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (keyboard_controls, advance_simulation).chain());
    }
}

fn advance_simulation(time: Res<Time>, mut sim: ResMut<Sim>) {
    if sim.paused {
        return;
    }
    let tps = sim.world.defs.ticks_per_second as f32;
    sim.accumulator += time.delta_secs() * tps * sim.speed as f32;
    let ticks = (sim.accumulator.floor() as u32).min(MAX_TICKS_PER_FRAME);
    sim.accumulator = (sim.accumulator - ticks as f32).min(MAX_TICKS_PER_FRAME as f32);
    for _ in 0..ticks {
        sim.world.step();
    }
}

fn keyboard_controls(keys: Res<ButtonInput<KeyCode>>, mut sim: ResMut<Sim>) {
    if keys.just_pressed(KeyCode::Space) {
        sim.paused = !sim.paused;
    }
    for (key, speed) in [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
    ]
    .into_iter()
    .zip(SPEEDS)
    {
        if keys.just_pressed(key) {
            sim.speed = speed;
        }
    }
    if keys.just_pressed(KeyCode::F5) {
        let s = sim.scenario;
        sim.restart(s);
    }
}
