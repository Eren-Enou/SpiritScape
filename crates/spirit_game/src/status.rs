//! One-word spirit status with a colour, shared by the map markers and the
//! side panel so both always say the same thing.

use bevy::prelude::*;
use spirit_sim::entities::{Activity, Spirit};
use spirit_sim::metrics::WaitKind;
use spirit_sim::tasks::Task;
use spirit_sim::world::World;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Chopping,
    Crafting,
    Walking,
    Carrying,
    Handling,
    Building,
    Waiting,
    Idle,
    Blocked,
}

impl Status {
    pub fn color(self) -> Color {
        match self {
            Status::Chopping => Color::srgb(0.45, 0.85, 0.45),
            Status::Crafting => Color::srgb(0.95, 0.55, 0.75),
            Status::Walking => Color::srgb(0.55, 0.75, 1.0),
            Status::Carrying | Status::Handling => Color::srgb(0.45, 0.85, 0.85),
            Status::Building => Color::srgb(1.0, 0.85, 0.35),
            Status::Waiting => Color::srgb(1.0, 0.6, 0.2),
            Status::Idle => Color::srgb(0.6, 0.62, 0.66),
            Status::Blocked => Color::srgb(1.0, 0.35, 0.3),
        }
    }
}

/// What a spirit is doing right now, as a status and a few words.
pub fn spirit_status(s: &Spirit) -> (Status, String) {
    let carried = s.actor.inventory.total();
    match s.actor.queue.front() {
        Some(Task::Chop { .. }) => (Status::Chopping, "gathering".into()),
        Some(Task::Craft { .. }) => (Status::Crafting, "at the bench".into()),
        Some(Task::Move { .. }) if carried > 0 => (Status::Carrying, format!("carrying {carried}")),
        Some(Task::Move { .. }) => (Status::Walking, "walking".into()),
        Some(Task::Build { .. }) => (Status::Building, "building".into()),
        Some(_) => (Status::Handling, "loading".into()),
        None => match (&s.activity, s.wait) {
            (Activity::Blocked(_), Some(WaitKind::Recovery)) => {
                (Status::Waiting, "waiting for regrowth".into())
            }
            (Activity::Blocked(_), Some(WaitKind::Inputs)) => {
                (Status::Waiting, "waiting for materials".into())
            }
            (Activity::Blocked(_), Some(WaitKind::Station)) => {
                (Status::Waiting, "waiting for a bench".into())
            }
            (Activity::Blocked(_), Some(WaitKind::Storage)) => {
                (Status::Blocked, "storage full".into())
            }
            (Activity::Blocked(_), _) => (Status::Blocked, "stuck".into()),
            // Not idle: it's where they've chosen to be.
            (Activity::Idle(what), _) if what == World::AT_THE_HEARTH => {
                (Status::Idle, "at the Hearth".into())
            }
            _ => (Status::Idle, "idle".into()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spirit_sim::scenario::ScenarioKind;

    #[test]
    fn a_spirit_at_the_hearth_says_so() {
        let mut w = World::from_scenario(ScenarioKind::Midwinter);
        w.run_ticks(1200);
        assert!(w.hearth_gathering());
        let said: Vec<String> = w.spirits.iter().map(|s| spirit_status(s).1).collect();
        assert!(said.iter().any(|t| t == "at the Hearth"), "{said:?}");
        assert!(!said.iter().any(|t| t == "idle"), "{said:?}");
    }
}
