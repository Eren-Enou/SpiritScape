//! What Quarrying and the Stone Causeway take and give. Deterministic.
//!
//!     cargo run -p spirit_sim --release --example quarry_report
//!
//! The scenario's rates are for development, so times here aren't a pace.

use spirit_sim::autoplay::{compare_nursery, stone_time, walk_across};

fn clock(t: Option<u64>) -> String {
    t.map_or("not within the run".into(), |s| {
        format!("{:02}:{:02}", s / 60, s % 60)
    })
}

fn main() {
    println!("# v0.15 quarry report\n");
    println!("| Causeway finished | Time |\n|---|---|");
    println!(
        "| With the player cutting too | {} |",
        clock(stone_time(false))
    );
    println!("| Left to the spirit alone | {} |", clock(stone_time(true)));
    println!("\n| Player's walk, Main Storage to the Distant Forest | Seconds |");
    println!("|---|---|");
    println!("| By the bridge | {:.1} |", walk_across(false));
    println!("| By the causeway | {:.1} |", walk_across(true));
    println!("\n| Settlement | Hardwood/min | Beams home/min | Hauler walking |");
    println!("|---|---|---|---|");
    for r in compare_nursery(15, 30) {
        println!(
            "| {} | {:.1} | {:.1} | {}% |",
            r.name, r.hardwood_per_min, r.beams_per_min, r.hauler_walking_pct
        );
    }
}
