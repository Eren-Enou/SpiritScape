//! v0.8: a remote production site at Old Growth, multi-facility logistics,
//! construction demand, training at any bench, and that it all keeps working
//! across saves.

use spirit_sim::autoplay::{compare_layouts, establish_outpost};
use spirit_sim::defs::{JobKind, RecipeId, ResourceId, SiteId};
use spirit_sim::entities::TreeState;
use spirit_sim::grid::TilePos;
use spirit_sim::outpost::WorkerState;
use spirit_sim::save::{SAVE_VERSION, SaveStore, Slot, decode, encode};
use spirit_sim::tasks::Task;
use spirit_sim::training::TrainingMode;
use spirit_sim::{Command, ScenarioKind, World};

const STORE_AT: TilePos = TilePos { x: 34, y: 4 };
const BENCH_AT: TilePos = TilePos { x: 36, y: 4 };

fn central() -> World {
    World::from_scenario(ScenarioKind::CentralProduction)
}

fn remote() -> World {
    World::from_scenario(ScenarioKind::RemoteProduction)
}

fn establishing() -> World {
    World::from_scenario(ScenarioKind::OutpostEstablishment)
}

fn res(w: &World, id: &str) -> ResourceId {
    w.defs.resource_id(id).unwrap()
}

fn old_growth(w: &World) -> SiteId {
    w.defs.site_id("old_growth").unwrap()
}

fn recipe(w: &World, id: &str) -> RecipeId {
    RecipeId(w.defs.recipes.iter().position(|r| r.id == id).unwrap() as u16)
}

fn id_at(w: &World, p: TilePos) -> usize {
    w.building_at(p).unwrap().id
}

fn run_until(w: &mut World, max: u64, done: impl Fn(&World) -> bool) -> bool {
    for _ in 0..max {
        if done(w) {
            return true;
        }
        w.step();
        w.check_conservation().unwrap();
    }
    done(w)
}

fn order(w: &mut World, building: &str, at: TilePos) {
    let kind = w.defs.building_kind_id(building).unwrap();
    w.apply(Command::RequestBuildingAt { kind, site: at })
        .unwrap();
}

fn beams_home(w: &World) -> u32 {
    w.main().storage.get(res(w, "beams"))
}

fn spirit_of(w: &World, job: JobKind) -> usize {
    w.spirits
        .iter()
        .position(|s| w.defs.role(s.role).has_job(job))
        .unwrap()
}

/// Everything in the world is counted once, however much has moved.
fn no_resource_appears_from_nowhere(w: &World) {
    w.check_conservation().unwrap();
    for b in &w.buildings {
        for i in 0..w.defs.resources.len() {
            let r = ResourceId(i as u16);
            assert!(
                b.storage.reserved(r) <= b.storage.get(r),
                "{} reserves more {} than it holds",
                b.name,
                w.defs.resource(r).name
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Establishing the outpost
// ---------------------------------------------------------------------------

#[test]
fn an_outpost_store_costs_real_materials_and_a_builder_carries_them() {
    let mut w = establishing();
    let logs = res(&w, "logs");
    let planks = res(&w, "planks");
    order(&mut w, "outpost_storage", STORE_AT);
    let site = w.defs.site_at(STORE_AT.x, STORE_AT.y);
    assert_eq!(site, Some(old_growth(&w)));
    let id = id_at(&w, STORE_AT);
    assert!(!w.buildings[id].is_complete());
    assert!(run_until(&mut w, 3000, |w| w.active_project().is_none()));
    assert!(w.buildings[id].is_complete());
    assert_eq!(w.ledger.consumed.get(&logs), Some(&30));
    assert_eq!(w.ledger.consumed.get(&planks), Some(&15));
    let builder = spirit_of(&w, JobKind::Construct);
    assert!(
        w.spirits[builder].time.total.traveling > 0,
        "it walked there"
    );
}

#[test]
fn the_outpost_store_is_a_place_the_player_can_use() {
    let mut w = remote();
    let hardwood = res(&w, "hardwood");
    let beams = res(&w, "beams");
    let id = id_at(&w, STORE_AT);
    // With no order running, nothing else has a claim on the stock.
    w.apply(Command::SetOrderEnabled {
        output: beams,
        enabled: false,
    })
    .unwrap();
    w.buildings[id].storage.deposit(hardwood, 12);
    *w.ledger.seeded.entry(hardwood).or_default() += 12;
    w.apply(Command::PlayerInteract(STORE_AT)).unwrap();
    assert!(run_until(&mut w, 600, |w| w.player.actor.queue.is_empty()));
    assert!(
        w.player.actor.inventory.get(hardwood) > 0,
        "the player took some"
    );
}

#[test]
fn the_report_walks_through_setting_up() {
    let mut w = establishing();
    let site = old_growth(&w);
    assert!(w.outpost_sites().contains(&site));
    assert!(w.outpost_report(site).status.contains("Outpost Storage"));
    order(&mut w, "outpost_storage", STORE_AT);
    assert!(w.outpost_report(site).status.contains("Under construction"));
    run_until(&mut w, 3000, |w| w.active_project().is_none());
    assert!(w.outpost_report(site).status.contains("Workshop"));
    order(&mut w, "workshop", BENCH_AT);
    run_until(&mut w, 12000, |w| w.active_project().is_none());
    assert!(w.outpost_report(site).status.contains("Staff outpost"));
    w.apply(Command::SetOutpostStaffed { site, on: true })
        .unwrap();
    let r = w.outpost_report(site);
    assert!(r.status.contains("Running"), "{}", r.status);
    assert!(r.staffed && r.keep > 0);
}

#[test]
fn staffing_is_one_instruction_and_can_be_undone() {
    let mut w = establishing();
    let site = old_growth(&w);
    order(&mut w, "outpost_storage", STORE_AT);
    run_until(&mut w, 3000, |w| w.active_project().is_none());
    order(&mut w, "workshop", BENCH_AT);
    run_until(&mut w, 12000, |w| w.active_project().is_none());
    let bench = id_at(&w, BENCH_AT);
    w.apply(Command::SetOutpostStaffed { site, on: true })
        .unwrap();
    let posted = w.spirits.iter().filter(|s| s.policy.site == Some(site));
    // The level 10 woodcutter and one of the two haulers. The level 5 can't
    // cut Ironwood, so it stays home gathering logs.
    assert_eq!(posted.count(), 2);
    assert!(w.spirits.iter().any(|s| s.policy.station == Some(bench)));
    w.apply(Command::SetOutpostStaffed { site, on: false })
        .unwrap();
    assert!(w.spirits.iter().all(|s| s.policy.site.is_none()));
    assert!(w.spirits.iter().all(|s| s.policy.station.is_none()));
}

#[test]
fn staffing_warns_when_nobody_is_left_gathering_logs() {
    let mut w = remote();
    let site = old_growth(&w);
    let msg = w
        .apply(Command::SetOutpostStaffed { site, on: true })
        .unwrap();
    assert!(msg.contains("Nobody is left gathering Logs"), "{msg}");
}

#[test]
fn staffing_a_site_nobody_can_work_is_refused() {
    let mut w = World::from_scenario(ScenarioKind::WorkshopAutomated);
    let site = old_growth(&w);
    let err = w.apply(Command::SetOutpostStaffed { site, on: true });
    assert!(err.is_err());
}

// ---------------------------------------------------------------------------
// Central and remote production
// ---------------------------------------------------------------------------

#[test]
fn gatherers_drop_off_at_the_store_beside_the_trees() {
    let mut w = remote();
    let hardwood = res(&w, "hardwood");
    let id = id_at(&w, STORE_AT);
    let found = run_until(&mut w, 3000, |w| w.buildings[id].storage.get(hardwood) > 0);
    assert!(found);
    assert_eq!(w.main().storage.get(hardwood), 0);
}

#[test]
fn remote_production_sends_only_finished_goods_home() {
    let mut w = remote();
    let hardwood = res(&w, "hardwood");
    let mut carried_home = 0;
    for _ in 0..6000 {
        w.step();
        carried_home = carried_home.max(w.main().storage.get(hardwood));
    }
    w.check_conservation().unwrap();
    assert!(beams_home(&w) > 0, "beams arrived");
    assert_eq!(carried_home, 0, "no hardwood went to Main Storage");
    assert!(w.building_at(BENCH_AT).unwrap().crafted > 0);
}

#[test]
fn central_production_makes_the_same_beams_at_home() {
    let mut w = central();
    w.run_ticks(6000);
    w.check_conservation().unwrap();
    assert!(beams_home(&w) > 0);
    assert!(w.building_at(TilePos::new(3, 16)).unwrap().crafted > 0);
}

#[test]
fn both_use_the_same_rules_with_no_outpost_bonus() {
    let mut c = central();
    let mut r = remote();
    c.run_ticks(9000);
    r.run_ticks(9000);
    let beams = res(&c, "beams");
    let hardwood = res(&c, "hardwood");
    for w in [&c, &r] {
        let made = w.ledger.crafted[&beams];
        assert_eq!(w.ledger.processed[&hardwood], made * 2, "2 hardwood a beam");
    }
    let d = &c.defs.recipes[recipe(&c, "structural_beams").index()];
    let e = &r.defs.recipes[recipe(&r, "structural_beams").index()];
    assert_eq!(d.craft_ticks, e.craft_ticks);
    assert_eq!(
        c.defs
            .building(c.buildings[id_at(&r, BENCH_AT).min(1)].kind)
            .slots,
        2
    );
}

#[test]
fn geography_decides_how_far_the_hauler_walks() {
    let runs = compare_layouts(20);
    let get = |name: &str| runs.iter().find(|r| r.name.contains(name)).unwrap();
    let central = get("Central");
    let remote = get("made at Old Growth");
    let bridge = get("by the bridge");
    assert!(remote.hauler_tiles < central.hauler_tiles, "less walking");
    assert!(
        bridge.hauler_tiles > remote.hauler_tiles,
        "placement matters"
    );
    assert_eq!(
        central.beams_home, remote.beams_home,
        "the same work got done"
    );
    assert!(
        remote.hauler_units < central.hauler_units,
        "half the load travels far"
    );
}

#[test]
fn an_outpost_pays_off_once_it_has_its_own_hauler() {
    let runs = compare_layouts(20);
    let tiles = |name: &str| runs.iter().find(|r| r.name == name).unwrap().hauler_tiles;
    // One hauler split between both ends of the map walks further for it.
    assert!(
        tiles("Remote, one hauler, planks too") > tiles("Central, one hauler, planks too"),
        "a lone shared hauler favours central production"
    );
    // A second one posted there turns that around.
    assert!(
        tiles("Remote, two haulers, planks too") < tiles("Central, two haulers, planks too"),
        "a posted hauler favours remote production"
    );
}

#[test]
fn staffing_changes_the_hauling_but_not_a_regrowth_limited_target() {
    let runs = compare_layouts(20);
    let get = |name: &str| runs.iter().find(|r| r.name.contains(name)).unwrap();
    let full = get("made at Old Growth");
    let one = get("one gatherer");
    assert_eq!(one.beams_home, full.beams_home);
    // A slower trickle of hardwood means smaller, more frequent loads.
    assert!(one.hauler_tiles > full.hauler_tiles);
}

#[test]
fn logs_are_not_carried_to_a_far_bench_while_a_near_one_can_use_them() {
    let mut w = remote();
    let logs = res(&w, "logs");
    let far = id_at(&w, BENCH_AT);
    let home = id_at(&w, TilePos::new(3, 16));
    let main = w.main_storage;
    let stocked = w.buildings[main].storage.deposit(logs, 200);
    *w.ledger.seeded.entry(logs).or_default() += stocked as u64;
    w.apply(Command::SetProductionTarget(60)).unwrap();
    // A second worker at home. The outpost's worker could make planks too.
    let second = spirit_of(&w, JobKind::Craft);
    let mut outpost = w.spirits[second].policy;
    outpost.station = None;
    w.apply(Command::SetSpiritPolicy {
        spirit: second,
        policy: outpost,
    })
    .unwrap();
    let mut policy = w.spirits[second].policy;
    policy.station = Some(far);
    w.apply(Command::SetSpiritPolicy {
        spirit: second,
        policy,
    })
    .unwrap();
    w.add_worker_for_test(home);
    for _ in 0..6000 {
        w.step();
        assert_eq!(
            w.buildings[far].storage.get(logs),
            0,
            "logs went the wrong way"
        );
    }
    assert!(w.buildings[home].crafted > 0, "planks were made at home");
}

// ---------------------------------------------------------------------------
// Multi-facility logistics
// ---------------------------------------------------------------------------

#[test]
fn a_hauler_fetches_from_the_nearest_stock() {
    let mut w = remote();
    let hardwood = res(&w, "hardwood");
    let store = id_at(&w, STORE_AT);
    let main = w.main_storage;
    w.buildings[main].storage.deposit(hardwood, 30);
    w.buildings[store].storage.deposit(hardwood, 30);
    *w.ledger.seeded.entry(hardwood).or_default() += 60;
    let mut took_from_main = false;
    let start = w.main().storage.get(hardwood);
    for _ in 0..1500 {
        w.step();
        took_from_main |= w.main().storage.get(hardwood) < start;
    }
    assert!(!took_from_main, "Main Storage is the far one");
    assert!(w.buildings[store].storage.get(hardwood) < 30 + 20);
}

#[test]
fn stock_is_never_shared_between_buildings() {
    let mut w = remote();
    let hardwood = res(&w, "hardwood");
    let store = id_at(&w, STORE_AT);
    let bench = id_at(&w, BENCH_AT);
    w.buildings[store].storage.deposit(hardwood, 20);
    *w.ledger.seeded.entry(hardwood).or_default() += 20;
    // With nobody carrying, nothing can reach the bench, however close it is.
    for job in [JobKind::Supply, JobKind::Craft] {
        let spirit = spirit_of(&w, job);
        let mut policy = w.spirits[spirit].policy;
        policy.focus = spirit_sim::entities::Focus::Rest;
        w.apply(Command::SetSpiritPolicy { spirit, policy })
            .unwrap();
    }
    let held = w.buildings[bench].storage.get(hardwood);
    w.run_ticks(1200);
    assert_eq!(w.buildings[bench].storage.get(hardwood), held);
    assert_eq!(w.building_at(BENCH_AT).unwrap().crafted, 0);
}

#[test]
fn a_store_keeps_what_it_is_told_and_the_surplus_goes_home() {
    let mut w = remote();
    let hardwood = res(&w, "hardwood");
    let store = id_at(&w, STORE_AT);
    // Nothing uses hardwood here, so only the keep can hold it back.
    let beams = res(&w, "beams");
    w.apply(Command::SetOrderEnabled {
        output: beams,
        enabled: false,
    })
    .unwrap();
    w.apply(Command::SetLocalStock {
        building: store,
        resource: hardwood,
        amount: 12,
    })
    .unwrap();
    w.buildings[store].storage.deposit(hardwood, 40);
    *w.ledger.seeded.entry(hardwood).or_default() += 40;
    assert!(run_until(&mut w, 2400, |w| w.main().storage.get(hardwood) >= 20));
    w.run_ticks(600);
    assert_eq!(
        w.buildings[store].storage.get(hardwood),
        12,
        "the keep stayed"
    );
}

#[test]
fn gatherers_top_up_the_local_supply_then_stop() {
    let mut w = remote();
    let hardwood = res(&w, "hardwood");
    let beams = res(&w, "beams");
    let store = id_at(&w, STORE_AT);
    w.apply(Command::SetOrderTarget {
        output: beams,
        target: 0,
    })
    .unwrap();
    assert!(run_until(&mut w, 6000, |w| w.buildings[store]
        .storage
        .get(hardwood)
        >= 20));
    w.run_ticks(300);
    let stock = w.buildings[store].storage.get(hardwood);
    w.run_ticks(900);
    assert_eq!(
        w.buildings[store].storage.get(hardwood),
        stock,
        "they stopped"
    );
    assert!(stock <= 40, "not much past the keep: {stock}");
}

#[test]
fn a_free_worker_uses_the_nearest_bench_with_work() {
    let mut w = remote();
    let worker = spirit_of(&w, JobKind::Craft);
    let hardwood = res(&w, "hardwood");
    let mut p = w.spirits[worker].policy;
    p.station = None;
    w.apply(Command::SetSpiritPolicy {
        spirit: worker,
        policy: p,
    })
    .unwrap();
    // Plenty at the home bench: it has no reason to cross the map.
    let home = id_at(&w, TilePos::new(3, 16));
    w.buildings[home].storage.deposit(hardwood, 20);
    *w.ledger.seeded.entry(hardwood).or_default() += 20;
    assert!(run_until(&mut w, 600, |w| {
        matches!(w.spirits[worker].actor.queue.front(), Some(Task::Craft { building, .. }) if *building == home)
    }));
}

#[test]
fn a_free_worker_does_not_cross_the_map_for_one_batch() {
    let mut w = remote();
    let worker = spirit_of(&w, JobKind::Craft);
    let hauler = spirit_of(&w, JobKind::Supply);
    let hardwood = res(&w, "hardwood");
    let bench = id_at(&w, BENCH_AT);
    for id in [worker, hauler] {
        let mut p = w.spirits[id].policy;
        p.station = None;
        if id == hauler {
            p.focus = spirit_sim::entities::Focus::Rest;
        }
        w.apply(Command::SetSpiritPolicy {
            spirit: id,
            policy: p,
        })
        .unwrap();
    }
    w.buildings[bench].storage.deposit(hardwood, 2);
    *w.ledger.seeded.entry(hardwood).or_default() += 2;
    w.run_ticks(300);
    assert_eq!(w.building_at(BENCH_AT).unwrap().crafted, 0);
    assert!(w.spirits[worker].actor.pos().x < 20, "it stayed home");
    w.buildings[bench].storage.deposit(hardwood, 8);
    *w.ledger.seeded.entry(hardwood).or_default() += 8;
    assert!(run_until(&mut w, 1500, |w| w
        .building_at(BENCH_AT)
        .unwrap()
        .crafted
        > 0));
}

#[test]
fn an_assigned_worker_stays_at_its_bench() {
    let mut w = remote();
    let worker = spirit_of(&w, JobKind::Craft);
    let bench = id_at(&w, BENCH_AT);
    let mut benches = std::collections::BTreeSet::new();
    for _ in 0..9000 {
        w.step();
        if let Some(Task::Craft { building, .. }) = w.spirits[worker].actor.queue.front() {
            benches.insert(*building);
        }
    }
    assert_eq!(benches.into_iter().collect::<Vec<_>>(), vec![bench]);
}

#[test]
fn a_hauler_tied_to_a_site_only_serves_that_site() {
    let mut w = World::from_scenario(ScenarioKind::RemoteProduction);
    let site = old_growth(&w);
    let hauler = spirit_of(&w, JobKind::Supply);
    let mut p = w.spirits[hauler].policy;
    p.site = Some(site);
    w.apply(Command::SetSpiritPolicy {
        spirit: hauler,
        policy: p,
    })
    .unwrap();
    // Finished goods at the home bench aren't its business.
    let beams = res(&w, "beams");
    let home = id_at(&w, TilePos::new(3, 16));
    w.buildings[home].storage.deposit(beams, 10);
    *w.ledger.seeded.entry(beams).or_default() += 10;
    let before = w.main().storage.get(beams);
    w.run_ticks(500);
    assert_eq!(w.buildings[home].storage.get(beams), 10);
    assert_eq!(w.main().storage.get(beams), before);
}

#[test]
fn a_long_trip_waits_for_a_real_load() {
    let mut w = central();
    let hardwood = res(&w, "hardwood");
    let hauler = spirit_of(&w, JobKind::Supply);
    let store = id_at(&w, STORE_AT);
    let gatherers: Vec<usize> = w.gatherer_ids();
    for g in gatherers {
        let mut p = w.spirits[g].policy;
        p.focus = spirit_sim::entities::Focus::Rest;
        w.apply(Command::SetSpiritPolicy {
            spirit: g,
            policy: p,
        })
        .unwrap();
    }
    w.buildings[store].storage.deposit(hardwood, 3);
    *w.ledger.seeded.entry(hardwood).or_default() += 3;
    w.run_ticks(400);
    assert_eq!(
        w.spirits[hauler].trips, 0,
        "three logs aren't worth the walk"
    );
    assert!(w.spirits[hauler].actor.pos().x < 20);
    w.buildings[store].storage.deposit(hardwood, 12);
    *w.ledger.seeded.entry(hardwood).or_default() += 12;
    assert!(run_until(&mut w, 1000, |w| w.spirits[hauler].trips > 0));
}

// ---------------------------------------------------------------------------
// Construction demand
// ---------------------------------------------------------------------------

#[test]
fn ordering_a_workshop_makes_the_chain_produce_what_it_needs() {
    let mut w = establishing();
    let beams = res(&w, "beams");
    let hardwood = res(&w, "hardwood");
    w.apply(Command::SetOrderTarget {
        output: beams,
        target: 0,
    })
    .unwrap();
    w.run_ticks(300);
    assert_eq!(
        w.ledger.harvested.get(&hardwood),
        None,
        "no reason to cut it yet"
    );

    order(&mut w, "workshop", TilePos::new(8, 18));
    let needs = w.project_needs();
    let line = needs.iter().find(|n| n.name == "Structural Beams").unwrap();
    assert_eq!((line.have, line.need), (0, 6));
    assert!(
        run_until(&mut w, 18000, |w| w.active_project().is_none()),
        "it was built"
    );
    assert_eq!(
        w.ledger.crafted.get(&beams),
        Some(&6),
        "as many as it needed"
    );
    assert_eq!(w.ledger.consumed.get(&beams), Some(&6));
    assert!(w.ledger.harvested[&hardwood] >= 12);
}

#[test]
fn after_the_project_workers_settle_and_say_so() {
    let mut w = establishing();
    let beams = res(&w, "beams");
    w.apply(Command::SetOrderTarget {
        output: beams,
        target: 0,
    })
    .unwrap();
    order(&mut w, "workshop", TilePos::new(8, 18));
    run_until(&mut w, 18000, |w| w.active_project().is_none());
    w.run_ticks(1800);
    let builder = spirit_of(&w, JobKind::Construct);
    assert_eq!(w.worker_state(&w.spirits[builder]), WorkerState::Unassigned);
    let worker = spirit_of(&w, JobKind::Craft);
    let state = w.worker_state(&w.spirits[worker]);
    assert!(
        matches!(state, WorkerState::GoalsMet | WorkerState::Working),
        "{state:?}"
    );
    // New demand wakes them.
    let made = w.spirits[worker].units_crafted;
    w.apply(Command::SetOrderTarget {
        output: beams,
        target: 4,
    })
    .unwrap();
    assert!(run_until(&mut w, 9000, |w| w.spirits[worker].units_crafted > made));
}

#[test]
fn production_leaves_a_project_its_raw_materials() {
    let mut w = World::from_scenario(ScenarioKind::WorkshopAutomated);
    let logs = res(&w, "logs");
    let main = w.main_storage;
    let spare = w.input_spare_for_test(logs);
    let kind = w.defs.building_kind_id("training_cache").unwrap();
    let grove = w.defs.site_id("starting_grove").unwrap();
    w.apply(Command::SetTrainingArea(Some(grove))).unwrap();
    let stock = w.buildings[main].storage.available(logs);
    w.apply(Command::RequestBuilding(kind)).unwrap();
    if !w.projects[0].reserved {
        assert!(w.input_spare_for_test(logs) < spare);
    }
    assert!(stock > 0);
}

#[test]
fn the_four_kinds_of_idle_are_told_apart() {
    // Goals met.
    let mut w = World::from_scenario(ScenarioKind::WorkshopAutomated);
    w.apply(Command::SetProductionTarget(0)).unwrap();
    w.apply(Command::SetObjectiveTarget(0)).unwrap();
    w.run_ticks(600);
    let worker = spirit_of(&w, JobKind::Craft);
    assert_eq!(w.worker_state(&w.spirits[worker]), WorkerState::GoalsMet);
    // Awaiting an assignment.
    w.apply(Command::SetSpiritAssigned {
        spirit: worker,
        assigned: false,
    })
    .unwrap();
    w.run_ticks(20);
    assert_eq!(w.worker_state(&w.spirits[worker]), WorkerState::Unassigned);

    // Resources unavailable.
    let mut w = central();
    let site = old_growth(&w);
    for t in &mut w.trees {
        if t.site == Some(site) {
            t.state = TreeState::Depleted {
                regrow_at: 1_000_000,
            };
        }
    }
    w.run_ticks(100);
    let gatherer = w.gatherer_ids()[0];
    assert_eq!(
        w.worker_state(&w.spirits[gatherer]),
        WorkerState::NoResources
    );

    // Blocked by logistics: more workers than places.
    let mut w = World::from_scenario(ScenarioKind::ProductionBottleneck);
    w.run_ticks(600);
    let states: Vec<_> = w.crafters().map(|s| w.worker_state(s)).collect();
    assert!(states.contains(&WorkerState::Logistics), "{states:?}");
}

#[test]
fn a_posted_gatherer_is_idle_not_blocked_when_logs_are_short() {
    let mut w = remote();
    let beams = res(&w, "beams");
    w.apply(Command::SetOrderTarget {
        output: beams,
        target: 0,
    })
    .unwrap();
    w.apply(Command::SetObjectiveTarget(500)).unwrap();
    let store = id_at(&w, STORE_AT);
    let hardwood = res(&w, "hardwood");
    w.apply(Command::SetLocalStock {
        building: store,
        resource: hardwood,
        amount: 0,
    })
    .unwrap();
    w.run_ticks(200);
    for s in w.gatherers_for_test() {
        assert_eq!(w.worker_state(s), WorkerState::GoalsMet, "{:?}", s.activity);
    }
}

#[test]
fn the_glance_separates_awaiting_work_from_goals_met() {
    let mut w = establishing();
    w.run_ticks(100);
    let g = w.settlement_glance();
    assert!(g.unassigned >= 1, "the builder has no project");
    assert!(g.idle >= g.unassigned);
}

// ---------------------------------------------------------------------------
// Training at any bench
// ---------------------------------------------------------------------------

/// The remote bench with materials on their way.
fn training_at_the_outpost() -> World {
    let mut w = remote();
    let ww = w.defs.skill_id("woodworking").unwrap();
    w.player_add_xp_for_test(ww, 1154);
    let bench = id_at(&w, BENCH_AT);
    let beams = recipe(&w, "structural_beams");
    w.apply(Command::SetPlayerRecipe(Some(beams))).unwrap();
    w.apply(Command::SetTrainingBench(Some(bench))).unwrap();
    w.apply(Command::SetTrainingMode(TrainingMode::Relaxed))
        .unwrap();
    w
}

#[test]
fn the_player_can_choose_the_remote_bench() {
    let w = training_at_the_outpost();
    assert_eq!(w.training_bench().unwrap().pos, BENCH_AT);
    assert_eq!(w.recipe_options().len(), 4);
    let report = w.training_report();
    assert!(report.bench.1 > 0);
}

#[test]
fn only_a_finished_workshop_can_be_chosen() {
    let mut w = remote();
    let store = id_at(&w, STORE_AT);
    assert!(w.apply(Command::SetTrainingBench(Some(store))).is_err());
}

#[test]
fn haulers_bring_the_chosen_recipes_material_to_the_chosen_bench() {
    let mut w = training_at_the_outpost();
    let hardwood = res(&w, "hardwood");
    let bench = id_at(&w, BENCH_AT);
    let store = id_at(&w, STORE_AT);
    w.buildings[store].storage.deposit(hardwood, 40);
    *w.ledger.seeded.entry(hardwood).or_default() += 40;
    assert!(run_until(&mut w, 2500, |w| w.buildings[bench]
        .storage
        .reserved(hardwood)
        > 0));
    assert_eq!(
        w.main().storage.get(hardwood),
        0,
        "no trip through Main Storage"
    );
    assert!(w.bench_stock() > 0);
}

#[test]
fn the_player_trains_at_the_outpost_with_spirits_carrying() {
    let mut w = training_at_the_outpost();
    let ww = w.defs.skill_id("woodworking").unwrap();
    let hardwood = res(&w, "hardwood");
    let store = id_at(&w, STORE_AT);
    w.buildings[store].storage.deposit(hardwood, 40);
    *w.ledger.seeded.entry(hardwood).or_default() += 40;
    let before = w.player.xp(ww);
    w.apply(Command::PlayerInteract(BENCH_AT)).unwrap();
    assert!(run_until(&mut w, 3000, |w| w.player.xp(ww) >= before + 240));
    assert_eq!(w.player.log.workshop_interactions, 1, "one click");
    assert_eq!(w.player.log.storage_interactions, 0);
}

#[test]
fn working_at_a_bench_makes_it_your_bench() {
    let mut w = training_at_the_outpost();
    w.apply(Command::SetTrainingBench(None)).unwrap();
    assert_ne!(w.training_bench().unwrap().pos, BENCH_AT);
    let hardwood = res(&w, "hardwood");
    w.player.actor.inventory.add(hardwood, 4);
    *w.ledger.seeded.entry(hardwood).or_default() += 4;
    w.apply(Command::PlayerInteract(BENCH_AT)).unwrap();
    assert_eq!(w.training_bench().unwrap().pos, BENCH_AT);
}

#[test]
fn changing_bench_or_recipe_returns_set_aside_stock_where_it_lies() {
    let mut w = training_at_the_outpost();
    let hardwood = res(&w, "hardwood");
    let store = id_at(&w, STORE_AT);
    let bench = id_at(&w, BENCH_AT);
    w.buildings[store].storage.deposit(hardwood, 40);
    *w.ledger.seeded.entry(hardwood).or_default() += 40;
    run_until(&mut w, 2500, |w| {
        w.buildings[bench].storage.reserved(hardwood) > 0
    });
    let stock = w.buildings[bench].storage.get(hardwood);
    let rough = recipe(&w, "rough_planks");
    w.apply(Command::SetPlayerRecipe(Some(rough))).unwrap();
    assert_eq!(w.buildings[bench].storage.reserved(hardwood), 0);
    assert_eq!(
        w.buildings[bench].storage.get(hardwood),
        stock,
        "nothing moved"
    );
    no_resource_appears_from_nowhere(&w);
}

#[test]
fn a_kept_place_only_applies_at_your_bench() {
    let mut w = training_at_the_outpost();
    w.apply(Command::SetKeepBenchPlace(true)).unwrap();
    let home = id_at(&w, TilePos::new(3, 16));
    let bench = id_at(&w, BENCH_AT);
    let b = w.buildings[bench].clone();
    let h = w.buildings[home].clone();
    assert!(w.slot_free_for_test(&h), "the other bench is unaffected");
    assert!(w.slot_free_for_test(&b));
}

#[test]
fn spirits_never_earn_the_player_xp_at_the_outpost() {
    let mut w = training_at_the_outpost();
    let earned = w.ledger.xp_earned;
    let spirit_xp: Vec<u64> = w.spirits.iter().map(|s| s.xp()).collect();
    w.run_ticks(6000);
    assert_eq!(w.ledger.xp_earned, earned);
    assert_eq!(
        w.spirits.iter().map(|s| s.xp()).collect::<Vec<_>>(),
        spirit_xp
    );
}

// ---------------------------------------------------------------------------
// Running without help
// ---------------------------------------------------------------------------

#[test]
fn the_outpost_runs_itself_and_conserves_everything() {
    let mut w = remote();
    for _ in 0..12000 {
        w.step();
        if w.tick.is_multiple_of(50) {
            no_resource_appears_from_nowhere(&w);
        }
    }
    assert!(beams_home(&w) >= 30);
}

#[test]
fn it_stops_at_the_target_and_starts_again_when_stock_is_used() {
    let mut w = remote();
    let beams = res(&w, "beams");
    w.apply(Command::SetOrderTarget {
        output: beams,
        target: 6,
    })
    .unwrap();
    assert!(run_until(&mut w, 9000, |w| beams_home(w) >= 6));
    w.run_ticks(1200);
    assert!(beams_home(&w) <= 8, "it stopped near the target");
    let made = w.building_at(BENCH_AT).unwrap().crafted;
    w.run_ticks(1200);
    assert_eq!(
        w.building_at(BENCH_AT).unwrap().crafted,
        made,
        "and stayed stopped"
    );
    w.apply(Command::SetOrderTarget {
        output: beams,
        target: 12,
    })
    .unwrap();
    assert!(
        run_until(&mut w, 9000, |w| beams_home(w) >= 12),
        "it picked up again"
    );
}

#[test]
fn a_full_main_storage_stalls_the_hauler_and_it_recovers() {
    let mut w = remote();
    let logs = res(&w, "logs");
    let main = w.main_storage;
    assert!(run_until(&mut w, 9000, |w| beams_home(w) > 0));
    let room = w.buildings[main].storage.free();
    w.buildings[main].storage.deposit(logs, room);
    *w.ledger.seeded.entry(logs).or_default() += room as u64;
    w.run_ticks(1500);
    let stuck = beams_home(&w);
    let hauler = spirit_of(&w, JobKind::Supply);
    assert_eq!(w.main().storage.free(), 0);
    // Room comes back.
    let taken = w.buildings[main].storage.withdraw_available(logs, 100);
    *w.ledger.consumed.entry(logs).or_default() += taken as u64;
    assert!(
        run_until(&mut w, 4000, |w| beams_home(w) > stuck),
        "deliveries resumed"
    );
    assert!(w.spirits[hauler].trips > 0);
}

#[test]
fn trees_running_out_pauses_gatherers_until_they_regrow() {
    let mut w = remote();
    let site = old_growth(&w);
    let regrow = w.tick + 900;
    for t in &mut w.trees {
        if t.site == Some(site) {
            t.state = TreeState::Depleted { regrow_at: regrow };
        }
    }
    w.run_ticks(400);
    let g = w.gatherer_ids()[0];
    assert_eq!(w.worker_state(&w.spirits[g]), WorkerState::NoResources);
    let hardwood = res(&w, "hardwood");
    assert!(run_until(&mut w, 2500, |w| {
        w.ledger.harvested.get(&hardwood).copied().unwrap_or(0) > 0
    }));
}

#[test]
fn a_full_store_sends_gatherers_on_without_losing_anything() {
    let mut w = remote();
    let logs = res(&w, "logs");
    let store = id_at(&w, STORE_AT);
    let room = w.buildings[store].storage.free();
    w.buildings[store].storage.deposit(logs, room);
    *w.ledger.seeded.entry(logs).or_default() += room as u64;
    w.apply(Command::SetLocalStock {
        building: store,
        resource: logs,
        amount: room,
    })
    .unwrap();
    w.run_ticks(3000);
    no_resource_appears_from_nowhere(&w);
    assert_eq!(w.buildings[store].storage.free(), 0);
}

#[test]
fn a_busy_bench_turns_the_next_worker_away_and_it_comes_back() {
    let mut w = remote();
    let bench = id_at(&w, BENCH_AT);
    let worker = spirit_of(&w, JobKind::Craft);
    assert!(w.spirits[worker].policy.station == Some(bench));
    run_until(&mut w, 6000, |w| {
        w.building_at(BENCH_AT).unwrap().crafted > 0
    });
    let slots = w.defs.building(w.buildings[bench].kind).slots;
    assert!(w.slots_used(bench) <= slots);
}

#[test]
fn reservations_are_released_when_a_trip_fails() {
    let mut w = remote();
    let hardwood = res(&w, "hardwood");
    let store = id_at(&w, STORE_AT);
    let hauler = spirit_of(&w, JobKind::Supply);
    w.buildings[store].storage.deposit(hardwood, 30);
    *w.ledger.seeded.entry(hardwood).or_default() += 30;
    assert!(run_until(&mut w, 2000, |w| w.buildings[store]
        .storage
        .reserved(hardwood)
        > 0));
    // The store stops being usable while the hauler is on its way.
    w.buildings[store].state =
        spirit_sim::entities::BuildingState::UnderConstruction { project: 0 };
    assert!(run_until(&mut w, 1500, |w| {
        !w.spirits[hauler]
            .actor
            .queue
            .iter()
            .any(|t| matches!(t, Task::Withdraw { .. }))
    }));
    assert_eq!(
        w.buildings[store].storage.reserved(hardwood),
        0,
        "nothing stays held"
    );
    no_resource_appears_from_nowhere(&w);
}

#[test]
fn no_trip_is_made_twice_for_the_same_goods() {
    // Two haulers, one pile: what's claimed by one isn't there for the other.
    let mut w = World::from_scenario(ScenarioKind::OutpostEstablishment);
    let hardwood = res(&w, "hardwood");
    let beams = res(&w, "beams");
    let bench = w.station().unwrap().id;
    w.buildings[bench].storage.deposit(beams, 6);
    *w.ledger.seeded.entry(beams).or_default() += 6;
    w.apply(Command::SetOrderTarget {
        output: beams,
        target: 6,
    })
    .unwrap();
    assert!(run_until(&mut w, 1200, |w| beams_home(w) == 6));
    assert_eq!(
        w.spirits
            .iter()
            .map(|s| s.actor.inventory.get(beams))
            .sum::<u32>(),
        0
    );
    assert_eq!(w.main().storage.get(hardwood), 0);
}

// ---------------------------------------------------------------------------
// Saves and replay
// ---------------------------------------------------------------------------

fn round_trip(w: &World, key: &str) -> World {
    decode(&encode(w, key)).unwrap().0
}

fn continues_exactly(mut w: World, key: &str) {
    let mut l = round_trip(&w, key);
    assert_eq!(l.state_text(), w.state_text());
    for _ in 0..2500 {
        w.step();
        l.step();
    }
    assert_eq!(l.state_text(), w.state_text());
    assert_eq!(l.state_hash(), w.state_hash());
    no_resource_appears_from_nowhere(&l);
}

#[test]
fn a_running_outpost_continues_exactly_after_loading() {
    for ticks in [700, 1900, 4100] {
        let mut w = remote();
        w.run_ticks(ticks);
        continues_exactly(w, "remote_production");
    }
}

#[test]
fn construction_at_old_growth_survives_loading() {
    let mut w = establishing();
    order(&mut w, "outpost_storage", STORE_AT);
    run_until(&mut w, 900, |w| {
        w.active_project().is_some_and(|p| !p.delivered.is_empty())
    });
    continues_exactly(w, "outpost_establishment");
}

#[test]
fn instructions_and_local_stock_survive_loading() {
    let mut w = training_at_the_outpost();
    let store = id_at(&w, STORE_AT);
    let hardwood = res(&w, "hardwood");
    w.apply(Command::SetLocalStock {
        building: store,
        resource: hardwood,
        amount: 33,
    })
    .unwrap();
    let l = round_trip(&w, "remote_production");
    assert_eq!(l.buildings[store].keep[&hardwood], 33);
    assert_eq!(l.settlement.training.bench, w.settlement.training.bench);
    for (a, b) in l.spirits.iter().zip(&w.spirits) {
        assert_eq!(a.policy, b.policy);
    }
    continues_exactly(w, "remote_production");
}

#[test]
fn training_at_the_outpost_continues_exactly_after_loading() {
    let mut w = training_at_the_outpost();
    let hardwood = res(&w, "hardwood");
    let store = id_at(&w, STORE_AT);
    w.buildings[store].storage.deposit(hardwood, 40);
    *w.ledger.seeded.entry(hardwood).or_default() += 40;
    w.apply(Command::PlayerInteract(BENCH_AT)).unwrap();
    w.run_ticks(900);
    continues_exactly(w, "remote_production");
}

#[test]
fn a_keep_of_an_unknown_material_is_refused() {
    let w = remote();
    let text = encode(&w, "remote_production");
    let json = text.split_once('\n').unwrap().1;
    let damaged = json.replacen("\"keep\":{\"2\":20}", "\"keep\":{\"99\":20}", 1);
    if damaged != json {
        let sealed = spirit_sim::save::seal(SAVE_VERSION, &damaged);
        assert!(decode(&sealed).is_err());
    }
    let bench = json.replacen("\"bench\":null", "\"bench\":999", 1);
    assert_ne!(bench, json);
    assert!(decode(&spirit_sim::save::seal(SAVE_VERSION, &bench)).is_err());
}

#[test]
fn replays_are_identical() {
    let run = || {
        let mut w = establishing();
        let site = old_growth(&w);
        order(&mut w, "outpost_storage", STORE_AT);
        run_until(&mut w, 3000, |w| w.active_project().is_none());
        order(&mut w, "workshop", BENCH_AT);
        run_until(&mut w, 12000, |w| w.active_project().is_none());
        w.apply(Command::SetOutpostStaffed { site, on: true })
            .unwrap();
        w.run_ticks(2000);
        (w.state_hash(), w.state_text())
    };
    assert_eq!(run(), run());
}

fn fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    String::from_utf8(std::fs::read(path).unwrap()).unwrap()
}

#[test]
fn saves_from_v0_7_load_and_a_session_carries_on() {
    let (mut w, meta) = decode(&fixture("v3_session.save")).unwrap();
    assert_eq!(meta.version, 3);
    assert!(w.player.session.is_some(), "the session came through");
    let wc = w.defs.skill_id("woodcutting").unwrap();
    let xp = w.player.xp(wc);
    w.run_ticks(600);
    no_resource_appears_from_nowhere(&w);
    assert!(w.player.xp(wc) > xp);

    let (mut p, _) = decode(&fixture("v3_expansion.save")).unwrap();
    assert_eq!(p.stations().count(), 2);
    assert_eq!(p.settlement.training.bench, None);
    let made = p.ledger.crafted.clone();
    p.run_ticks(1800);
    no_resource_appears_from_nowhere(&p);
    assert!(p.ledger.crafted != made, "production carried on");
}

#[test]
fn every_older_save_still_loads() {
    for name in [
        "v1_journey.save",
        "v1_workshop.save",
        "v2_journey.save",
        "v2_workshop.save",
    ] {
        let (mut w, _) = decode(&fixture(name)).unwrap();
        w.run_ticks(600);
        no_resource_appears_from_nowhere(&w);
    }
}

#[test]
fn an_upgraded_v0_7_save_keeps_its_original_and_can_build_an_outpost() {
    let dir = std::env::temp_dir().join(format!("spiritscape_v08_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("world.save"), fixture("v3_expansion.save")).unwrap();
    let store = SaveStore::new(&dir);
    let loaded = store.load(Slot::Manual).unwrap();
    assert!(loaded.warning.unwrap().contains("world.save.v3"));
    let mut w = loaded.world;
    run_until(&mut w, 3000, |w| w.active_project().is_none());
    order(&mut w, "outpost_storage", STORE_AT);
    store
        .write(Slot::Manual, &w, "expanded_production")
        .unwrap();
    assert_eq!(store.meta(Slot::Manual).unwrap().version, SAVE_VERSION);
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// The whole path
// ---------------------------------------------------------------------------

#[test]
fn an_outpost_can_be_established_with_four_instructions() {
    let (w, e) = establish_outpost(10).unwrap();
    no_resource_appears_from_nowhere(&w);
    assert!(e.store_built < e.workshop_built);
    assert_eq!(e.commands, 4);
    assert!(e.beams_for_building >= 6);
    assert!(
        e.beams_home_running > 0,
        "goods came home without further orders"
    );
    assert!(w.building_at(BENCH_AT).unwrap().crafted > 0);
}

#[test]
fn demand_comes_from_goals_so_quiet_after_them_is_fine() {
    let (_, e) = establish_outpost(10).unwrap();
    assert!(e.busy_building_pct > 20, "building gave spirits real work");
}
