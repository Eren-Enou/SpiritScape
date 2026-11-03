//! Scripted, repeatable version of the regrind playtest (v0.3, section 6):
//! train to Oak, cut Oak, sacrifice enough XP to lose Oak while creating a
//! spirit that keeps it, then retrain on Trees while the spirit works.
//! Prints how long each phase takes in simulation time.
//!
//!     cargo run -p spirit_sim --example regrind_playtest -- [sacrifice_xp]

use spirit_sim::grid::TilePos;
use spirit_sim::{Command, ScenarioKind, World};

fn main() {
    let sacrifice: u64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(400);
    let mut w = World::from_scenario(ScenarioKind::Regrind);
    let skill = w.defs.skill_id("woodcutting").unwrap();
    let tiers = w.defs.tree_tiers(skill);
    let (tree, oak) = (tiers[0], tiers[1]);
    let oak_level = w.defs.tree(oak).level_required;
    let oak_xp = w.defs.xp_table.xp_for_level(oak_level);
    let logs = w.defs.resource_id("logs").unwrap();
    let tps = w.defs.ticks_per_second as u64;
    let clock = |w: &World| format!("{:>3}:{:02}", w.tick / tps / 60, w.tick / tps % 60);
    let level = |w: &World| w.player.level(skill, &w.defs.xp_table);

    println!(
        "{} start: level {}, {} XP",
        clock(&w),
        level(&w),
        w.player.xp(skill)
    );
    train(&mut w, tree, oak_xp);
    println!(
        "{} 1-2. Oak unlocked at level {} ({} XP)",
        clock(&w),
        level(&w),
        w.player.xp(skill)
    );
    let target = oak_xp + sacrifice.saturating_sub(oak_xp).max(90);
    train(&mut w, oak, target);
    let peak = level(&w);
    println!(
        "{}      cut Oak up to level {peak} ({} XP)",
        clock(&w),
        w.player.xp(skill)
    );

    let p = w.soul_preview(skill, sacrifice);
    let id = match w.soul_separate(skill, sacrifice) {
        Ok(id) => id,
        Err(e) => panic!("separation failed: {e}"),
    };
    println!(
        "{} 3-4. Sacrificed {sacrifice} XP: level {} -> {}; {} is permanent level {}; player lost {:?}, spirit can cut {:?}",
        clock(&w),
        p.current_level,
        p.after_level,
        w.spirits[id].name,
        w.spirits[id].level(),
        p.lost
            .iter()
            .map(|k| &w.defs.tree(*k).name)
            .collect::<Vec<_>>(),
        p.spirit_can_cut
            .iter()
            .map(|k| &w.defs.tree(*k).name)
            .collect::<Vec<_>>()
    );

    let start = w.tick;
    let stock_before = w.main().storage.get(logs);
    train(&mut w, tree, oak_xp);
    let mins = (w.tick - start) as f64 / tps as f64 / 60.0;
    println!(
        "{} 5.   Retrained to level {} in {mins:.1} min (Oak regained)",
        clock(&w),
        level(&w)
    );
    println!(
        "        6.   Meanwhile {} delivered {} logs ({:.1}/min); Main Storage went {} -> {}",
        w.spirits[id].name,
        w.spirits[id].units_delivered,
        w.spirits[id].units_delivered as f64 / mins.max(0.01),
        stock_before,
        w.main().storage.get(logs)
    );
    let r = w.gathering_report();
    println!(
        "        Bottleneck: {} - {}",
        r.bottleneck.label(),
        r.summary
    );
    w.check_conservation().unwrap();
}

/// Chops the nearest grown tree of `kind` (depositing when full) until `xp`.
fn train(w: &mut World, kind: spirit_sim::defs::TreeKindId, xp: u64) {
    let skill = w.defs.skill_id("woodcutting").unwrap();
    while w.player.xp(skill) < xp {
        let p = w.player.actor.pos();
        let next: Option<TilePos> = w
            .trees
            .iter()
            .filter(|t| t.kind == kind && t.is_grown())
            .min_by_key(|t| t.pos.manhattan(p))
            .map(|t| t.pos);
        let target = if w.player.actor.inventory.is_full() {
            Some(w.main().pos)
        } else {
            next
        };
        match target {
            Some(t) => {
                w.apply(Command::PlayerInteract(t)).unwrap();
                for _ in 0..3000 {
                    if w.player.actor.queue.is_empty() {
                        break;
                    }
                    w.step();
                }
            }
            None => w.run_ticks(50),
        }
    }
}
