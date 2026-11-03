//! Times the v0.4 production chain without graphics.
//!
//!     cargo run -p spirit_sim --example workshop_playtest
//!
//! Prints how long each scenario takes to reach its milestones, and checks
//! resource conservation on every tick.

use spirit_sim::{Command, ScenarioKind, World};

fn seconds(w: &World) -> f64 {
    w.tick as f64 / w.defs.ticks_per_second as f64
}

fn run_until(w: &mut World, max_ticks: u64, done: impl Fn(&World) -> bool) -> bool {
    for _ in 0..max_ticks {
        if done(w) {
            return true;
        }
        w.step();
        w.check_conservation().expect("conservation violated");
    }
    false
}

fn idle(w: &World) -> bool {
    w.player.actor.queue.is_empty()
}

fn manual() {
    let mut w = World::from_scenario(ScenarioKind::WorkshopManual);
    let skill = w.defs.skill_id("woodworking").unwrap();
    let main = w.main().pos;
    let bench = w.station().unwrap().pos;
    println!("Scenario A: manual");
    while w.player.level(skill, &w.defs.xp_table) < 6 {
        w.apply(Command::PlayerInteract(main)).ok();
        run_until(&mut w, 2000, idle);
        w.apply(Command::PlayerInteract(bench)).ok();
        run_until(&mut w, 5000, idle);
        w.apply(Command::PlayerInteract(main)).ok();
        run_until(&mut w, 2000, idle);
    }
    println!(
        "  level 6 after {:.0}s, {} XP, {} planks stored",
        seconds(&w),
        w.player.xp(skill),
        w.main().storage.get(w.settlement.order.output)
    );
    let xp = w.player.xp(skill);
    let id = w.soul_separate(skill, 400).unwrap();
    let start = w.tick;
    println!(
        "  separated 400 of {xp} XP: you are level {}, spirit level {}",
        w.player.level(skill, &w.defs.xp_table),
        w.spirits[id].level()
    );
    let target = w.settlement.order.target;
    let output = w.settlement.order.output;
    let reached = run_until(&mut w, 12_000, |w| w.main().storage.get(output) >= target);
    println!(
        "  spirit reached {target} planks: {reached}, {:.0}s after the sacrifice",
        (w.tick - start) as f64 / w.defs.ticks_per_second as f64
    );
}

fn automated() {
    let mut w = World::from_scenario(ScenarioKind::WorkshopAutomated);
    let output = w.settlement.order.output;
    println!("Scenario B: automated");
    run_until(&mut w, 12_000, |w| w.main().storage.get(output) >= 30);
    println!("  30 planks in Main Storage after {:.0}s", seconds(&w));
    let logs = w.defs.objective.resource;
    println!(
        "  harvested {} logs, used {}",
        w.ledger.harvested.get(&logs).copied().unwrap_or(0),
        w.ledger.processed.get(&logs).copied().unwrap_or(0)
    );
    w.apply(Command::SetProductionTarget(120)).unwrap();
    let start = w.tick;
    run_until(&mut w, 30_000, |w| w.main().storage.get(output) >= 120);
    let r = w.production_report();
    println!(
        "  raised to 120: reached after {:.0}s more, {:.1} planks/min, last limit {}",
        (w.tick - start) as f64 / w.defs.ticks_per_second as f64,
        r.output_per_min,
        r.bottleneck.label()
    );
}

fn construction() {
    let mut w = World::from_scenario(ScenarioKind::WorkshopConstruction);
    println!("Scenario C: construction");
    run_until(&mut w, 2000, |w| w.projects[0].reserved);
    println!("  materials reserved after {:.0}s", seconds(&w));
    run_until(&mut w, 20_000, |w| !w.projects[0].is_active());
    println!(
        "  Storehouse built after {:.0}s, Main Storage capacity {}",
        seconds(&w),
        w.main().storage.capacity
    );
}

fn main() {
    manual();
    automated();
    construction();
}
