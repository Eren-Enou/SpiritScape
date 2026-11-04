//! Measures the first progression cycle under every profile. Deterministic:
//! the numbers repeat exactly.
//!
//!     cargo run -p spirit_sim --release --example pacing
//!
//! The times come from a reference player with a one-second reaction delay
//! who never hesitates, so a person will take longer.

use spirit_sim::GameDefs;
use spirit_sim::autoplay::{PacingReport, Stretch, play_first_cycle};
use spirit_sim::metrics::TimeUse;

fn clock(secs: u64) -> String {
    format!("{:02}:{:02}", secs / 60, secs % 60)
}

fn shares(t: &TimeUse) -> String {
    let total = t.total().max(1);
    let pct = |n: u64| n * 100 / total;
    format!(
        "chop {}%  saw {}%  walk {}%  handling {}%  idle {}%",
        pct(t.gathering),
        pct(t.crafting),
        pct(t.traveling),
        pct(t.handling),
        pct(t.idle)
    )
}

fn spirit_shares(t: &TimeUse) -> String {
    let total = t.total().max(1);
    let pct = |n: u64| n * 100 / total;
    format!(
        "busy {}%  waiting {}%  idle {}%",
        pct(t.gathering + t.crafting + t.traveling + t.handling + t.building),
        pct(t.waiting_recovery + t.waiting_input + t.blocked_storage + t.blocked_other),
        pct(t.idle)
    )
}

fn stretch(name: &str, s: &Stretch) {
    println!(
        "  {name:<24} {}  {:>4} clicks  {:>3} storage trips  logs {:+}  planks {:+}",
        clock(s.seconds),
        s.actions,
        s.storage_trips,
        s.logs_gained,
        s.planks_gained
    );
    println!("      you:     {}", shares(&s.player));
    println!("      spirits: {}", spirit_shares(&s.spirits));
}

fn report(r: &PacingReport) {
    println!("\n=== {} profile ===", r.profile);
    for (m, t) in &r.milestones {
        println!("  {}  {}", clock(*t), m.label());
    }
    println!("  final guidance phase: {:?}", r.final_phase);
    println!(
        "  sacrificed {} Woodcutting XP and {} Woodworking XP",
        r.woodcutting_sacrificed, r.woodworking_sacrificed
    );
    println!("Regaining what was sacrificed, from the same moment:");
    stretch("Woodcutting, alone", &r.retrain_alone);
    stretch("Woodcutting, supported", &r.retrain_supported);
    stretch("Woodworking, alone", &r.resaw_alone);
    stretch("Woodworking, supported", &r.resaw_supported);
    println!(
        "Whole session {}: {}",
        clock(r.total_seconds),
        shares(&r.player_time)
    );
}

fn main() {
    let profiles: Vec<String> = GameDefs::builtin()
        .profiles
        .iter()
        .map(|p| p.id.clone())
        .collect();
    for id in profiles {
        match play_first_cycle(&id) {
            Ok((w, r)) => {
                report(&r);
                w.check_conservation().expect("conservation");
            }
            Err(e) => println!("\n=== {id} profile ===\n  FAILED: {e}"),
        }
    }
}
