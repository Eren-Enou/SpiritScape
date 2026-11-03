//! Runs a scenario without graphics, printing the event log and a gathering
//! report every minute.
//!
//!     cargo run -p spirit_sim --example headless_demo -- [seconds] [scenario]
//!
//! Scenarios: demo (default), new_game, regrind, grove_sustainable,
//! grove_oversubscribed. Set QUIET=1 to print only the reports.

use spirit_sim::metrics::GatheringReport;
use spirit_sim::{ScenarioKind, World};

fn main() {
    let mut args = std::env::args().skip(1);
    let seconds: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(600);
    let key = args.next().unwrap_or_else(|| "demo".into());
    let kind = ScenarioKind::from_key(&key).unwrap_or_else(|| panic!("unknown scenario {key:?}"));
    let quiet = std::env::var("QUIET").is_ok();
    let mut world = World::from_scenario(kind);
    let tps = world.defs.ticks_per_second as u64;
    let logs = world.defs.resource_id("logs").unwrap();
    let mut printed = 0;
    for _ in 0..seconds * tps {
        world.step();
        world
            .check_conservation()
            .expect("resource conservation violated");
        if !quiet {
            for e in world.events.iter().skip(printed) {
                println!(
                    "[{:>6.1}s] {:?}: {}",
                    e.tick as f64 / tps as f64,
                    e.kind,
                    e.text
                );
            }
        }
        printed = world.events.iter().count();
        if world.tick.is_multiple_of(tps * 60) {
            print_report(&world, &world.gathering_report());
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

fn print_report(world: &World, r: &GatheringReport) {
    let mins = world.tick / (world.defs.ticks_per_second as u64 * 60);
    println!("=== {mins} min: bottleneck {} ===", r.bottleneck.label());
    for s in &r.sites {
        println!(
            "  {:<15} trees {:>2}/{:<2} logs {:>3}/{:<3} measured {:>5.1}/min sustainable {:>5.1}/min workers {}",
            s.name,
            s.trees_grown,
            s.trees_total,
            s.logs_available,
            s.logs_max,
            s.measured_per_min,
            s.sustainable_per_min,
            s.workers
        );
    }
    let t = &r.gatherers;
    println!(
        "  gatherers {}: chop {}% travel {}% handle {}% wait {}% storage {}% | idle {}% of all | chop capacity {:.1}/min",
        r.gatherer_count,
        t.pct_of_working(t.gathering),
        t.pct_of_working(t.traveling),
        t.pct_of_working(t.handling),
        t.pct_of_working(t.waiting_recovery),
        t.pct_of_working(t.blocked_storage),
        (t.idle * 100).checked_div(t.total()).unwrap_or(0),
        r.chop_capacity_per_min
    );
    println!("  {}", r.summary);
}
