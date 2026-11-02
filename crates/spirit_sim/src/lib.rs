//! SpiritScape simulation core.
//!
//! Pure Rust with no engine dependency: deterministic, fixed-step, and fully
//! testable headless. The Bevy front end (`spirit_game`) only reads this
//! state and submits [`world::Command`]s.

pub mod behavior;
pub mod defs;
pub mod entities;
pub mod events;
pub mod grid;
pub mod inventory;
pub mod planner;
pub mod scenario;
pub mod skills;
pub mod tasks;
pub mod world;

pub use defs::GameDefs;
pub use scenario::ScenarioKind;
pub use world::{Command, World};
