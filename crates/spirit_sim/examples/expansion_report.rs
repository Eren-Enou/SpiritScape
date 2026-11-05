//! What the Forest Crossing changes, how haulers choose, and how long the road
//! to it takes under normal rules. Deterministic: the numbers repeat exactly.
//!
//!     cargo run -p spirit_sim --release --example expansion_report
//!
//! Nobody touches the worlds in the first two parts. The third is a reference
//! player with a one-second reaction who never hesitates, so a person will
//! take longer. None of this says whether any of it is fun.

use spirit_sim::autoplay::{
    compare_expansion, compare_hauling, crossing_payback, play_to_expansion,
};

const WARM_UP: u64 = 15;
const MINUTES: u64 = 30;

fn clock(secs: u64) -> String {
    format!("{:02}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
}

fn main() {
    println!("# v0.9 expansion report\n");
    println!("## Steady pace\n");
    println!("{MINUTES} minutes, after a {WARM_UP}-minute warm-up, with an unreachable target.\n");
    println!(
        "| Settlement | Hardwood/min | Beams home/min | Chopping | Waiting for regrowth | Hauler tiles/min | Hauler walking | At bench | Hardwood used |"
    );
    println!("|---|---|---|---|---|---|---|---|---|");
    for r in compare_expansion(WARM_UP, MINUTES) {
        println!(
            "| {} | {:.1} | {:.1} | {}% | {}% | {:.0} | {}% | {}% | {} |",
            r.name,
            r.hardwood_per_min,
            r.beams_per_min,
            r.gathering_pct,
            r.waiting_pct,
            r.hauler_tiles_per_min,
            r.hauler_walking_pct,
            r.bench_pct,
            r.hardwood_used
        );
    }

    println!("\n## Payback\n");
    let p = crossing_payback(90);
    let at = |t: Option<u64>| t.map_or("not within the run".to_string(), clock);
    println!("Crossing built at {}.", at(p.built_at));
    println!("Caught up with not building it at {}.", at(p.caught_up_at));
    println!("After 90 minutes: {} beams home without, {} with.", p.without, p.with);

    println!("\n## A shared hauler\n");
    println!("| Hauler | Tiles | Trips | Units | Walking | First beam home | Target met |");
    println!("|---|---|---|---|---|---|---|");
    for r in compare_hauling(20) {
        println!(
            "| {} | {} | {} | {} | {}% | {} | {} |",
            r.name,
            r.tiles,
            r.trips,
            r.units,
            r.walking_pct,
            at(r.first_beam_home),
            at(r.reached_target)
        );
    }

    println!("\n## The road under normal rules\n");
    match play_to_expansion("normal") {
        Ok((w, r)) => {
            w.check_conservation().expect("conservation");
            println!("| Step | Time |\n|---|---|");
            for (name, t) in &r.steps {
                println!("| {name} | {} |", clock(*t));
            }
            println!(
                "\nIn the {} minutes after the crossing: {} Hardwood cut, {} beams home.\n",
                r.observed_minutes, r.hardwood_after, r.beams_after
            );
            let spent: Vec<String> = r.consumed.iter().map(|(n, c)| format!("{c} {n}")).collect();
            println!("Construction used {}.\n", spent.join(", "));
            println!("```text\n{}```", r.report);
            if std::env::args().any(|a| a == "--state") {
                println!("\n```text\n{}\n```", r.final_state);
            }
        }
        Err(e) => println!("FAILED: {e}"),
    }
}
