//! Runs the demonstration scenario without graphics and prints the event log.
//!
//!     cargo run -p spirit_sim --example headless_demo -- [seconds]

use spirit_sim::{ScenarioKind, World};

fn main() {
    let seconds: u64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(600);
    let mut world = World::from_scenario(ScenarioKind::Demo);
    let tps = world.defs.ticks_per_second as u64;
    let logs = world.defs.resource_id("logs").unwrap();
    let mut printed = 0;
    for _ in 0..seconds * tps {
        world.step();
        world
            .check_conservation()
            .expect("resource conservation violated");
        for e in world.events.iter().skip(printed) {
            println!(
                "[{:>6.1}s] {:?}: {}",
                e.tick as f64 / tps as f64,
                e.kind,
                e.text
            );
        }
        printed = world.events.iter().count();
        if std::env::var("TRACE").is_ok() && world.tick.is_multiple_of(tps * 10) {
            println!(
                "    planner: {} | samples {:?}",
                world.settlement.planner.status,
                world
                    .settlement
                    .planner
                    .samples
                    .iter()
                    .map(|s| (s.leg_tiles, s.work_ticks, s.amount))
                    .collect::<Vec<_>>()
            );
        }
    }
    println!("\n--- after {seconds}s ---");
    for b in &world.buildings {
        println!("{} at {}: {} logs", b.name, b.pos, b.storage.get(logs));
    }
    for s in &world.spirits {
        println!(
            "{} ({}): {} - {}",
            s.name,
            world.spirit_level_name(s),
            s.activity.label(),
            s.activity.text()
        );
    }
    println!("Planner: {}", world.settlement.planner.status);
}
