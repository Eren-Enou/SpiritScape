//! SpiritScape: Bevy front end for the `spirit_sim` simulation.
//!
//! The simulation lives entirely in `spirit_sim::World`. This crate only
//! paces it in real time, draws it, and turns input into commands.
//!
//!     cargo run              # new game
//!     cargo run -- --demo    # development demonstration scenario
//!     cargo run -- --speed 4 # start at 4x

mod capture;
mod render;
mod sim_runner;
mod ui;

use bevy::prelude::*;
use bevy::window::WindowResolution;
use spirit_sim::ScenarioKind;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let scenario = if args.iter().any(|a| a == "--demo") {
        ScenarioKind::Demo
    } else {
        ScenarioKind::NewGame
    };

    let mut sim = sim_runner::Sim::new(scenario);
    if let Some(speed) = args
        .iter()
        .position(|a| a == "--speed")
        .and_then(|i| args.get(i + 1)?.parse().ok())
    {
        sim.speed = speed;
    }

    let mut app = App::new();
    app.insert_resource(ClearColor(Color::srgb(0.08, 0.09, 0.08)))
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "SpiritScape - Spirit Automation PoC".into(),
                resolution: WindowResolution::from((1600u32, 920u32)),
                ..default()
            }),
            ..default()
        }))
        .insert_resource(sim)
        .add_plugins((
            sim_runner::SimRunnerPlugin,
            render::RenderPlugin,
            ui::UiPlugin,
        ))
        .add_systems(Update, capture::capture_system);
    if let Some(plan) = capture::CapturePlan::from_args(&args) {
        app.insert_resource(plan);
    }
    app.run();
}
