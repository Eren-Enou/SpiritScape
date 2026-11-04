//! Runs a scenario without graphics, printing the event log and a gathering
//! report every minute.
//!
//!     cargo run -p spirit_sim --example headless_demo -- [seconds] [scenario]
//!
//! Scenarios: demo (default), new_game, regrind, grove_sustainable,
//! grove_oversubscribed, workshop_manual, workshop_automated,
//! workshop_construction. Set QUIET=1 to print only the reports.

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
            print_production(&world);
        }
    }
    println!("\n--- after {seconds}s ---");
    for b in &world.buildings {
        println!(
            "{} at {}: {} (capacity {})",
            b.name,
            b.pos,
            world.defs.describe_amounts(b.storage.contents()),
            b.storage.capacity
        );
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

fn print_production(world: &World) {
    let r = world.production_report();
    let t = &r.processing;
    let h = &r.hauling;
    println!(
        "  production: {} | Main {} logs {}/{} planks | station {} logs {} planks | {:.1} planks/min | bench {}% wait {}% | haulers walk {}% wait {}%",
        r.status.label(),
        r.input_in_main,
        r.output_in_main,
        r.goal,
        r.input_at_station,
        r.output_at_station,
        r.output_per_min,
        t.pct_of_working(t.crafting),
        t.pct_of_working(t.waiting_input),
        h.pct_of_working(h.traveling),
        h.pct_of_working(h.waiting_input),
    );
    println!("  {}: {}", r.bottleneck.label(), r.summary);
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
