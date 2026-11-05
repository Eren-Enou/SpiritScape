//! Measures the four v0.6 scenarios without graphics. Deterministic: the
//! numbers repeat exactly.
//!
//!     cargo run -p spirit_sim --release --example specialization

use spirit_sim::autoplay::{RetrainResult, compare_chopping, compare_sawing};
use spirit_sim::defs::{JobKind, ResourceId};
use spirit_sim::{Command, ScenarioKind, World};

fn seconds(w: &World) -> u64 {
    w.tick / w.defs.ticks_per_second as u64
}

fn crafted(w: &World, r: ResourceId) -> u64 {
    w.ledger.crafted.get(&r).copied().unwrap_or(0)
}

/// Planks per minute over one minute, once the line is running.
fn plank_rate(w: &mut World) -> u64 {
    let planks = w.settlement.order.output;
    w.run_ticks(300);
    let before = crafted(w, planks);
    w.run_ticks(600);
    crafted(w, planks) - before
}

fn advanced() {
    println!("Scenario A: advanced specialization");
    let mut w = World::from_scenario(ScenarioKind::AdvancedSpecialization);
    let (wc, ww) = (
        w.defs.skill_id("woodcutting").unwrap(),
        w.defs.skill_id("woodworking").unwrap(),
    );
    let table = w.defs.xp_table.clone();
    println!(
        "  level 10 costs {} XP; development spirit at 1154 XP keeps Ironwood and Beams",
        table.xp_for_level(10)
    );
    for skill in [wc, ww] {
        for o in w.soul_options(skill) {
            println!(
                "  {:<9} {:<22} {:>5} XP  you Lv {:>2}  spirit Lv {:>2}  {}",
                w.defs.skill(skill).name,
                o.label,
                o.preview.transfer,
                o.preview.after_level,
                o.preview.spirit_level,
                o.preview.benefit
            );
        }
    }
    let normal = spirit_sim::GameDefs::builtin_profile("normal").unwrap();
    println!(
        "  in the normal profile level 10 costs {} XP",
        normal.xp_table.xp_for_level(10)
    );
}

fn bottleneck() {
    println!("Scenario B: production bottleneck (planks in one minute)");
    for workers in 1..=4 {
        let mut w = World::from_scenario(ScenarioKind::ProductionBottleneck);
        let crafters: Vec<usize> = w.crafters().map(|s| s.id).collect();
        for &spirit in crafters.iter().skip(workers) {
            w.apply(Command::SetSpiritAssigned {
                spirit,
                assigned: false,
            })
            .unwrap();
        }
        let made = plank_rate(&mut w);
        let r = w.production_report();
        println!(
            "  {workers} worker(s): {made} planks/min, limit: {}",
            r.bottleneck.label()
        );
        if workers == 4 {
            println!("    {}", r.summary);
        }
    }
}

fn expanded() {
    println!("Scenario C: expanded production");
    let mut w = World::from_scenario(ScenarioKind::ExpandedProduction);
    let planks = w.settlement.order.output;
    let beams = w.defs.resource_id("beams").unwrap();
    let kind = w.defs.building_kind_id("workshop").unwrap();
    let count = |w: &World| w.stations().filter(|b| b.kind == kind).count();
    let mut before = None;
    for _ in 0..20_000 {
        w.step();
        if before.is_none() && w.tick == 300 {
            before = Some(crafted(&w, planks));
        }
        if count(&w) == 2 {
            break;
        }
    }
    println!(
        "  second Workshop open after {}s; {} planks and {} beams made so far",
        seconds(&w),
        crafted(&w, planks),
        crafted(&w, beams)
    );
    let bottleneck_rate = plank_rate(&mut World::from_scenario(
        ScenarioKind::ProductionBottleneck,
    ));
    let main = w.main_storage;
    let logs = w.defs.objective.resource;
    w.buildings[main].storage.capacity += 400;
    w.buildings[main].storage.deposit(logs, 400);
    *w.ledger.seeded.entry(logs).or_default() += 400;
    let after = plank_rate(&mut w);
    println!("  one Workshop: {bottleneck_rate} planks/min; two Workshops: {after} planks/min");
    for s in w.station_reports() {
        println!(
            "    {} #{}: {}/{} places, {} made",
            s.name, s.id, s.occupied, s.slots, s.crafted
        );
    }
    for o in w.order_reports() {
        println!("    {}", o.status);
    }
    let haulers: Vec<_> = w
        .spirits
        .iter()
        .filter(|s| w.defs.role(s.role).has_job(JobKind::Supply))
        .collect();
    println!("    {} hauler(s)", haulers.len());
    w.check_conservation().unwrap();
}

fn table(rows: &[RetrainResult], xp: u64) {
    for r in rows {
        let s = &r.stretch;
        let total = s.player.total().max(1);
        println!(
            "  {:<44} {:>2}:{:02}  {:>4} XP/min  {:>3} clicks  {:>2} trips  walk {:>2}%  idle {:>2}%",
            r.setup,
            s.seconds / 60,
            s.seconds % 60,
            r.xp_per_min(xp),
            s.actions,
            s.storage_trips,
            s.player.traveling * 100 / total,
            s.player.idle * 100 / total,
        );
    }
}

fn main() {
    advanced();
    bottleneck();
    expanded();
    println!("Scenario D: supported retraining");
    println!(" Woodworking, 1500 XP:");
    table(&compare_sawing(1500), 1500);
    println!(" Woodcutting, 1000 XP:");
    table(&compare_chopping(1000), 1000);
}
