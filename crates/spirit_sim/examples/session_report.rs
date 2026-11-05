//! The structured playtest path under normal rules, as a pacing report.
//! Deterministic: the numbers repeat exactly.
//!
//!     cargo run -p spirit_sim --release --example session_report
//!
//! The player here has a one-second reaction delay, never hesitates and always
//! knows what to do. These numbers show where the game's own limits are. They
//! say nothing about how it feels, and a person will take longer.

use spirit_sim::autoplay::{FullSession, Stretch, play_full_session, storage_wall};
use spirit_sim::metrics::TimeUse;
use spirit_sim::training::TrainingMode;

const MINUTES: u64 = 45;

fn clock(secs: u64) -> String {
    format!("{:02}:{:02}", secs / 60, secs % 60)
}

fn shares(t: &TimeUse) -> String {
    let total = t.total().max(1);
    let pct = |n: u64| n * 100 / total;
    format!(
        "chop {}%, saw {}%, walk {}%, wait {}%, stuck {}%, idle {}%",
        pct(t.gathering),
        pct(t.crafting),
        pct(t.traveling),
        pct(t.waiting_recovery + t.waiting_input),
        pct(t.blocked_storage),
        pct(t.idle)
    )
}

fn stretch(name: &str, s: &Stretch, xp: u64) {
    println!(
        "| {name} | {} | {} | {} | {} | {:+} / {:+} | {} |",
        clock(s.seconds),
        xp * 60 / s.seconds.max(1),
        s.actions,
        s.storage_trips,
        s.logs_gained,
        s.planks_gained,
        shares(&s.player)
    );
}

fn session(s: &FullSession) {
    let r = &s.first_cycle;
    println!(
        "\n## Automatic targeting {}\n",
        if s.auto_gather { "on" } else { "off" }
    );
    println!("| Milestone | Time |\n|---|---|");
    for (m, t) in &r.milestones {
        println!("| {} | {} |", m.label(), clock(*t));
    }
    println!("\nRegaining the sacrifice, from the same moment:\n");
    println!("| Stretch | Time | XP/min | Clicks | Storage trips | Logs / planks | Your time |");
    println!("|---|---|---|---|---|---|---|");
    stretch(
        "Woodcutting alone",
        &r.retrain_alone,
        r.woodcutting_sacrificed,
    );
    stretch(
        "Woodcutting supported",
        &r.retrain_supported,
        r.woodcutting_sacrificed,
    );
    for (mode, st) in &s.modes {
        let name = format!("Woodworking, {}", TrainingMode::label(*mode));
        stretch(&name, st, r.woodworking_sacrificed);
    }
    let name = format!(
        "Woodworking, Optimized after {} of building up",
        clock(s.invest_seconds)
    );
    stretch(&name, &s.invested, r.woodworking_sacrificed);
    println!(
        "\nAfter {}: Woodcutting {} of the {} XP Ironwood needs. Save and resume exact: {}.\n",
        clock(s.seconds),
        s.advanced_xp.0,
        s.advanced_xp.1,
        s.resumed_exactly
    );
    println!("```text\n{}```", s.report);
}

fn main() {
    println!("# v0.7 pacing report (normal profile, reference player)");
    for auto in [true, false] {
        match play_full_session("normal", auto, MINUTES) {
            Ok((w, s)) => {
                w.check_conservation().expect("conservation");
                session(&s);
            }
            Err(e) => println!("\nFAILED: {e}"),
        }
    }
    println!("\n## Chopping and spending nothing\n");
    match storage_wall("normal", 180) {
        Ok(w) => {
            let at = |t: Option<u64>| t.map_or("never".to_string(), clock);
            println!(
                "Warned at {}. Stuck with a full bag at {}, with {} of the {} XP Ironwood needs.",
                at(w.warned_at),
                at(w.stuck_at),
                w.xp_at_stuck,
                w.goal_xp
            );
        }
        Err(e) => println!("FAILED: {e}"),
    }
}
