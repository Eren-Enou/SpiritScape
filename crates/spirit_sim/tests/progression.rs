//! Progression profiles, Soul Separation decisions, training support and the
//! complete first cycle of a real playthrough.

use spirit_sim::GameDefs;
use spirit_sim::autoplay::{Bot, Goal, PacingReport, play_first_cycle};
use spirit_sim::defs::{JobKind, ResourceId, SkillId};
use spirit_sim::journey::{Milestone, Phase};
use spirit_sim::save;
use spirit_sim::{Command, ScenarioKind, World};

fn wc(w: &World) -> SkillId {
    w.defs.skill_id("woodcutting").unwrap()
}

fn ww(w: &World) -> SkillId {
    w.defs.skill_id("woodworking").unwrap()
}

fn logs(w: &World) -> ResourceId {
    w.defs.resource_id("logs").unwrap()
}

fn planks(w: &World) -> ResourceId {
    w.defs.resource_id("planks").unwrap()
}

fn run_until(w: &mut World, max: u64, mut done: impl FnMut(&World) -> bool) -> bool {
    for _ in 0..max {
        if done(w) {
            return true;
        }
        w.step();
    }
    done(w)
}

/// A settlement with stock, gatherers, a hauler and a woodworker at rest.
fn settlement() -> World {
    let mut w = World::from_scenario(ScenarioKind::WorkshopAutomated);
    w.apply(Command::SetProductionEnabled(false)).unwrap();
    w
}

fn order_cache(w: &mut World) {
    let grove = w.defs.site_id("starting_grove").unwrap();
    w.apply(Command::SetTrainingArea(Some(grove))).unwrap();
    let kind = w.training_cache_kind().unwrap();
    w.apply(Command::RequestBuilding(kind)).unwrap();
    assert!(run_until(w, 3000, |w| w.training_cache().is_some()));
}

/// Adds stock the test needs, accounted for as if the scenario had seeded it.
fn seed_cache(w: &mut World, n: u32) -> usize {
    let id = w.training_cache().unwrap().id;
    let l = logs(w);
    w.buildings[id].storage.deposit(l, n);
    *w.ledger.seeded.entry(l).or_default() += n as u64;
    id
}

// --- Profiles ---------------------------------------------------------------

#[test]
fn profiles_are_data_and_the_default_stays_fast() {
    let dev = GameDefs::builtin();
    let normal = GameDefs::builtin_profile("normal").unwrap();
    assert_eq!(dev.profile.id, "development");
    assert_eq!(dev.xp_table.xp_for_level(5), 388, "tests rely on this table");
    assert!(normal.xp_table.xp_for_level(5) > dev.xp_table.xp_for_level(5) * 3);
    assert!(normal.soul_min_xp > dev.soul_min_xp);
    assert!(GameDefs::builtin_profile("hard").is_err());
    assert_eq!(dev.profiles.len(), 2);
}

#[test]
fn only_the_profile_block_differs_between_profiles() {
    let dev = GameDefs::builtin();
    let normal = GameDefs::builtin_profile("normal").unwrap();
    for (a, b) in dev.trees.iter().zip(&normal.trees) {
        assert_eq!(a.xp_per_log, b.xp_per_log);
        assert_eq!(a.level_required, b.level_required);
    }
    for (a, b) in dev.recipes.iter().zip(&normal.recipes) {
        assert_eq!((a.xp, a.level_required), (b.xp, b.level_required));
    }
    let storehouse = |d: &GameDefs| {
        d.buildings
            .iter()
            .find(|b| b.id == "storehouse")
            .unwrap()
            .cost
            .clone()
    };
    assert_eq!(storehouse(&normal)[0].1, storehouse(&dev)[0].1 * 4);
}

#[test]
fn normal_levels_take_longer_the_higher_they_are() {
    let t = GameDefs::builtin_profile("normal").unwrap().xp_table;
    let gaps: Vec<u64> = (1..t.max_level())
        .map(|l| t.xp_for_level(l + 1) - t.xp_for_level(l))
        .collect();
    assert!(gaps.windows(2).all(|g| g[1] > g[0]), "{gaps:?}");
}

#[test]
fn the_same_play_reaches_lower_levels_under_normal() {
    let mut levels = Vec::new();
    for profile in ["development", "normal"] {
        let mut w = World::new_journey(profile).unwrap();
        let mut bot = Bot::new(&w);
        bot.chop_until(&mut w, 900, |_| false);
        levels.push((
            w.player.xp(wc(&w)),
            w.player.level(wc(&w), &w.defs.xp_table),
        ));
    }
    assert!(levels[1].1 < levels[0].1, "{levels:?}");
}

#[test]
fn founding_spirits_match_the_level_in_both_profiles() {
    for profile in ["development", "normal"] {
        let w = World::new_journey(profile).unwrap();
        assert!(w.spirits.iter().all(|s| s.level() == 5), "{profile}");
    }
}

// --- Soul Separation --------------------------------------------------------

#[test]
fn soul_separation_rules_hold_at_every_level_in_both_profiles() {
    for profile in ["development", "normal"] {
        for level in [3, 5, 7] {
            let mut w = World::new_journey(profile).unwrap();
            let skill = wc(&w);
            let xp = w.defs.xp_table.xp_for_level(level) + 7;
            let mut bot = Bot::new(&w);
            assert!(bot.run(&mut w, Goal::ChopTo(xp), 60_000));
            let have = w.player.xp(skill);
            let give = (have / 2).max(w.defs.soul_min_xp);
            let preview = w.soul_preview(skill, give);
            let id = w.soul_separate(skill, give).unwrap();
            assert_eq!(w.player.xp(skill), have - give, "exact transfer");
            assert_eq!(w.spirits[id].xp(), give, "exact transfer");
            assert_eq!(w.spirits[id].level(), preview.spirit_level);
            assert_eq!(
                w.player.level(skill, &w.defs.xp_table),
                preview.after_level,
                "the preview tells the truth"
            );
            assert_eq!(w.ledger.xp_separated, give);
            w.run_ticks(1200);
            assert_eq!(w.spirits[id].xp(), give, "spirits never gain XP");
        }
    }
}

#[test]
fn unlocks_are_lost_to_a_sacrifice_and_regained_by_training() {
    let mut w = World::new_journey("development").unwrap();
    let skill = wc(&w);
    let oak = w
        .trees
        .iter()
        .find(|t| w.defs.tree(t.kind).id == "oak")
        .unwrap()
        .kind;
    let mut bot = Bot::new(&w);
    assert!(bot.run(&mut w, Goal::ChopTo(520), 20_000));
    assert!(w.player_can_cut(oak));
    let have = w.player.xp(skill);
    assert_eq!(w.soul_preview(skill, 388).lost, vec![oak]);
    w.soul_separate(skill, 388).unwrap();
    assert!(!w.player_can_cut(oak));
    let oak_pos = w.trees.iter().find(|t| t.kind == oak).unwrap().pos;
    let refused = w.apply(Command::PlayerInteract(oak_pos));
    assert!(refused.unwrap_err().contains("You need Woodcutting level 5"));
    let mut bot = Bot::new(&w);
    assert!(bot.run(&mut w, Goal::ChopTo(have), 20_000));
    assert!(w.player_can_cut(oak));
    assert!(w.notices.contains("regained"));
    assert!(w.notices.contains("Oak trees are again available"));
    assert_eq!(w.player.history(skill).xp_sacrificed, 388);
}

#[test]
fn options_compare_real_consequences() {
    let mut w = World::from_scenario(ScenarioKind::WorkshopManual);
    let skill = ww(&w);
    let mut bot = Bot::new(&w);
    assert!(bot.run(&mut w, Goal::SawTo(800), 20_000));
    let options = w.soul_options(skill);
    assert!(options.len() >= 3 && options.len() <= 5, "{options:?}");
    let amounts: Vec<u64> = options.iter().map(|o| o.preview.transfer).collect();
    assert!(amounts.windows(2).all(|a| a[0] < a[1]), "{amounts:?}");
    assert!(options.iter().all(|o| o.preview.error.is_none()));
    let keep = options
        .iter()
        .find(|o| o.label == "Keep Precise Planks")
        .unwrap();
    assert!(keep.preview.lost_recipes.is_empty());
    let all = options.last().unwrap();
    assert_eq!(all.label, "Everything");
    assert_eq!(all.preview.after_xp, 0);
    assert!(!all.preview.lost_recipes.is_empty());
    assert!(
        all.preview.benefit.contains("Planks/min"),
        "{}",
        all.preview.benefit
    );
}

#[test]
fn nothing_is_offered_below_the_minimum() {
    let w = World::new_journey("normal").unwrap();
    assert!(w.soul_options(wc(&w)).is_empty());
}

// --- Training support -------------------------------------------------------

#[test]
fn gatherers_stay_out_of_the_training_area() {
    let mut w = World::from_scenario(ScenarioKind::WorkshopAutomated);
    let grove = w.defs.site_id("starting_grove").unwrap();
    let forest = w.defs.site_id("distant_forest").unwrap();
    w.apply(Command::SetObjectiveTarget(200)).unwrap();
    w.apply(Command::SetTrainingArea(Some(grove))).unwrap();
    w.run_ticks(100);
    let harvested = w.site_stats[grove.index()].harvested_total;
    w.run_ticks(1500);
    assert!(
        w.site_stats[grove.index()].harvested_total <= harvested + 10,
        "at most the load already under way"
    );
    assert!(w.site_stats[forest.index()].harvested_total > 0);
}

#[test]
fn a_low_level_gatherer_is_stranded_by_reserving_its_only_trees() {
    let mut w = World::new_journey("development").unwrap();
    let mut bot = Bot::new(&w);
    assert!(bot.run(&mut w, Goal::ChopTo(200), 20_000));
    w.soul_separate(wc(&w), 170).unwrap();
    let grove = w.defs.site_id("starting_grove").unwrap();
    let msg = w.apply(Command::SetTrainingArea(Some(grove))).unwrap();
    assert!(msg.contains("no open site"), "{msg}");
    // Giving the area up undoes it.
    w.apply(Command::SetTrainingArea(None)).unwrap();
    assert!(w.settlement.site_open[grove.index()]);
}

#[test]
fn the_cache_needs_an_area_and_real_construction() {
    let mut w = settlement();
    let kind = w.training_cache_kind().unwrap();
    let refused = w.apply(Command::RequestBuilding(kind)).unwrap_err();
    assert!(refused.contains("training area"), "{refused}");
    order_cache(&mut w);
    assert_eq!(w.ledger.consumed.get(&logs(&w)).copied(), Some(15));
    w.check_conservation().unwrap();
    assert!(w.notices.contains("Training Cache complete"));
}

#[test]
fn a_hauler_collects_the_players_logs_without_any_xp_changing_hands() {
    let mut w = settlement();
    order_cache(&mut w);
    w.apply(Command::SetTrainingCollect(true)).unwrap();
    let (cache_id, cache_pos) = {
        let c = w.training_cache().unwrap();
        (c.id, c.pos)
    };
    // The player carries a load to the cache and leaves it there.
    let l = logs(&w);
    w.player.actor.inventory.add(l, 20);
    *w.ledger.seeded.entry(l).or_default() += 20;
    w.apply(Command::PlayerInteract(cache_pos)).unwrap();
    assert!(run_until(&mut w, 400, |w| {
        w.player.actor.inventory.total() == 0
    }));
    // A hauler carries a load at a time; a few logs wait for the next one.
    assert!(run_until(&mut w, 1500, |w| {
        w.buildings[cache_id].storage.total() < 5
    }));
    assert!(run_until(&mut w, 900, |w| {
        w.notices
            .contains("has collected your training resources")
    }));
    assert_eq!(w.ledger.xp_earned, 0, "carrying logs earns nobody XP");
    w.check_conservation().unwrap();
}

#[test]
fn nothing_collects_with_support_off() {
    let mut w = settlement();
    order_cache(&mut w);
    let id = seed_cache(&mut w, 20);
    w.run_ticks(800);
    assert_eq!(w.buildings[id].storage.get(logs(&w)), 20);
}

#[test]
fn a_full_main_storage_stops_collection_without_losing_logs() {
    let mut w = settlement();
    order_cache(&mut w);
    w.apply(Command::SetTrainingCollect(true)).unwrap();
    let id = seed_cache(&mut w, 20);
    let main = w.main_storage;
    w.buildings[main].storage.capacity = w.buildings[main].storage.total();
    w.run_ticks(600);
    assert!(w.buildings[id].storage.get(logs(&w)) > 0);
    assert!(w.training_report().collect_status.contains("full"));
    w.check_conservation().unwrap();
}

#[test]
fn supply_fills_the_players_part_of_the_bench_and_respects_the_reserve() {
    let mut w = settlement();
    let l = logs(&w);
    w.apply(Command::SetInputReserve(25)).unwrap();
    w.apply(Command::SetTrainingSupply(true)).unwrap();
    let mut lowest = u32::MAX;
    for _ in 0..2500 {
        w.step();
        if w.main().storage.get(l) + w.bench_stock() > 25 {
            lowest = lowest.min(w.main().storage.get(l));
        }
        assert!(w.bench_stock() <= 20);
    }
    assert!(w.bench_stock() > 0);
    assert!(lowest >= 20, "stays near the reserve: {lowest}");
    w.check_conservation().unwrap();
}

#[test]
fn shortages_stay_possible() {
    let mut w = World::from_scenario(ScenarioKind::NewGame);
    w.apply(Command::SetTrainingSupply(true)).unwrap();
    w.run_ticks(600);
    assert_eq!(w.bench_stock(), 0, "no logs, so none arrive");
    assert!(w.training_report().supply_status.contains("No spare"));
}

#[test]
fn production_cannot_touch_logs_set_aside_for_the_player() {
    let mut w = settlement();
    w.apply(Command::SetInputReserve(0)).unwrap();
    w.apply(Command::SetTrainingSupply(true)).unwrap();
    assert!(run_until(&mut w, 1500, |w| w.bench_stock() >= 15));
    let mut off = w.clone();
    off.apply(Command::SetTrainingSupply(false)).unwrap();
    assert_eq!(off.bench_stock(), 0, "turning support off hands them back");

    w.apply(Command::SetProductionEnabled(true)).unwrap();
    let stock = w.bench_stock();
    w.run_ticks(1500);
    assert!(w.bench_stock() >= stock, "the woodworker left them alone");
    assert!(w.ledger.crafted.get(&planks(&w)).copied().unwrap_or(0) > 0);
    w.check_conservation().unwrap();
}

#[test]
fn the_player_saws_from_the_bench_and_earns_all_the_xp() {
    let mut w = settlement();
    w.apply(Command::SetTrainingSupply(true)).unwrap();
    assert!(run_until(&mut w, 1500, |w| w.bench_stock() >= 10));
    let skill = ww(&w);
    let station = w.station().unwrap().pos;
    let spirit_xp: Vec<u64> = w.spirits.iter().map(|s| s.xp()).collect();
    w.apply(Command::PlayerInteract(station)).unwrap();
    assert!(run_until(&mut w, 900, |w| w.player.xp(skill) >= 50));
    assert_eq!(
        w.player.actor.inventory.total(),
        0,
        "planks stay for a hauler"
    );
    assert_eq!(w.ledger.xp_earned, w.player.xp(skill));
    let now: Vec<u64> = w.spirits.iter().map(|s| s.xp()).collect();
    assert_eq!(now, spirit_xp);
    w.check_conservation().unwrap();
}

#[test]
fn no_spirit_action_earns_the_player_xp() {
    let mut w = World::from_scenario(ScenarioKind::WorkshopAutomated);
    w.apply(Command::SetTrainingSupply(true)).unwrap();
    w.apply(Command::SetTrainingCollect(true)).unwrap();
    w.run_ticks(4000);
    assert_eq!(w.ledger.xp_earned, 0);
    assert_eq!(w.player.xp(wc(&w)) + w.player.xp(ww(&w)), 0);
}

#[test]
fn production_comes_first_unless_the_player_says_otherwise() {
    let ticks_to_first_delivery = |training_first: bool| {
        let mut w = World::from_scenario(ScenarioKind::WorkshopAutomated);
        w.apply(Command::SetProductionTarget(200)).unwrap();
        w.apply(Command::SetTrainingSupply(true)).unwrap();
        w.apply(Command::SetTrainingFirst(training_first)).unwrap();
        let mut ticks = 0;
        while w.bench_stock() == 0 && ticks < 3000 {
            w.step();
            ticks += 1;
        }
        ticks
    };
    assert!(ticks_to_first_delivery(true) < ticks_to_first_delivery(false));
}

#[test]
fn production_keeps_running_during_personal_retraining() {
    let mut w = World::from_scenario(ScenarioKind::WorkshopAutomated);
    w.apply(Command::SetProductionTarget(60)).unwrap();
    w.apply(Command::SetTrainingCollect(true)).unwrap();
    let mut bot = Bot::new(&w);
    bot.chop_until(&mut w, 3000, |_| false);
    assert!(w.player.xp(wc(&w)) > 0);
    let made = w.ledger.crafted.get(&planks(&w)).copied().unwrap_or(0);
    assert!(made >= 30, "{made}");
    w.check_conservation().unwrap();
}

// --- Feedback and guidance --------------------------------------------------

#[test]
fn milestones_are_announced_in_plain_words() {
    let mut w = World::new_journey("development").unwrap();
    let mut bot = Bot::new(&w);
    assert!(bot.run(&mut w, Goal::ChopTo(520), 20_000));
    assert!(
        w.notices
            .contains("Woodcutting level increased. Oak trees are now available.")
    );
    w.soul_separate(wc(&w), 388).unwrap();
    assert!(w.notices.contains("Your Woodcutting Spirit can cut Tree, Oak."));
}

#[test]
fn a_woodworking_spirit_announces_its_best_recipe() {
    let mut w = World::from_scenario(ScenarioKind::WorkshopManual);
    let mut bot = Bot::new(&w);
    assert!(bot.run(&mut w, Goal::SawTo(420), 20_000));
    bot.stop(&mut w);
    w.soul_separate(ww(&w), 400).unwrap();
    assert!(
        w.notices
            .contains("Your Woodworking Spirit can produce Precise Planks.")
    );
}

#[test]
fn scenarios_have_no_guidance_and_journeys_start_at_the_beginning() {
    assert!(
        World::from_scenario(ScenarioKind::NewGame)
            .guidance()
            .is_none()
    );
    let w = World::new_journey("normal").unwrap();
    assert_eq!(w.journey.as_ref().unwrap().phase, Phase::Beginnings);
    let g = w.guidance().unwrap();
    assert!(g.title.contains("Woodcutting level 3"));
    assert!(g.hint.contains("Click a tree"));
    assert_eq!(w.main().storage.total(), 0, "minimal resources");
    assert_eq!(w.player.actor.inventory.total(), 0);
}

#[test]
fn guidance_moves_past_stages_done_out_of_order() {
    let mut w = World::new_journey("development").unwrap();
    let mut bot = Bot::new(&w);
    // Woodworking first: nobody told the player to wait.
    assert!(bot.run(&mut w, Goal::SawTo(400), 30_000));
    assert!(bot.run(&mut w, Goal::ChopTo(174), 30_000));
    w.run_ticks(2);
    assert_eq!(w.journey.as_ref().unwrap().phase, Phase::FirstSpirit);
    w.soul_separate(wc(&w), 100).unwrap();
    w.run_ticks(2);
    assert_eq!(
        w.journey.as_ref().unwrap().phase,
        Phase::Automation,
        "Woodworking was already trained"
    );
}

// --- The whole cycle --------------------------------------------------------

fn at(r: &PacingReport, m: Milestone) -> u64 {
    r.milestones.iter().find(|(x, _)| *x == m).unwrap().1
}

#[test]
fn the_first_cycle_completes_under_both_profiles() {
    for profile in ["development", "normal"] {
        let (w, r) = play_first_cycle(profile).unwrap();
        w.check_conservation().unwrap();
        assert_eq!(r.final_phase, Phase::Settled, "{profile}");
        let times: Vec<u64> = [
            Milestone::FirstLevel,
            Milestone::FirstUnlock,
            Milestone::FirstSpirit,
            Milestone::CraftSpirit,
            Milestone::FirstImprovement,
            Milestone::LevelRegained,
        ]
        .into_iter()
        .map(|m| at(&r, m))
        .collect();
        assert!(
            times.windows(2).all(|t| t[0] <= t[1]),
            "{profile}: {times:?}"
        );
        let gatherers = w
            .spirits
            .iter()
            .filter(|s| w.defs.role(s.role).has_job(JobKind::Gather))
            .count();
        assert_eq!(gatherers, 1);
        assert_eq!(
            w.player.xp_invested(),
            r.woodcutting_sacrificed + r.woodworking_sacrificed
        );
    }
}

#[test]
fn normal_is_much_slower_than_development_where_it_matters() {
    let (_, dev) = play_first_cycle("development").unwrap();
    let (_, normal) = play_first_cycle("normal").unwrap();
    assert!(at(&normal, Milestone::FirstUnlock) >= at(&dev, Milestone::FirstUnlock) * 3);
    assert!(
        at(&normal, Milestone::FirstLevel) < 60,
        "the first level stays quick"
    );
    assert!(normal.total_seconds >= 20 * 60, "{}", normal.total_seconds);
}

#[test]
fn support_saves_clicks_and_walking_on_the_way_back() {
    let (_, r) = play_first_cycle("normal").unwrap();
    assert!(r.resaw_supported.actions < r.resaw_alone.actions / 2);
    assert!(r.resaw_supported.storage_trips < r.resaw_alone.storage_trips);
    assert!(r.retrain_supported.player.traveling < r.retrain_alone.player.traveling);
}

#[test]
fn a_playthrough_interrupted_by_saves_ends_the_same() {
    let mut plain = World::new_journey("development").unwrap();
    let mut bot_a = Bot::new(&plain);
    let mut resumed = plain.clone();
    let mut bot_b = bot_a.clone();
    for _ in 0..40 {
        for _ in 0..100 {
            bot_a.play(&mut plain, Goal::Chop);
            plain.step();
            bot_b.play(&mut resumed, Goal::Chop);
            resumed.step();
        }
        resumed = save::decode(&save::encode(&resumed, "new_game")).unwrap().0;
    }
    assert_eq!(resumed.state_text(), plain.state_text());
}
