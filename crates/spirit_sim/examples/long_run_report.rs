//! Whether a game slows down or its save grows as hours go by. Timings depend
//! on the machine, so compare them with each other, not with other reports.
//!
//!     cargo run -p spirit_sim --release --example long_run_report

use spirit_sim::autoplay::play_guided;
use spirit_sim::save::{decode, encode};
use std::time::Instant;

const HOURS: u64 = 24;

fn main() {
    let (mut w, _) = play_guided("normal").expect("the route finishes");
    let tps = w.defs.ticks_per_second as u64;
    let target = w.settlement.order.target;
    let _ = target;
    println!("# Long run report\n");
    println!("Left running after the guided route, at about 1,000x real time.\n");
    println!("| Game time | Ticks/s | Save size | Save | Load | Events | Projects |");
    println!("|---|---|---|---|---|---|---|");
    for hour in 0..=HOURS {
        if hour % 6 == 0 {
            let start = Instant::now();
            let n = 3000;
            let mut probe = w.clone();
            probe.run_ticks(n);
            let rate = n as f64 / start.elapsed().as_secs_f64();
            let start = Instant::now();
            let text = encode(&w, "new_game");
            let saved = start.elapsed();
            let start = Instant::now();
            decode(&text).expect("it loads");
            let loaded = start.elapsed();
            println!(
                "| {:02}:{:02} | {:.0} | {} KB | {} ms | {} ms | {} | {} |",
                w.tick / tps / 3600,
                w.tick / tps / 60 % 60,
                rate,
                text.len() / 1024,
                saved.as_millis(),
                loaded.as_millis(),
                w.events.iter().count(),
                w.projects.len()
            );
        }
        w.run_ticks(3600 * tps);
    }
    w.check_conservation().expect("conservation");
}
