//! One long game that tries to meet every long-term goal. Deterministic.
//!
//!     cargo run -p spirit_sim --release --example all_goals_report
//!
//! The player never hesitates, so a person will take far longer. This shows the
//! goals can be reached, not how long they should take.

use spirit_sim::autoplay::play_all_goals;

fn clock(secs: u64) -> String {
    format!("{:02}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
}

fn main() {
    println!("# All goals report\n");
    match play_all_goals(60) {
        Ok((w, r)) => {
            w.check_conservation().expect("conservation");
            println!("| Goal | Met |\n|---|---|");
            for (title, at) in &r.met {
                println!("| {title} | {} |", clock(*at));
            }
            println!("\nMissed: {:?}", r.missed);
            println!("Ended at {}.\n", clock(r.seconds));
            let main = &w.main().storage;
            println!("Main Storage: {} of {}.", main.total(), main.capacity);
            let left: u64 = w.ledger.left_behind.values().sum();
            println!("Left behind: {left} units.");
            if std::env::args().any(|a| a == "--state") {
                println!("\n```text\n{}\n```", r.final_state);
            }
        }
        Err(e) => println!("FAILED: {e}"),
    }
}
