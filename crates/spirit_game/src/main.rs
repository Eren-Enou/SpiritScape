//! SpiritScape: Bevy front end for the `spirit_sim` simulation.
//!
//! The simulation lives entirely in `spirit_sim::World` on a 2D tile grid.
//! This crate only paces it in real time, draws it as a 3D world (2.5D view),
//! and turns input into commands.
//!
//!     cargo run                 # title menu: Continue or New Game
//!     cargo run -- --continue   # straight into the most recent save
//!     cargo run -- --new-game   # straight into a new game
//!     cargo run -- --demo       # development demonstration scenario
//!     cargo run -- --speed 4    # start at 4x
//!
//! `--profile ID` picks the progression profile of a new game (`normal`, or
//! `development` for fast rates), `--save-dir DIR` where saves are kept and
//! `--autosave SECONDS` how often the game autosaves (0 for never). `--fps`
//! logs the frame rate every five seconds.

mod camera;
mod capture;
mod cards;
mod characters;
mod feedback;
mod script;
mod sim_runner;
mod status;
mod ui;
mod visuals;
mod world_view;

use bevy::prelude::*;
use bevy::window::WindowResolution;
use spirit_sim::ScenarioKind;
use spirit_sim::save::SaveStore;

fn main() -> AppExit {
    let args: Vec<String> = std::env::args().collect();
    let value = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1).cloned())
    };
    let flag = |name: &str| args.iter().any(|a| a == name);
    let profile = value("--profile").unwrap_or_else(|| "normal".into());
    let scenario = if flag("--demo") {
        Some(ScenarioKind::Demo)
    } else {
        value("--scenario").map(|key| {
            ScenarioKind::from_key(&key).unwrap_or_else(|| {
                let keys: Vec<_> = ScenarioKind::ALL.iter().map(|k| k.key()).collect();
                panic!("unknown scenario {key:?}; expected one of {keys:?}")
            })
        })
    };

    let mut sim = match scenario {
        Some(kind) => sim_runner::Sim::new(kind),
        None if flag("--new-game") => sim_runner::Sim::journey(&profile).expect("known profile"),
        None => sim_runner::Sim::title_screen(&profile).expect("known profile"),
    };
    if let Some(dir) = value("--save-dir") {
        sim.store = SaveStore::new(dir);
    }
    if let Some(seconds) = value("--autosave").and_then(|v| v.parse().ok()) {
        sim.autosave_seconds = seconds;
    }
    if flag("--continue") {
        sim.continue_game();
    }
    if !sim.started {
        sim.open_menu();
    }
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
                    title: "SpiritScape".into(),
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
    .insert_resource(ui::StartTab {
        gathering: args.iter().any(|a| a == "--gathering"),
        production: args.iter().any(|a| a == "--production"),
    })
    .insert_resource(visuals::Visuals::builtin())
    .add_plugins((
        sim_runner::SimRunnerPlugin,
        world_view::WorldViewPlugin,
        characters::CharactersPlugin,
        camera::CameraPlugin,
        ui::UiPlugin,
        script::ScriptPlugin,
        feedback::FeedbackPlugin,
    ))
    .add_systems(Update, capture::capture_system);
    if flag("--fps") {
        app.add_systems(Update, log_frame_rate);
    }
    // A script takes its own screenshots, so `--capture` is just its folder then.
    if let Some(script) = script::Script::from_args(&args) {
        app.insert_resource(script);
    } else if let Some(plan) = capture::CapturePlan::from_args(&args) {
        app.insert_resource(plan);
    }
    app.run()
}

/// Average and slowest frame over the last five seconds.
fn log_frame_rate(time: Res<Time>, mut window: Local<(f32, u32, f32)>) {
    let dt = time.delta_secs();
    window.0 += dt;
    window.1 += 1;
    window.2 = window.2.max(dt);
    if window.0 >= 5.0 {
        let fps = window.1 as f32 / window.0;
        info!("fps {fps:.0}, slowest frame {:.1} ms", window.2 * 1000.0);
        *window = (0.0, 0, 0.0);
    }
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
