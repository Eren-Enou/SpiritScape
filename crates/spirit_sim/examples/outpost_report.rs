//! Central against remote production, and what setting up an outpost takes.
//! Deterministic: the numbers repeat exactly.
//!
//!     cargo run -p spirit_sim --release --example outpost_report
//!
//! Nobody touches the world during a run, so these measure the logistics, not
//! how it feels to play.

use spirit_sim::autoplay::{OutpostRun, compare_layouts, establish_outpost};
use spirit_sim::metrics::TimeUse;

const MINUTES: u64 = 20;

fn share(t: &TimeUse, part: u64) -> u64 {
    part * 100 / t.total().max(1)
}

fn row(r: &OutpostRun) {
    let first = r.first_beam_home.map_or("never".to_string(), |s| {
        format!("{:02}:{:02}", s / 60, s % 60)
    });
    let done = r.reached_target.map_or("not reached".to_string(), |s| {
        format!("{:02}:{:02}", s / 60, s % 60)
    });
    println!(
        "| {} | {} | {first} | {done} | {} | {} | {} | {} | {} | {}% | {}% | {}% |",
        r.name,
        r.beams_home,
        r.beams_made,
        r.hardwood_used,
        r.hauler_tiles,
        r.hauler_trips,
        r.hauler_units,
        share(&r.hauler, r.hauler.traveling),
        share(&r.workers, r.workers.crafting),
        share(&r.gatherers, r.gatherers.gathering),
    );
}

fn main() {
    println!("# v0.8 outpost report\n");
    println!("Each run lasts {MINUTES} minutes from the same spirits and target.\n");
    println!(
        "| Layout | Beams home | First home | Target met | Beams made | Hardwood used | Hauler tiles | Hauler trips | Units carried | Hauler walking | Worker at bench | Gatherers chopping |"
    );
    println!("|---|---|---|---|---|---|---|---|---|---|---|---|");
    for r in compare_layouts(MINUTES) {
        row(&r);
    }
    println!("\n## Establishing an outpost from nothing\n");
    match establish_outpost(10) {
        Ok((w, e)) => {
            w.check_conservation().expect("conservation");
            let clock = |s: u64| format!("{:02}:{:02}", s / 60, s % 60);
            println!("- Outpost Storage finished at {}", clock(e.store_built));
            println!("- Workshop finished at {}", clock(e.workshop_built));
            println!(
                "- Beams made so far, starting from none: {}",
                e.beams_for_building
            );
            let spent: Vec<String> = e.consumed.iter().map(|(n, c)| format!("{c} {n}")).collect();
            println!("- Construction used {}", spent.join(", "));
            println!("- Spirits busy while building: {}%", e.busy_building_pct);
            println!(
                "- Spirits busy over {} minutes of running: {}%",
                e.seconds_running / 60,
                e.busy_running_pct
            );
            println!("- Beams home while running: {}", e.beams_home_running);
            println!("- Commands given: {}", e.commands);
        }
        Err(e) => println!("FAILED: {e}"),
    }
}
