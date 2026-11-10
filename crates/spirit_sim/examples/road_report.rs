//! What plank roads change. Deterministic: the numbers repeat exactly.
//!
//!     cargo run -p spirit_sim --release --example road_report
//!
//! Nobody touches these worlds, and every road is laid before they start, so
//! this measures what roads are worth, not what it's like to pay for them.

use spirit_sim::autoplay::{compare_roads, walk_seconds};
use spirit_sim::grid::TilePos;

const WARM_UP: u64 = 15;
const MINUTES: u64 = 30;

fn main() {
    println!("# v0.10 road report\n");
    println!("## Walking\n");
    println!("| Trip for the player | Without | With roads |");
    println!("|---|---|---|");
    let home = TilePos::new(6, 15);
    for (name, to) in [
        ("Main Storage to the Starting Grove", TilePos::new(15, 13)),
        ("Main Storage to the bridge", TilePos::new(20, 3)),
        ("Main Storage to Old Growth", TilePos::new(35, 4)),
        ("Main Storage to the Forest Crossing", TilePos::new(47, 5)),
        ("Main Storage to the Distant Forest", TilePos::new(36, 13)),
    ] {
        println!(
            "| {name} | {:.1}s | {:.1}s |",
            walk_seconds(false, home, to),
            walk_seconds(true, home, to)
        );
    }

    println!("\n## Steady pace\n");
    println!("{MINUTES} minutes, after a {WARM_UP}-minute warm-up, with an unreachable target.\n");
    println!(
        "| Settlement | Hardwood/min | Beams home/min | Waiting for regrowth | Hauler walking | At bench | Hardwood used |"
    );
    println!("|---|---|---|---|---|---|---|");
    for r in compare_roads(WARM_UP, MINUTES) {
        println!(
            "| {} | {:.1} | {:.1} | {}% | {}% | {}% | {} |",
            r.name,
            r.hardwood_per_min,
            r.beams_per_min,
            r.waiting_pct,
            r.hauler_walking_pct,
            r.bench_pct,
            r.hardwood_used
        );
    }
}
