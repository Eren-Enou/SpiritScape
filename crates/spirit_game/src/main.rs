//! SpiritScape: Bevy front end for the `spirit_sim` simulation.
//!
//! The simulation lives entirely in `spirit_sim::World` on a 2D tile grid.
//! This crate only paces it in real time, draws it as a 3D world (2.5D view),
//! and turns input into commands.
//!
//!     cargo run              # new game
//!     cargo run -- --demo    # development demonstration scenario
//!     cargo run -- --speed 4 # start at 4x

mod camera;
mod capture;
mod characters;
mod sim_runner;
mod ui;
mod visuals;
mod world_view;

use bevy::prelude::*;
use bevy::window::WindowResolution;
use spirit_sim::ScenarioKind;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let value = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1).cloned())
    };
    let scenario = if args.iter().any(|a| a == "--demo") {
        ScenarioKind::Demo
    } else if let Some(key) = value("--scenario") {
        ScenarioKind::from_key(&key).unwrap_or_else(|| {
            let keys: Vec<_> = ScenarioKind::ALL.iter().map(|k| k.key()).collect();
            panic!("unknown scenario {key:?}; expected one of {keys:?}")
        })
    } else {
        ScenarioKind::NewGame
    };

    let mut sim = sim_runner::Sim::new(scenario);
    // Development aid: perform a Soul Separation of this many XP at start.
    if let Some(xp) = value("--separate").and_then(|v| v.parse().ok()) {
        let skill = sim.world.defs.skill_id("woodcutting").unwrap();
        sim.separate_soul(skill, xp);
    }
    if let Some(speed) = args
        .iter()
        .position(|a| a == "--speed")
        .and_then(|i| args.get(i + 1)?.parse().ok())
    {
        sim.speed = speed;
    }

    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "SpiritScape - Spirit Automation PoC".into(),
                    resolution: WindowResolution::from((1600u32, 920u32)),
                    ..default()
                }),
                ..default()
            })
            .set(AssetPlugin {
                file_path: asset_dir(),
                ..default()
            }),
    )
    .insert_resource(sim)
    .insert_resource(ui::StartOnGatheringTab(
        args.iter().any(|a| a == "--gathering"),
    ))
    .insert_resource(visuals::Visuals::builtin())
    .add_plugins((
        sim_runner::SimRunnerPlugin,
        world_view::WorldViewPlugin,
        characters::CharactersPlugin,
        camera::CameraPlugin,
        ui::UiPlugin,
    ))
    .add_systems(Update, capture::capture_system);
    if let Some(plan) = capture::CapturePlan::from_args(&args) {
        app.insert_resource(plan);
    }
    app.run();
}

/// `assets/` next to the working directory (or the executable), falling back
/// to the workspace copy so `cargo run -p spirit_game` works from anywhere.
fn asset_dir() -> String {
    let candidates = [
        std::env::current_dir().ok().map(|d| d.join("assets")),
        std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(|d| d.join("assets"))),
        Some(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets")),
    ];
    candidates
        .into_iter()
        .flatten()
        .find(|d| d.join("models").is_dir())
        .map(|d| d.to_string_lossy().into_owned())
        .unwrap_or_else(|| "assets".into())
}
