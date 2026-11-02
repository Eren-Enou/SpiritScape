//! Development aid: save window screenshots at chosen simulation times, then exit.
//!
//!     spirit_game --demo --speed 8 --capture target/shots --at 20,160,260

use crate::sim_runner::Sim;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};

#[derive(Resource)]
pub struct CapturePlan {
    dir: String,
    /// Simulation seconds at which to capture, ascending.
    at: Vec<u64>,
    next: usize,
    exit_in: Option<f32>,
}

impl CapturePlan {
    pub fn from_args(args: &[String]) -> Option<Self> {
        let value = |flag: &str| {
            args.iter()
                .position(|a| a == flag)
                .and_then(|i| args.get(i + 1))
                .cloned()
        };
        let dir = value("--capture")?;
        let mut at: Vec<u64> = value("--at")
            .unwrap_or_else(|| "5".into())
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect();
        at.sort_unstable();
        let _ = std::fs::create_dir_all(&dir);
        Some(Self {
            dir,
            at,
            next: 0,
            exit_in: None,
        })
    }
}

pub fn capture_system(
    mut commands: Commands,
    time: Res<Time>,
    sim: Res<Sim>,
    plan: Option<ResMut<CapturePlan>>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(mut plan) = plan else { return };
    if let Some(t) = plan.exit_in.as_mut() {
        *t -= time.delta_secs();
        if *t <= 0.0 {
            exit.write(AppExit::Success);
        }
        return;
    }
    let secs = sim.world.tick / sim.world.defs.ticks_per_second as u64;
    if let Some(&at) = plan.at.get(plan.next)
        && secs >= at
    {
        let path = format!("{}/shot_{at:04}s.png", plan.dir);
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path));
        plan.next += 1;
        if plan.next == plan.at.len() {
            plan.exit_in = Some(1.5);
        }
    }
}
