//! Whether the game's own messages are enough to get through storage trouble.
//! Deterministic: the numbers repeat exactly.
//!
//!     cargo run -p spirit_sim --release --example guided_report
//!
//! Both players follow the same route to the crossing. The reference player
//! has hand-written habits, and the guided player has only the storage message.
//! Neither is a person: this shows what the message offers is enough, not that
//! anyone would read it.

use spirit_sim::autoplay::{play_guided, play_to_expansion};

fn clock(secs: u64) -> String {
    format!("{:02}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
}

fn main() {
    println!("# Guided player report\n");
    for (name, run) in [
        ("Reference player", play_to_expansion("normal")),
        ("Guided player", play_guided("normal")),
    ] {
        println!("## {name}\n");
        match run {
            Ok((w, r)) => {
                w.check_conservation().expect("conservation");
                println!("| Step | Time |\n|---|---|");
                for (step, t) in r.steps.iter().rev().take(9).rev() {
                    println!("| {step} | {} |", clock(*t));
                }
                let main = &w.main().storage;
                println!(
                    "\nThings done because the message offered them: {}.",
                    r.advice_taken
                );
                println!(
                    "Main Storage ended at {} of {}.",
                    main.total(),
                    main.capacity
                );
                println!("\n```text\n{}```\n", r.report);
            }
            Err(e) => println!("FAILED: {e}\n"),
        }
    }
}
