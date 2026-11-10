//! What Fitted Panels change for someone who keeps training Woodworking.
//! Deterministic: the numbers repeat exactly.
//!
//!     cargo run -p spirit_sim --release --example panel_report
//!
//! Each run plays the normal route, then trains by hand. The reference player
//! never hesitates, so this measures the economy, not how it feels.

use spirit_sim::autoplay::train_on;

const MINUTES: u64 = 120;

fn main() {
    println!("# v0.11 panel report\n");
    println!("{MINUTES} minutes of Woodworking by hand, after the normal route.\n");
    println!(
        "| Habit | XP | XP/min | Panels made | Logs and planks in Main | Main Storage | Storage trips | Full bag | Waiting for materials |"
    );
    println!("|---|---|---|---|---|---|---|---|---|");
    for panels in [false, true] {
        match train_on(panels, MINUTES) {
            Ok(r) => println!(
                "| {} | {} | {} | {} | {} | {} of {} | {} | {}s | {}s |",
                r.name,
                r.xp,
                r.xp / r.minutes,
                r.panels_made,
                r.logs_and_planks_in_main,
                r.main_used,
                r.main_capacity,
                r.storage_trips,
                r.full_bag_seconds,
                r.waiting_seconds
            ),
            Err(e) => println!("FAILED: {e}"),
        }
    }
}
