//! v0.3 tests: Woodcutting tiers, Soul Separation consequences and
//! retraining, sustainable gathering sites, measurements and spirit responses
//! to shortages.

use spirit_sim::defs::{BUILTIN_DEFS, GameDefs, SiteId, SkillId, TreeKindId};
use spirit_sim::entities::Activity;
use spirit_sim::grid::{BUILTIN_MAP, MapLayout, TilePos};
use spirit_sim::metrics::{Bottleneck, WaitKind};
use spirit_sim::scenario::{BUILTIN_SCENARIOS, ScenarioDef, load_scenarios};
use spirit_sim::{Command, ScenarioKind, World};

const MINUTE: u64 = 600;

fn scenario(kind: ScenarioKind, patch: impl Fn(&mut ScenarioDef)) -> World {
    let mut scenarios = load_scenarios(BUILTIN_SCENARIOS).unwrap();
    let s = scenarios.get_mut(kind.key()).unwrap();
    patch(s);
    World::build(
        GameDefs::from_toml(BUILTIN_DEFS).unwrap(),
        &MapLayout::parse(BUILTIN_MAP).unwrap(),
        s,
    )
    .unwrap()
}

fn woodcutting(w: &World) -> SkillId {
    w.defs.skill_id("woodcutting").unwrap()
}

fn oak(w: &World) -> TreeKindId {
    w.defs.tree_tiers(woodcutting(w))[1]
}

fn basic_tree(w: &World) -> TreeKindId {
    w.defs.tree_tiers(woodcutting(w))[0]
}

fn grove(w: &World) -> SiteId {
    w.defs.site_id("starting_grove").unwrap()
}

fn forest(w: &World) -> SiteId {
    w.defs.site_id("distant_forest").unwrap()
}

fn run_until(w: &mut World, max_ticks: u64, mut cond: impl FnMut(&World) -> bool) -> bool {
    for _ in 0..max_ticks {
        if cond(w) {
            return true;
        }
        w.step();
    }
    cond(w)
}

fn nearest(w: &World, kind: TreeKindId) -> Option<TilePos> {
    let p = w.player.actor.pos();
    w.trees
        .iter()
        .filter(|t| t.kind == kind && t.is_grown())
        .min_by_key(|t| t.pos.manhattan(p))
        .map(|t| t.pos)
}

/// Chops trees of `kind` (depositing when full) until the player has `xp`.
fn train(w: &mut World, kind: TreeKindId, xp: u64) {
    let skill = woodcutting(w);
    for _ in 0..300 {
        if w.player.xp(skill) >= xp {
            return;
        }
        if w.player.actor.inventory.is_full() {
            let main = w.main().pos;
            w.apply(Command::PlayerInteract(main)).unwrap();
        } else if let Some(tree) = nearest(w, kind) {
            w.apply(Command::PlayerInteract(tree)).unwrap();
        } else {
            w.run_ticks(50);
            continue;
        }
        run_until(w, 3000, |w| w.player.actor.queue.is_empty());
    }
    panic!("could not train to {xp} XP");
}

fn grown_on(w: &World, site: SiteId) -> usize {
    w.trees
        .iter()
        .filter(|t| t.site == Some(site) && t.is_grown())
        .count()
}

// ---------------------------------------------------------------------------
// Tiers and skill requirements
// ---------------------------------------------------------------------------

#[test]
fn the_player_needs_the_level_to_cut_a_higher_tier() {
    let mut w = World::from_scenario(ScenarioKind::NewGame);
    let oak = oak(&w);
    assert!(w.defs.tree(oak).level_required > 1);
    let oak_pos = nearest(&w, oak).unwrap();
    let err = w.apply(Command::PlayerInteract(oak_pos)).unwrap_err();
    assert!(err.contains("level 5"), "{err}");
    let tree_pos = nearest(&w, basic_tree(&w)).unwrap();
    assert!(w.apply(Command::PlayerInteract(tree_pos)).is_ok());
}

#[test]
fn tiers_differ_in_xp_yield_and_recovery() {
    let w = World::from_scenario(ScenarioKind::NewGame);
    let (t, o) = (w.defs.tree(basic_tree(&w)), w.defs.tree(oak(&w)));
    assert!(o.xp_per_log > t.xp_per_log);
    assert!(o.logs > t.logs);
    assert!(o.regrow_ticks > t.regrow_ticks);
}

#[test]
fn low_level_spirits_only_cut_trees_they_qualify_for() {
    // A level 2 woodcutter in the grove (which has both Trees and Oaks).
    let mut w = scenario(ScenarioKind::GroveSustainable, |s| s.spirits[0].xp = 100);
    let oak = oak(&w);
    let mut gathered = false;
    for _ in 0..5 * MINUTE {
        w.step();
        assert!(
            w.trees
                .iter()
                .all(|t| t.kind != oak || t.claimed_by.is_none()),
            "a level 2 spirit claimed an oak"
        );
        gathered |= w.spirits[0].units_gathered > 0;
    }
    assert!(gathered, "the spirit still gathers basic trees");
    assert!(
        w.trees
            .iter()
            .filter(|t| t.kind == oak)
            .all(|t| t.is_grown())
    );
}

// ---------------------------------------------------------------------------
// Soul Separation, lost access and retraining
// ---------------------------------------------------------------------------

#[test]
fn soul_preview_explains_the_sacrifice() {
    let mut w = World::from_scenario(ScenarioKind::Regrind);
    let skill = woodcutting(&w);
    let oak = oak(&w);
    let tree = basic_tree(&w);
    train(&mut w, tree, 420);
    let p = w.soul_preview(skill, 400);
    assert_eq!(p.error, None);
    assert_eq!(p.current_level, 5);
    assert_eq!(p.after_xp, w.player.xp(skill) - 400);
    assert!(p.after_level < 5);
    assert_eq!(p.spirit_level, 5);
    assert_eq!(p.lost, vec![oak], "the player loses Oak");
    assert!(p.spirit_can_cut.contains(&oak), "the spirit keeps Oak");

    // A smaller sacrifice gives a spirit too weak for Oak.
    let small = w.soul_preview(skill, 100);
    assert_eq!(small.spirit_level, 2);
    assert!(!small.spirit_can_cut.contains(&oak));
    assert!(w.soul_preview(skill, 50).error.is_some());
    assert!(w.soul_preview(skill, 100_000).error.is_some());
}

#[test]
fn separation_removes_player_access_but_the_spirit_keeps_it_permanently() {
    let mut w = World::from_scenario(ScenarioKind::Regrind);
    let skill = woodcutting(&w);
    let oak = oak(&w);
    let starting_xp = w.player.xp(skill);

    // 1-2. Train on Trees until Oak unlocks, then cut Oak.
    let tree = basic_tree(&w);
    train(&mut w, tree, 388);
    assert!(w.player_can_cut(oak));
    assert!(
        w.events
            .contains(spirit_sim::events::EventKind::Player, "You can now cut Oak")
    );
    train(&mut w, oak, 480);
    let peak = w.player.level(skill, &w.defs.xp_table);

    // 3. Start chopping an Oak, then sacrifice mid-chop.
    let oak_pos = nearest(&w, oak).unwrap();
    w.apply(Command::PlayerInteract(oak_pos)).unwrap();
    run_until(&mut w, 400, |w| {
        matches!(
            w.player.actor.queue.front(),
            Some(spirit_sim::tasks::Task::Chop { .. })
        )
    });
    let xp_before = w.player.xp(skill);
    let id = w.soul_separate(skill, 400).unwrap();
    assert_eq!(w.player.xp(skill), xp_before - 400, "exact transfer");
    assert_eq!(w.spirits[id].xp(), 400);
    assert!(!w.player_can_cut(oak));
    let xp_after = w.player.xp(skill);
    w.run_ticks(100);
    assert_eq!(
        w.player.xp(skill),
        xp_after,
        "the interrupted oak chop gives no XP"
    );
    let err = w.apply(Command::PlayerInteract(oak_pos)).unwrap_err();
    assert!(err.contains("level 5"), "{err}");
    assert!(w.events.contains(
        spirit_sim::events::EventKind::Player,
        "You can no longer cut Oak until you retrain"
    ));

    let h = w.player.history(skill);
    assert_eq!(h.highest_level, peak);
    assert_eq!(h.xp_sacrificed, 400);
    assert_eq!(h.spirits_created, 1);

    // 4. The spirit cuts Oak with its permanent level.
    let oaks_cut = |w: &World| {
        w.trees
            .iter()
            .filter(|t| t.kind == oak)
            .map(|t| w.defs.tree(oak).logs - t.logs_left())
            .sum::<u32>()
    };
    let before = oaks_cut(&w);
    assert!(
        run_until(&mut w, 3 * MINUTE, |w| oaks_cut(w) > before + 3),
        "the level 5 spirit cuts oaks"
    );

    // 5-6. The player retrains on Trees while the spirit supplies the settlement.
    let delivered_before = w.spirits[id].units_delivered;
    let target = w.defs.xp_table.xp_for_level(5);
    train(&mut w, tree, target);
    assert!(w.player_can_cut(oak), "Oak access regained by retraining");
    assert!(
        w.events
            .contains(spirit_sim::events::EventKind::Player, "regained")
    );
    assert_eq!(w.player.history(skill).highest_level, peak);
    assert_eq!(w.spirits[id].level(), 5, "spirit competency never changes");
    assert_eq!(w.spirits[id].xp(), 400);
    assert!(w.spirits[id].units_delivered > delivered_before);
    assert_eq!(
        starting_xp + w.ledger.xp_earned,
        w.player.xp(skill) + w.ledger.xp_separated,
        "XP is never duplicated or refunded"
    );
    w.check_conservation().unwrap();
}

// ---------------------------------------------------------------------------
// Sites: depletion, recovery, sustainable and excessive demand
// ---------------------------------------------------------------------------

#[test]
fn trees_belong_to_sites_and_recover_on_schedule() {
    let w = World::from_scenario(ScenarioKind::NewGame);
    let (g, f) = (grove(&w), forest(&w));
    assert!(w.trees.iter().all(|t| t.site.is_some()));
    // The forest's oaks recover faster than the grove's (regrow_pct).
    let oak = oak(&w);
    let regrow = |site| {
        w.trees
            .iter()
            .find(|t| t.kind == oak && t.site == Some(site))
            .unwrap()
            .regrow_ticks
    };
    assert!(regrow(f) < regrow(g));
    let r = w.gathering_report();
    assert!(r.sites[f.index()].sustainable_per_min > r.sites[g.index()].sustainable_per_min);
    assert!(r.sites[f.index()].logs_max > r.sites[g.index()].logs_max);
}

#[test]
fn an_exhausted_site_fully_recovers() {
    let mut w = World::from_scenario(ScenarioKind::GroveOversubscribed);
    let g = grove(&w);
    assert!(
        run_until(&mut w, 5 * MINUTE, |w| grown_on(w, g) == 0),
        "four spirits exhaust the grove"
    );
    // Stop demand; every tree must come back.
    w.apply(Command::SetObjectiveTarget(0)).unwrap();
    let total = w.trees.iter().filter(|t| t.site == Some(g)).count();
    let slowest = w.trees.iter().map(|t| t.regrow_ticks).max().unwrap();
    assert!(
        run_until(&mut w, slowest + 10, |w| grown_on(w, g) == total),
        "grove recovers within the slowest regrowth time"
    );
}

#[test]
fn one_spirit_gathers_sustainably_at_the_grove() {
    let mut w = World::from_scenario(ScenarioKind::GroveSustainable);
    let g = grove(&w);
    let mut min_grown = usize::MAX;
    for tick in 0..10 * MINUTE {
        w.step();
        if tick > MINUTE {
            min_grown = min_grown.min(grown_on(&w, g));
        }
    }
    let r = w.gathering_report();
    let site = &r.sites[g.index()];
    let t = &r.gatherers;
    assert!(min_grown > 0, "the grove never runs out");
    assert!(t.pct_of_working(t.waiting_recovery) < 5, "{t:?}");
    assert!(site.measured_per_min > 10.0);
    assert!(site.measured_per_min < site.sustainable_per_min);
    assert_eq!(r.bottleneck, Bottleneck::GatheringSpeed, "{}", r.summary);
}

#[test]
fn too_many_spirits_cause_visible_waiting_for_recovery() {
    let mut w = World::from_scenario(ScenarioKind::GroveOversubscribed);
    let g = grove(&w);
    w.run_ticks(6 * MINUTE);
    let r = w.gathering_report();
    let site = &r.sites[g.index()];
    let t = &r.gatherers;
    assert_eq!(r.bottleneck, Bottleneck::ResourceRecovery, "{}", r.summary);
    assert!(r.summary.contains("recovering"), "{}", r.summary);
    assert!(t.pct_of_working(t.waiting_recovery) >= 25, "{t:?}");
    // Output is capped near the grove's regrowth, far below chop capacity.
    assert!(site.measured_per_min < site.sustainable_per_min * 1.3);
    assert!(r.chop_capacity_per_min > 2.0 * site.measured_per_min);
    assert_eq!(site.workers, 4);
    let waiting = w
        .spirits
        .iter()
        .filter(|s| s.wait == Some(WaitKind::Recovery))
        .count();
    assert!(waiting > 0, "some spirits are visibly waiting");
}

#[test]
fn waiting_spirits_resume_when_trees_regrow() {
    let mut w = World::from_scenario(ScenarioKind::GroveOversubscribed);
    assert!(run_until(&mut w, 5 * MINUTE, |w| w.spirits[0].wait
        == Some(WaitKind::Recovery)));
    assert!(matches!(w.spirits[0].activity, Activity::Blocked(ref r) if r.contains("regrowth")));
    let gathered = w.spirits[0].units_gathered;
    assert!(
        run_until(&mut w, 3 * MINUTE, |w| w.spirits[0].units_gathered
            > gathered),
        "the spirit resumes gathering after recovery"
    );
}

#[test]
fn spirits_relocate_when_their_site_runs_dry_without_oscillating() {
    // Four spirits, both sites open: the grove can't feed them all.
    let mut w = scenario(ScenarioKind::GroveOversubscribed, |s| {
        s.closed_sites.clear()
    });
    let (g, f) = (grove(&w), forest(&w));
    let mut switches = vec![0; w.spirits.len()];
    let mut last: Vec<_> = w.spirits.iter().map(|s| s.home_site).collect();
    for _ in 0..10 * MINUTE {
        w.step();
        for s in &w.spirits {
            if s.home_site != last[s.id] {
                if last[s.id].is_some() {
                    switches[s.id] += 1;
                }
                last[s.id] = s.home_site;
            }
        }
    }
    assert!(
        w.spirits.iter().any(|s| s.home_site == Some(f)),
        "someone moved to the forest"
    );
    assert!(w.events.contains(
        spirit_sim::events::EventKind::Spirit,
        "relocates from Starting Grove to Distant Forest"
    ));
    assert!(
        switches.iter().all(|&n| n <= 4),
        "site switches per spirit in 10 minutes: {switches:?}"
    );
    let _ = g;
    w.check_conservation().unwrap();
}

#[test]
fn reserving_a_site_keeps_spirits_out_without_creating_resources() {
    let mut w = scenario(ScenarioKind::GroveOversubscribed, |s| {
        s.closed_sites.clear()
    });
    let g = grove(&w);
    w.run_ticks(MINUTE);
    w.apply(Command::SetSiteOpen {
        site: g,
        open: false,
    })
    .unwrap();
    // Let current chops finish, then the grove must be left alone.
    w.run_ticks(MINUTE / 2);
    let harvested = w.site_stats[g.index()].harvested_total;
    for _ in 0..4 * MINUTE {
        w.step();
        w.check_conservation().unwrap();
        assert!(
            w.trees
                .iter()
                .all(|t| t.site != Some(g) || t.claimed_by.is_none())
        );
    }
    assert_eq!(w.site_stats[g.index()].harvested_total, harvested);
    // The player can still train there.
    let tree = nearest(&w, basic_tree(&w)).unwrap();
    assert!(w.apply(Command::PlayerInteract(tree)).is_ok());
}

// ---------------------------------------------------------------------------
// Conservation and determinism in every scenario
// ---------------------------------------------------------------------------

#[test]
fn every_scenario_conserves_resources_and_replays_exactly() {
    for kind in ScenarioKind::ALL {
        let run = || {
            let mut w = World::from_scenario(kind);
            for _ in 0..6 * MINUTE {
                w.step();
                w.check_conservation()
                    .unwrap_or_else(|e| panic!("{kind:?}: {e}"));
            }
            (w.state_hash(), w.events.total)
        };
        assert_eq!(run(), run(), "{kind:?} is not deterministic");
    }
}
