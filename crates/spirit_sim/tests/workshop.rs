//! v0.4: Woodworking, the workshop, multi-resource logistics, production
//! targets with upstream demand, and planks in construction.

use spirit_sim::defs::{JobKind, RecipeId, ResourceId, SkillId};
use spirit_sim::entities::{Activity, ProjectState};
use spirit_sim::events::EventKind;
use spirit_sim::grid::{BUILTIN_MAP, MapLayout};
use spirit_sim::metrics::WaitKind;
use spirit_sim::production::{ProductionBottleneck, WorkshopStatus};
use spirit_sim::scenario::{BUILTIN_SCENARIOS, ScenarioDef, load_scenarios};
use spirit_sim::tasks::Task;
use spirit_sim::{Command, GameDefs, ScenarioKind, World};

fn logs(w: &World) -> ResourceId {
    w.defs.resource_id("logs").unwrap()
}

fn planks(w: &World) -> ResourceId {
    w.defs.resource_id("planks").unwrap()
}

fn woodworking(w: &World) -> SkillId {
    w.defs.skill_id("woodworking").unwrap()
}

fn recipe(w: &World, id: &str) -> RecipeId {
    let i = w.defs.recipes.iter().position(|r| r.id == id).unwrap();
    RecipeId(i as u16)
}

fn batches(w: &World, id: &str) -> u64 {
    let r = recipe(w, id);
    w.ledger.batches.get(&r).copied().unwrap_or(0)
}

fn world_with(key: &str, edit: impl FnOnce(&mut ScenarioDef)) -> World {
    let mut scenarios = load_scenarios(BUILTIN_SCENARIOS).unwrap();
    let mut scenario = scenarios.remove(key).unwrap();
    edit(&mut scenario);
    let map = MapLayout::parse(BUILTIN_MAP).unwrap();
    World::build(GameDefs::builtin(), &map, &scenario).unwrap()
}

/// Steps until `done`, checking conservation throughout. Panics on timeout.
fn run_until(w: &mut World, max_ticks: u64, done: impl Fn(&World) -> bool) {
    for _ in 0..max_ticks {
        if done(w) {
            return;
        }
        w.step();
        w.check_conservation().unwrap();
    }
    panic!(
        "condition not reached in {max_ticks} ticks (tick {})",
        w.tick
    );
}

fn run_checked(w: &mut World, ticks: u64) {
    for _ in 0..ticks {
        w.step();
        w.check_conservation().unwrap();
    }
}

fn idle(w: &World) -> bool {
    w.player.actor.queue.is_empty()
}

/// The player walks to Main Storage and takes logs.
fn take_logs(w: &mut World) {
    let main = w.main().pos;
    w.apply(Command::PlayerInteract(main)).unwrap();
    run_until(w, 500, idle);
}

/// The player walks to the Workshop and works until out of materials.
fn saw_everything(w: &mut World) {
    let bench = w.station().unwrap().pos;
    w.apply(Command::PlayerInteract(bench)).unwrap();
    run_until(w, 5000, idle);
}

fn woodworker(w: &World) -> usize {
    w.spirits
        .iter()
        .position(|s| w.defs.role(s.role).has_job(JobKind::Craft))
        .expect("a woodworking spirit")
}

// --------------------------------------------------------------------------
// Player Woodworking
// --------------------------------------------------------------------------

#[test]
fn the_player_turns_logs_into_planks_and_earns_xp() {
    let mut w = world_with("workshop_manual", |s| {
        s.player_xp.clear();
    });
    let skill = woodworking(&w);
    take_logs(&mut w);
    assert_eq!(w.player.actor.inventory.get(logs(&w)), 28);
    assert_eq!(w.main().storage.get(logs(&w)), 80 - 28);

    saw_everything(&mut w);

    let bag = &w.player.actor.inventory;
    assert!(bag.get(planks(&w)) > 0);
    let rough = batches(&w, "rough_planks");
    let precise = batches(&w, "precise_planks");
    assert!(rough > 0);
    assert_eq!(
        w.ledger.processed[&logs(&w)],
        rough * 2 + precise * 3,
        "logs consumed must match the batches"
    );
    let xp_rough = w.defs.recipe(recipe(&w, "rough_planks")).xp;
    let xp_precise = w.defs.recipe(recipe(&w, "precise_planks")).xp;
    assert_eq!(w.player.xp(skill), rough * xp_rough + precise * xp_precise);
    assert_eq!(w.ledger.xp_earned, w.player.xp(skill));
    assert!(w.player.level(skill, &w.defs.xp_table) > 1);
}

#[test]
fn processing_repeats_until_materials_run_out_and_needs_materials() {
    let mut w = World::from_scenario(ScenarioKind::WorkshopManual);
    let bench = w.station().unwrap().pos;
    let err = w.apply(Command::PlayerInteract(bench)).unwrap_err();
    assert!(err.contains("You need 2 Logs"), "{err}");

    take_logs(&mut w);
    saw_everything(&mut w);
    assert!(
        w.player.actor.inventory.get(logs(&w)) < 3,
        "one click uses all the logs it can"
    );
    assert!(w.player.actor.inventory.get(planks(&w)) >= 10);
    // No materials, so nothing more can be made however long we wait.
    let made = w.ledger.crafted[&planks(&w)];
    run_checked(&mut w, 500);
    assert_eq!(w.ledger.crafted[&planks(&w)], made);
}

#[test]
fn planks_are_stored_by_clicking_main_storage() {
    let mut w = World::from_scenario(ScenarioKind::WorkshopManual);
    take_logs(&mut w);
    saw_everything(&mut w);
    let bag = w.player.actor.inventory.get(planks(&w));
    let main = w.main().pos;
    w.apply(Command::PlayerInteract(main)).unwrap();
    run_until(&mut w, 500, idle);
    assert_eq!(w.player.actor.inventory.total(), 0);
    assert_eq!(w.main().storage.get(planks(&w)), bag);
}

#[test]
fn recipes_are_gated_by_level_and_the_better_one_is_used_when_unlocked() {
    // Level 4 with 12 XP to go: the first batches are rough, then precise.
    let mut w = World::from_scenario(ScenarioKind::WorkshopManual);
    let skill = woodworking(&w);
    assert_eq!(w.player.level(skill, &w.defs.xp_table), 4);
    take_logs(&mut w);
    saw_everything(&mut w);
    assert!(batches(&w, "rough_planks") >= 1);
    assert!(batches(&w, "precise_planks") >= 1);
    assert!(w.player.level(skill, &w.defs.xp_table) >= 5);
    assert!(
        w.events
            .contains(EventKind::Player, "You can now make Precise Planks")
    );

    // A level 1 worker never gets the precise recipe.
    let mut low = world_with("workshop_manual", |s| {
        s.player_xp.clear();
        s.main_storage.insert("logs".into(), 6);
    });
    take_logs(&mut low);
    saw_everything(&mut low);
    assert_eq!(batches(&low, "precise_planks"), 0);
    assert_eq!(batches(&low, "rough_planks"), 3);
}

#[test]
fn a_full_bag_cannot_lose_output() {
    // Output replaces inputs in the bag, so conservation holds at capacity.
    let mut w = World::from_scenario(ScenarioKind::WorkshopManual);
    take_logs(&mut w);
    assert!(w.player.actor.inventory.is_full());
    saw_everything(&mut w);
    assert!(w.player.actor.inventory.total() <= w.player.actor.inventory.capacity);
    w.check_conservation().unwrap();
}

// --------------------------------------------------------------------------
// Soul Separation
// --------------------------------------------------------------------------

#[test]
fn woodworking_soul_separation_transfers_exact_xp_and_explains_the_cost() {
    let mut w = world_with("workshop_manual", |s| {
        s.player_xp.insert("woodworking".into(), 500);
    });
    let skill = woodworking(&w);
    let precise = recipe(&w, "precise_planks");
    let rough = recipe(&w, "rough_planks");

    let p = w.soul_preview(skill, 400);
    assert_eq!(p.error, None);
    assert_eq!((p.current_level, p.after_level, p.spirit_level), (5, 2, 5));
    assert_eq!(p.lost_recipes, vec![precise]);
    assert_eq!(p.spirit_recipes, vec![rough, precise]);

    let id = w.soul_separate(skill, 400).unwrap();
    assert_eq!(w.player.xp(skill), 100);
    assert_eq!(w.player.level(skill, &w.defs.xp_table), 2);
    assert_eq!(w.spirits[id].xp(), 400);
    assert_eq!(w.spirits[id].level(), 5);
    assert_eq!(w.spirits[id].skill(), skill);
    assert_eq!(w.defs.role(w.spirits[id].role).id, "woodworker");
    let h = w.player.history(skill);
    assert_eq!(
        (h.highest_level, h.xp_sacrificed, h.spirits_created),
        (5, 400, 1)
    );
    assert!(
        w.events
            .contains(EventKind::Player, "no longer make Precise Planks")
    );
}

#[test]
fn the_player_loses_a_recipe_that_the_spirit_keeps() {
    let mut w = world_with("workshop_manual", |s| {
        s.player_xp.insert("woodworking".into(), 500);
        s.main_storage.insert("logs".into(), 6);
    });
    let skill = woodworking(&w);
    let id = w.soul_separate(skill, 400).unwrap();
    w.apply(Command::SetSpiritAssigned {
        spirit: id,
        assigned: false,
    })
    .unwrap();
    take_logs(&mut w);
    saw_everything(&mut w);
    assert_eq!(batches(&w, "rough_planks"), 3);
    assert_eq!(batches(&w, "precise_planks"), 0, "given away with the XP");
    assert_eq!(w.spirits[id].level(), 5);

    let mut spirit_only = world_with("workshop_manual", |s| {
        s.player_xp.insert("woodworking".into(), 500);
        s.main_storage.insert("logs".into(), 9);
        s.log_reserve = 0;
    });
    spirit_only.soul_separate(skill, 400).unwrap();
    run_until(&mut spirit_only, 3000, |w| {
        batches(w, "precise_planks") == 3
    });
    assert_eq!(batches(&spirit_only, "rough_planks"), 0);
}

#[test]
fn spirits_never_gain_xp_and_keep_their_competency() {
    let mut w = World::from_scenario(ScenarioKind::WorkshopAutomated);
    let id = woodworker(&w);
    let xp = w.spirits[id].xp();
    let earned = w.ledger.xp_earned;
    run_until(&mut w, 6000, |w| w.spirits[id].units_crafted >= 10);
    assert_eq!(w.spirits[id].xp(), xp);
    assert_eq!(w.spirits[id].level(), 5);
    assert_eq!(w.ledger.xp_earned, earned, "spirit work gives nobody XP");
    assert!(batches(&w, "precise_planks") > 0);
    assert_eq!(
        batches(&w, "rough_planks"),
        0,
        "level 5 uses the better recipe"
    );
}

#[test]
fn a_low_level_spirit_is_held_to_the_recipes_its_level_allows() {
    let mut w = world_with("workshop_automated", |s| {
        for seed in &mut s.spirits {
            if seed.role == "woodworker" {
                seed.xp = 100; // Level 2.
            }
        }
    });
    run_until(&mut w, 6000, |w| {
        w.ledger.crafted.get(&ResourceId(1)).copied().unwrap_or(0) >= 5
    });
    assert_eq!(batches(&w, "precise_planks"), 0);
}

#[test]
fn normal_play_has_no_free_woodworking_spirits() {
    for kind in [ScenarioKind::NewGame, ScenarioKind::WorkshopManual] {
        let mut w = World::from_scenario(kind);
        run_checked(&mut w, 3000);
        assert!(w.crafters().next().is_none());
    }
}

#[test]
fn a_separated_spirit_goes_to_work_on_its_own() {
    // Scenario A end to end: train, sacrifice, then step back.
    let mut w = world_with("workshop_manual", |s| {
        s.player_xp.insert("woodworking".into(), 500);
    });
    let skill = woodworking(&w);
    let id = w.soul_separate(skill, 400).unwrap();
    run_until(&mut w, 8000, |w| w.main().storage.get(ResourceId(1)) >= 10);
    assert!(w.spirits[id].units_crafted >= 10);
    assert_eq!(
        w.player.xp(skill),
        100,
        "the player earned nothing by watching"
    );
}

// --------------------------------------------------------------------------
// Logistics
// --------------------------------------------------------------------------

#[test]
fn haulers_physically_carry_logs_in_and_planks_out() {
    let mut w = World::from_scenario(ScenarioKind::WorkshopAutomated);
    let hauler = w
        .spirits
        .iter()
        .position(|s| w.defs.role(s.role).has_job(JobKind::Supply))
        .unwrap();
    let (l, p) = (logs(&w), planks(&w));
    let (mut carried_logs, mut carried_planks) = (false, false);
    for _ in 0..3000 {
        w.step();
        w.check_conservation().unwrap();
        let bag = &w.spirits[hauler].actor.inventory;
        carried_logs |= bag.get(l) > 0;
        carried_planks |= bag.get(p) > 0;
        assert!(bag.total() <= bag.capacity);
    }
    assert!(carried_logs && carried_planks);
    assert!(w.main().storage.get(p) > 0);
}

#[test]
fn the_workshop_input_buffer_and_output_limit_hold() {
    let mut w = world_with("workshop_automated", |s| {
        s.plank_target = 500;
        s.main_storage.insert("logs".into(), 250);
        s.log_reserve = 0;
    });
    let (l, p) = (logs(&w), planks(&w));
    let limits = w.defs.building(w.station().unwrap().kind).clone();
    for _ in 0..4000 {
        w.step();
        w.check_conservation().unwrap();
        let b = w.station().unwrap();
        assert!(b.storage.get(l) <= limits.input_buffer);
        assert!(b.storage.get(p) <= limits.capacity - limits.input_buffer);
        assert!(b.storage.total() <= b.storage.capacity);
    }
}

/// Puts planks straight into the workshop, recording them as seeded.
fn fill_workshop_output(w: &mut World) {
    let p = planks(w);
    let id = w.station().unwrap().id;
    let def = w.defs.building(w.buildings[id].kind);
    let limit = def.capacity - def.input_buffer;
    let stored = w.buildings[id].storage.deposit(p, limit);
    assert_eq!(stored, limit);
    *w.ledger.seeded.entry(p).or_default() += stored as u64;
}

#[test]
fn a_full_output_stops_the_bench_until_it_is_hauled_away() {
    let mut w = world_with("workshop_automated", |s| {
        s.plank_target = 100;
        s.log_reserve = 0;
    });
    fill_workshop_output(&mut w);
    assert_eq!(w.workshop_status(), WorkshopStatus::OutputFull);
    assert_eq!(
        w.production_report().bottleneck,
        ProductionBottleneck::Hauling
    );
    let id = woodworker(&w);
    run_checked(&mut w, 3);
    assert_eq!(w.spirits[id].units_crafted, 0, "no room, so no work");
    let p = planks(&w);
    run_until(&mut w, 2000, |w| w.main().storage.get(p) > 0);
    run_until(&mut w, 3000, |w| w.spirits[id].units_crafted > 0);
}

#[test]
fn full_main_storage_blocks_haulers_without_losing_anything() {
    let mut w = world_with("workshop_automated", |s| {
        s.plank_target = 100;
        s.log_reserve = 0;
    });
    fill_workshop_output(&mut w);
    let main = w.main_storage;
    w.buildings[main].storage.capacity = w.main().storage.total();
    assert_eq!(w.main().storage.free(), 0);
    let report = w.production_report();
    assert_eq!(report.bottleneck, ProductionBottleneck::Storage);
    assert!(report.summary.contains("Storehouse"), "{}", report.summary);

    // A spirit may free space by taking logs out, so hold them back too.
    w.apply(Command::SetProductionEnabled(false)).unwrap();
    let p = planks(&w);
    run_checked(&mut w, 300);
    assert_eq!(w.main().storage.get(p), 0);

    w.buildings[main].storage.capacity += 50;
    run_until(&mut w, 2000, |w| w.main().storage.get(p) > 0);
}

#[test]
fn a_worker_without_a_hauler_fetches_and_stores_for_itself() {
    let mut w = world_with("workshop_automated", |s| {
        s.spirits
            .retain(|x| x.role == "woodworker" || x.role == "woodcutter");
        s.plank_target = 12;
    });
    assert!(
        w.spirits
            .iter()
            .all(|s| { !w.defs.role(s.role).has_job(JobKind::Supply) })
    );
    let p = planks(&w);
    run_until(&mut w, 12_000, |w| w.main().storage.get(p) >= 12);
    assert_eq!(w.station().unwrap().storage.get(p), 0);
}

#[test]
fn nothing_is_requested_with_no_worker_at_the_bench() {
    let mut w = World::from_scenario(ScenarioKind::NewGame);
    let l = logs(&w);
    assert_eq!(w.production_deficit(), 0);
    assert_eq!(w.upstream_demand(l), 0);
    assert_eq!(w.gather_goal(), w.settlement.objective.target);
    run_checked(&mut w, 1000);
    assert_eq!(
        w.station().unwrap().storage.total(),
        0,
        "no logs sent to an empty bench"
    );
}

// --------------------------------------------------------------------------
// Targets and upstream demand
// --------------------------------------------------------------------------

#[test]
fn a_plank_target_is_reached_and_then_production_stops() {
    let mut w = World::from_scenario(ScenarioKind::WorkshopAutomated);
    let p = planks(&w);
    run_until(&mut w, 12_000, |w| w.main().storage.get(p) >= 30);
    run_checked(&mut w, 1500);
    let made = w.ledger.crafted[&p];
    run_checked(&mut w, 1500);
    assert_eq!(w.ledger.crafted[&p], made, "production stops at the target");
    assert_eq!(w.main().storage.get(p), 30, "and doesn't overshoot");
    assert_eq!(w.workshop_status(), WorkshopStatus::TargetMet);
    assert_eq!(w.production_report().bottleneck, ProductionBottleneck::None);
    assert_eq!(
        w.main().storage.reserved(logs(&w)),
        0,
        "no reservation left behind"
    );
}

#[test]
fn a_shortfall_creates_bounded_upstream_log_demand() {
    let mut w = world_with("workshop_automated", |s| {
        s.plank_target = 1000;
        s.objective_target = 20;
    });
    let l = logs(&w);
    let cap = w.defs.production.upstream_cap;
    w.step();
    assert_eq!(w.upstream_demand(l), cap);
    assert_eq!(w.gather_goal(), 20 + cap);
    assert!(w.objective_unmet(), "gatherers work toward the raised goal");

    w.apply(Command::SetProductionTarget(0)).unwrap();
    assert_eq!(w.upstream_demand(l), 0);
    assert_eq!(w.gather_goal(), 20);
}

#[test]
fn gatherers_keep_working_only_while_production_needs_logs() {
    let mut w = world_with("workshop_automated", |s| {
        s.main_storage.insert("logs".into(), 0);
        s.plank_target = 10;
    });
    let (l, p) = (logs(&w), planks(&w));
    run_until(&mut w, 20_000, |w| w.main().storage.get(p) >= 10);
    let harvested = w.ledger.harvested[&l];
    // Enough logs for the planks and the log goal, and not many more.
    assert!(harvested <= 20 + 15 + 25, "harvested {harvested}");
    run_checked(&mut w, 1500);
    let settled = w.ledger.harvested[&l];
    run_checked(&mut w, 1500);
    assert_eq!(w.ledger.harvested[&l], settled, "gatherers stop too");
}

#[test]
fn the_log_reserve_is_left_alone() {
    let mut w = world_with("workshop_automated", |s| {
        s.spirits.retain(|x| x.role != "woodcutter");
        s.main_storage.insert("logs".into(), 50);
        s.log_reserve = 30;
        s.plank_target = 100;
    });
    let l = logs(&w);
    run_checked(&mut w, 4000);
    assert_eq!(w.main().storage.get(l), 30);
    let r = w.production_report();
    assert_eq!(r.status, WorkshopStatus::WaitingForInputs);
    assert_eq!(r.bottleneck, ProductionBottleneck::InputReserve);
    assert!(r.summary.contains("reserve"), "{}", r.summary);

    // Lowering the reserve frees exactly the difference.
    w.apply(Command::SetInputReserve(10)).unwrap();
    run_checked(&mut w, 4000);
    assert_eq!(w.main().storage.get(l), 10);
}

#[test]
fn production_can_be_disabled_and_workers_unassigned() {
    let mut w = World::from_scenario(ScenarioKind::WorkshopAutomated);
    let p = planks(&w);
    let id = woodworker(&w);
    run_until(&mut w, 3000, |w| w.spirits[id].units_crafted > 0);

    w.apply(Command::SetProductionEnabled(false)).unwrap();
    assert_eq!(w.workshop_status(), WorkshopStatus::Disabled);
    assert_eq!(w.gather_goal(), w.settlement.objective.target);
    run_checked(&mut w, 300);
    let made = w.ledger.crafted[&p];
    run_checked(&mut w, 600);
    assert_eq!(w.ledger.crafted[&p], made);
    assert!(w.spirits[id].activity.text().contains("disabled"));

    w.apply(Command::SetProductionEnabled(true)).unwrap();
    w.apply(Command::SetSpiritAssigned {
        spirit: id,
        assigned: false,
    })
    .unwrap();
    assert_eq!(w.workshop_status(), WorkshopStatus::NoWorker);
    run_checked(&mut w, 300);
    assert!(w.spirits[id].activity.text().contains("not assigned"));

    w.apply(Command::SetSpiritAssigned {
        spirit: id,
        assigned: true,
    })
    .unwrap();
    run_until(&mut w, 3000, |w| w.ledger.crafted[&p] > made);
}

#[test]
fn workers_wait_for_missing_materials_and_recover() {
    let mut w = world_with("workshop_automated", |s| {
        s.main_storage.insert("logs".into(), 0);
        s.log_reserve = 0;
        s.closed_sites = vec!["starting_grove".into(), "distant_forest".into()];
    });
    let id = woodworker(&w);
    run_checked(&mut w, 300);
    assert_eq!(w.spirits[id].wait, Some(WaitKind::Inputs));
    assert!(matches!(w.spirits[id].activity, Activity::Blocked(_)));
    assert!(
        w.spirits[id].activity.text().contains("insufficient logs"),
        "{}",
        w.spirits[id].activity.text()
    );
    let r = w.production_report();
    assert_eq!(r.status, WorkshopStatus::WaitingForInputs);
    assert_eq!(r.status.label(), "Workshop waiting: insufficient logs");
    assert_eq!(r.bottleneck, ProductionBottleneck::LogSupply);
    assert!(!w.ledger.crafted.contains_key(&planks(&w)));

    let grove = w.defs.site_id("starting_grove").unwrap();
    w.apply(Command::SetSiteOpen {
        site: grove,
        open: true,
    })
    .unwrap();
    run_until(&mut w, 6000, |w| w.spirits[id].units_crafted > 0);
}

#[test]
fn hauling_shortages_are_identified() {
    // Plenty of logs in Main Storage but no way to move them fast: at the
    // moment the bench runs dry the report points at hauling, not at supply.
    let mut w = world_with("workshop_automated", |s| {
        s.spirits.retain(|x| x.role != "woodcutter");
        s.main_storage.insert("logs".into(), 200);
        s.plank_target = 200;
        s.log_reserve = 0;
    });
    run_until(&mut w, 5000, |w| {
        w.workshop_status() == WorkshopStatus::WaitingForInputs
    });
    assert_eq!(
        w.production_report().bottleneck,
        ProductionBottleneck::Hauling
    );
}

// --------------------------------------------------------------------------
// Construction with planks
// --------------------------------------------------------------------------

#[test]
fn a_storehouse_consumes_planks_and_expands_main_storage() {
    let mut w = World::from_scenario(ScenarioKind::WorkshopConstruction);
    let (l, p) = (logs(&w), planks(&w));
    assert_eq!(w.projects.len(), 1);
    assert_eq!(w.projects[0].state, ProjectState::AwaitingMaterials);
    assert_eq!(w.production_goal(), 5 + 20, "the project adds to the goal");

    run_until(&mut w, 40_000, |w| !w.projects[0].is_active());

    let project = &w.projects[0];
    assert_eq!(project.consumed[&p], 20);
    assert_eq!(project.consumed[&l], 10);
    assert_eq!(w.ledger.consumed[&p], 20);
    assert!(w.buildings[project.building].is_complete());
    assert_eq!(w.main().storage.capacity, 450);
    assert!(
        !w.grid.is_walkable(w.buildings[project.building].pos),
        "the building physically occupies its tile"
    );
    assert_eq!(w.main().storage.reserved(p), 0);

    // Production then settles on the plain target.
    run_until(&mut w, 6000, |w| w.main().storage.get(p) >= 5);
    assert_eq!(w.production_goal(), 5);
}

#[test]
fn reserved_planks_are_not_spent_twice() {
    let mut w = world_with("workshop_construction", |s| {
        s.main_storage.insert("planks".into(), 20);
        s.main_storage.insert("logs".into(), 40);
    });
    let (l, p) = (logs(&w), planks(&w));
    run_until(&mut w, 10, |w| w.projects[0].reserved);
    assert_eq!(w.main().storage.reserved(p), 20);
    assert_eq!(w.main().storage.available(p), 0);
    assert_eq!(w.main().storage.reserved(l), 10);
    assert_eq!(
        w.projected_stock(p),
        0,
        "reserved planks don't count toward the target"
    );
    let main = w.main_storage;
    assert_eq!(w.buildings[main].storage.withdraw_available(p, 5), 0);
    run_until(&mut w, 20_000, |w| !w.projects[0].is_active());
    assert_eq!(w.ledger.consumed[&p], 20);
}

#[test]
fn reserved_logs_cannot_be_taken_by_hauler_or_player() {
    let mut w = world_with("workshop_construction", |s| {
        s.spirits.retain(|x| x.role != "woodcutter");
        s.main_storage.insert("planks".into(), 20);
        s.main_storage.insert("logs".into(), 10);
        s.log_reserve = 0;
    });
    let l = logs(&w);
    run_until(&mut w, 10, |w| w.projects[0].reserved);
    assert_eq!(w.main().storage.available(l), 0);
    // Empty-handed click: with nothing available it doesn't take logs.
    let main = w.main().pos;
    w.apply(Command::PlayerInteract(main)).unwrap();
    run_until(&mut w, 200, idle);
    assert_eq!(w.player.actor.inventory.get(l), 0);
    run_until(&mut w, 20_000, |w| !w.projects[0].is_active());
    assert_eq!(w.ledger.consumed[&l], 10);
}

#[test]
fn buildings_are_ordered_only_within_their_limits() {
    let mut w = World::from_scenario(ScenarioKind::WorkshopAutomated);
    let storehouse = w.defs.building_kind_id("storehouse").unwrap();
    let field = w.defs.building_kind_id("field_storage").unwrap();
    assert!(w.apply(Command::RequestBuilding(field)).is_err());
    w.apply(Command::RequestBuilding(storehouse)).unwrap();
    let err = w.apply(Command::RequestBuilding(storehouse)).unwrap_err();
    assert!(err.contains("Finish the current"), "{err}");
    let site = w.projects[0].site;
    assert!(
        (site.x - w.main().pos.x).abs() + (site.y - w.main().pos.y).abs() >= 2,
        "workers can still reach Main Storage"
    );
}

// --------------------------------------------------------------------------
// Whole-chain checks
// --------------------------------------------------------------------------

#[test]
fn no_spirit_is_left_holding_a_task_it_cannot_finish() {
    // Long unattended run: everything keeps moving or settles, with no
    // repeated failures in the log.
    let mut w = World::from_scenario(ScenarioKind::WorkshopAutomated);
    run_checked(&mut w, 30_000);
    let failures = w
        .events
        .iter()
        .filter(|e| e.text.contains("task failed"))
        .count();
    assert_eq!(failures, 0);
    let in_queue = w.spirits.iter().any(|s| {
        s.actor
            .queue
            .iter()
            .any(|t| matches!(t, Task::Craft { .. }))
    });
    assert!(!in_queue, "workers are idle once the target is met");
}

#[test]
fn the_chain_replays_exactly_with_the_same_commands() {
    let run = || {
        let mut w = World::from_scenario(ScenarioKind::WorkshopConstruction);
        run_checked(&mut w, 1500);
        w.apply(Command::SetProductionTarget(25)).unwrap();
        w.apply(Command::SetInputReserve(5)).unwrap();
        run_checked(&mut w, 3000);
        w.state_hash()
    };
    assert_eq!(run(), run());
}

#[test]
fn the_field_storage_demo_is_unaffected_by_the_workshop() {
    let mut w = World::from_scenario(ScenarioKind::Demo);
    run_until(&mut w, 5000, |w| {
        w.events
            .contains(EventKind::Planner, "Decision: build a Field Storage")
    });
    let decided = w.tick;
    assert!(
        (1400..=1800).contains(&decided),
        "decided at tick {decided}"
    );
    run_until(&mut w, 3000, |w| {
        w.projects.first().is_some_and(|p| !p.is_active())
    });
    assert_eq!(w.station().unwrap().storage.total(), 0);
    assert_eq!(w.ledger.crafted.len(), 0);
}
