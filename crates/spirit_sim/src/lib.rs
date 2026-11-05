//! SpiritScape simulation core.
//!
//! Pure Rust with no engine dependency: deterministic, fixed-step, and fully
//! testable headless. The Bevy front end (`spirit_game`) only reads this
//! state and submits [`world::Command`]s.

pub mod advice;
pub mod ambitions;
pub mod autoplay;
pub mod behavior;
pub mod defs;
pub mod entities;
pub mod events;
pub mod grid;
pub mod inventory;
pub mod journey;
pub mod metrics;
pub mod outpost;
pub mod planner;
pub mod production;
pub mod projects;
pub mod save;
pub mod scenario;
pub mod session;
pub mod skills;
pub mod soul;
pub mod tasks;
pub mod training;
pub mod world;

pub use defs::GameDefs;
pub use scenario::ScenarioKind;
pub use world::{Command, World};
