//! v0.9: the Forest Crossing, the construction plan, demand from planned
//! buildings, hauler choices, and the storage rules that keep a long game moving.

use spirit_sim::autoplay::{
    compare_expansion, compare_hauling, crossing_payback, play_to_expansion,
};
use spirit_sim::defs::{BuildingKindId, JobKind, ResourceId, SiteId};
use spirit_sim::entities::{BuildingState, ProjectState, TreeState};
use spirit_sim::grid::{Terrain, TilePos};
use spirit_sim::projects::PlanStage;
use spirit_sim::save::{SAVE_VERSION, SaveStore, Slot, decode, encode};
use spirit_sim::tasks::Task;
use spirit_sim::{Command, ScenarioKind, World};

const STORE_AT: TilePos = TilePos { x: 34, y: 4 };
const BENCH_AT: TilePos = TilePos { x: 36, y: 4 };
const NEAR_BANK: TilePos = TilePos { x: 47, y: 4 };
const FAR_BANK: TilePos = TilePos { x: 50, y: 4 };

fn world(kind: ScenarioKind) -> World {
    World::from_scenario(kind)
}

fn res(w: &World, id: &str) -> ResourceId {
    w.defs.resource_id(id).unwrap()
}

fn kind(w: &World, id: &str) -> BuildingKindId {
    w.defs.building_kind_id(id).unwrap()
}

fn grove(w: &World) -> SiteId {
    w.defs.site_id("hollow_grove").unwrap()
}

fn plan(w: &mut World, id: &str, site: Option<TilePos>) -> Result<String, String> {
    let kind = kind(w, id);
    w.apply(Command::QueueBuilding { kind, site })
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

fn stock(w: &mut World, id: &str, n: u32) {
    let r = res(w, id);
    let main = w.main_storage;
    let stored = w.buildings[main].storage.deposit(r, n);
    *w.ledger.seeded.entry(r).or_default() += stored as u64;
}

fn reachable(w: &World, from: TilePos, to: TilePos) -> bool {
    w.grid.distance_field(&[from]).get(to).is_some()
}

fn crossing_built(w: &World) -> bool {
    let k = kind(w, "forest_crossing");
    w.buildings.iter().any(|b| b.kind == k && b.is_complete())
}

/// The outpost with everything the crossing costs already in storage.
fn ready_to_cross() -> World {
    let mut w = world(ScenarioKind::ForestAccess);
    stock(&mut w, "planks", 40);
    stock(&mut w, "beams", 10);
    w
}

fn spirit_of(w: &World, job: JobKind) -> usize {
    w.spirits
        .iter()
        .position(|s| w.defs.role(s.role).has_job(job))
        .unwrap()
}

// ---------------------------------------------------------------------------
// The crossing and the grove
// ---------------------------------------------------------------------------

#[test]
fn the_grove_cannot_be_reached_until_the_crossing_is_built() {
    let mut w = ready_to_cross();
    let site = grove(&w);
    assert!(!reachable(&w, NEAR_BANK, FAR_BANK));
    assert!(w.site_locked_by(site).is_some());
    plan(&mut w, "forest_crossing", None).unwrap();
    assert!(run_until(&mut w, 3000, crossing_built));
    assert!(reachable(&w, NEAR_BANK, FAR_BANK), "the way is open");
    assert_eq!(w.site_locked_by(site), None);
    assert_eq!(w.grid.terrain(TilePos::new(48, 4)), Some(Terrain::Bridge));
    assert_eq!(w.grid.terrain(TilePos::new(49, 4)), Some(Terrain::Bridge));
    assert_eq!(w.grid.terrain(TilePos::new(48, 3)), Some(Terrain::Water));
}

#[test]
fn the_crossing_uses_real_materials_and_a_builder() {
    let mut w = ready_to_cross();
    plan(&mut w, "forest_crossing", None).unwrap();
    run_until(&mut w, 3000, crossing_built);
    let beams = res(&w, "beams");
    let planks = res(&w, "planks");
    assert_eq!(w.ledger.consumed.get(&beams), Some(&10));
    assert_eq!(w.ledger.consumed.get(&planks), Some(&40));
    let builder = spirit_of(&w, JobKind::Construct);
    assert!(w.spirits[builder].time.total.building > 0);
}

#[test]
fn nothing_can_cut_the_grove_while_it_is_shut() {
    let mut w = world(ScenarioKind::RemoteProduction);
    let site = grove(&w);
    let full: u32 = w
        .trees
        .iter()
        .filter(|t| t.site == Some(site))
        .map(|t| t.logs_left())
        .sum();
    assert!(full > 0);
    w.run_ticks(6000);
    let left: u32 = w
        .trees
        .iter()
        .filter(|t| t.site == Some(site))
        .map(|t| t.logs_left())
        .sum();
    assert_eq!(left, full);
}

#[test]
fn the_player_is_told_why_a_tree_across_the_ravine_cannot_be_reached() {
    let mut w = world(ScenarioKind::ForestAccess);
    let site = grove(&w);
    let p = w.trees.iter().find(|t| t.site == Some(site)).unwrap().pos;
    let err = w.apply(Command::PlayerInteract(p)).unwrap_err();
    assert!(err.contains("Forest Crossing"), "{err}");
}

#[test]
fn spirits_posted_to_old_growth_work_the_grove_once_it_opens() {
    let mut w = world(ScenarioKind::ExpandedRemote);
    let site = grove(&w);
    let full: u32 = w
        .trees
        .iter()
        .filter(|t| t.site == Some(site))
        .map(|t| t.logs_left())
        .sum();
    assert!(run_until(&mut w, 9000, |w| {
        w.trees
            .iter()
            .filter(|t| t.site == Some(site))
            .map(|t| t.logs_left())
            .sum::<u32>()
            < full
    }));
}

#[test]
fn the_grove_regrows_like_anywhere_else_and_has_its_limit() {
    let w = world(ScenarioKind::ExpandedRemote);
    let report = w.gathering_report();
    let old = report
        .sites
        .iter()
        .find(|s| s.name == "Old Growth")
        .unwrap();
    let new = report
        .sites
        .iter()
        .find(|s| s.name == "Hollow Grove")
        .unwrap();
    assert_eq!(new.trees_total, 6);
    assert!(new.sustainable_per_min > 0.0);
    assert!(
        new.sustainable_per_min < old.sustainable_per_min,
        "not unlimited"
    );
    let mut w = w;
    let site = grove(&w);
    let tick = w.tick;
    for t in &mut w.trees {
        if t.site == Some(site) {
            t.state = TreeState::Depleted {
                regrow_at: tick + 50,
            };
        }
    }
    w.run_ticks(60);
    assert!(
        w.trees
            .iter()
            .filter(|t| t.site == Some(site))
            .all(|t| t.is_grown())
    );
}

#[test]
fn more_access_raises_output_but_a_worker_alone_does_not() {
    let runs = compare_expansion(5, 15);
    let get = |name: &str| runs.iter().find(|r| r.name.starts_with(name)).unwrap();
    let original = get("A. Original, remote");
    let expanded = get("C. Expanded, remote");
    let worker = get("Original, remote, third gatherer");
    assert!(expanded.hardwood_per_min > original.hardwood_per_min * 1.2);
    assert!(expanded.beams_per_min > original.beams_per_min * 1.2);
    assert!(
        worker.hardwood_per_min < original.hardwood_per_min * 1.05,
        "regrowth was the limit, not gatherers"
    );
    assert!(original.waiting_pct > expanded.waiting_pct);
}

#[test]
fn the_crossing_pays_for_itself() {
    let p = crossing_payback(45);
    assert!(p.built_at.is_some());
    assert!(p.caught_up_at.is_some());
    assert!(p.with > p.without);
}

// ---------------------------------------------------------------------------
// The plan
// ---------------------------------------------------------------------------

#[test]
fn a_crossing_needs_a_store_first_or_one_planned_ahead_of_it() {
    let mut w = world(ScenarioKind::ProjectQueue);
    let err = plan(&mut w, "forest_crossing", None).unwrap_err();
    assert!(err.contains("Outpost Storage"), "{err}");
    plan(&mut w, "outpost_storage", Some(STORE_AT)).unwrap();
    plan(&mut w, "forest_crossing", None).unwrap();
}

#[test]
fn projects_run_one_at_a_time_in_the_order_given() {
    let mut w = world(ScenarioKind::ProjectQueue);
    plan(&mut w, "outpost_storage", Some(STORE_AT)).unwrap();
    plan(&mut w, "workshop", Some(BENCH_AT)).unwrap();
    plan(&mut w, "forest_crossing", None).unwrap();
    assert_eq!(
        w.active_project().unwrap().kind,
        kind(&w, "outpost_storage")
    );
    assert_eq!(w.settlement.queue.len(), 2);
    let active = |w: &World| w.projects.iter().filter(|p| p.is_active()).count();
    for _ in 0..3000 {
        w.step();
        assert!(active(&w) <= 1);
    }
    assert!(run_until(&mut w, 30000, crossing_built));
    let order: Vec<BuildingKindId> = w.projects.iter().map(|p| p.kind).collect();
    let want = [
        kind(&w, "outpost_storage"),
        kind(&w, "workshop"),
        kind(&w, "forest_crossing"),
    ];
    assert_eq!(order, want);
    assert!(w.settlement.queue.is_empty());
}

#[test]
fn the_plan_has_a_limit() {
    let mut w = world(ScenarioKind::ProjectQueue);
    plan(&mut w, "outpost_storage", Some(STORE_AT)).unwrap();
    plan(&mut w, "storehouse", None).unwrap();
    plan(&mut w, "storehouse", None).unwrap();
    plan(&mut w, "storehouse", None).unwrap();
    plan(&mut w, "log_rack", None).unwrap();
    let err = plan(&mut w, "log_rack", None).unwrap_err();
    assert!(err.contains("full"), "{err}");
}

#[test]
fn duplicates_and_impossible_sites_are_refused() {
    let mut w = ready_to_cross();
    plan(&mut w, "forest_crossing", None).unwrap();
    assert!(plan(&mut w, "forest_crossing", None).is_err(), "only one");
    let mut w = world(ScenarioKind::ProjectQueue);
    plan(&mut w, "outpost_storage", Some(STORE_AT)).unwrap();
    plan(&mut w, "storehouse", None).unwrap();
    assert!(
        plan(&mut w, "workshop", Some(TilePos::new(21, 10))).is_err(),
        "water"
    );
    plan(&mut w, "workshop", Some(BENCH_AT)).unwrap();
    assert!(
        plan(&mut w, "training_cache", Some(BENCH_AT)).is_err(),
        "taken"
    );
}

#[test]
fn the_plan_can_be_reordered_within_reason() {
    let mut w = world(ScenarioKind::ProjectQueue);
    plan(&mut w, "storehouse", None).unwrap();
    plan(&mut w, "outpost_storage", Some(STORE_AT)).unwrap();
    plan(&mut w, "log_rack", None).unwrap();
    plan(&mut w, "forest_crossing", None).unwrap();
    let ids: Vec<u32> = w.settlement.queue.iter().map(|q| q.id).collect();
    // The rack can go ahead of the store.
    w.apply(Command::MoveQueued {
        id: ids[1],
        up: true,
    })
    .unwrap();
    assert_eq!(w.settlement.queue[0].id, ids[1]);
    // The store can't go behind the crossing that needs it.
    let err = w
        .apply(Command::MoveQueued {
            id: ids[0],
            up: false,
        })
        .unwrap_err();
    assert!(err.contains("has to come before"), "{err}");
}

#[test]
fn cancelling_a_planned_building_changes_nothing_else() {
    let mut w = world(ScenarioKind::ProjectQueue);
    plan(&mut w, "storehouse", None).unwrap();
    plan(&mut w, "log_rack", None).unwrap();
    let id = w.settlement.queue[0].id;
    let before = w.main().storage.clone();
    w.apply(Command::CancelQueued(id)).unwrap();
    assert!(w.settlement.queue.is_empty());
    assert_eq!(w.main().storage.contents(), before.contents());
    assert_eq!(
        w.project_demand_for_test(res(&w, "planks")),
        w.project_demand_for_test(res(&w, "planks"))
    );
}

#[test]
fn something_another_project_needs_cannot_be_cancelled_first() {
    let mut w = world(ScenarioKind::ProjectQueue);
    plan(&mut w, "storehouse", None).unwrap();
    plan(&mut w, "outpost_storage", Some(STORE_AT)).unwrap();
    plan(&mut w, "forest_crossing", None).unwrap();
    let store = w.settlement.queue[0].id;
    let err = w.apply(Command::CancelQueued(store)).unwrap_err();
    assert!(err.contains("Forest Crossing"), "{err}");
}

#[test]
fn cancelling_a_project_gives_back_its_reservation_and_its_tile() {
    let mut w = world(ScenarioKind::ProjectQueue);
    let logs = res(&w, "logs");
    let available = w.main().storage.available(logs);
    plan(&mut w, "outpost_storage", Some(STORE_AT)).unwrap();
    let project = w.active_project().unwrap();
    assert!(project.reserved);
    let (id, building) = (project.id, project.building);
    assert!(w.main().storage.available(logs) < available);
    w.apply(Command::CancelProject(id)).unwrap();
    assert_eq!(w.main().storage.available(logs), available, "released");
    assert_eq!(w.projects[id].state, ProjectState::Cancelled);
    assert_eq!(w.buildings[building].state, BuildingState::Removed);
    assert!(w.grid.is_buildable(STORE_AT), "the tile is free again");
    assert!(w.building_at(STORE_AT).is_none());
    plan(&mut w, "outpost_storage", Some(STORE_AT)).unwrap();
    assert!(run_until(&mut w, 3000, |w| w.active_project().is_none()));
    w.check_conservation().unwrap();
}

#[test]
fn work_under_way_cannot_be_cancelled() {
    let mut w = world(ScenarioKind::ProjectQueue);
    plan(&mut w, "outpost_storage", Some(STORE_AT)).unwrap();
    let id = w.active_project().unwrap().id;
    assert!(run_until(&mut w, 1500, |w| !w.projects[id]
        .delivered
        .is_empty()));
    assert!(w.apply(Command::CancelProject(id)).is_err());
}

#[test]
fn the_next_project_starts_by_itself_when_the_last_one_finishes() {
    let mut w = world(ScenarioKind::ProjectQueue);
    plan(&mut w, "log_rack", None).unwrap();
    plan(&mut w, "storehouse", None).unwrap();
    assert!(run_until(&mut w, 6000, |w| w.projects.len() == 2));
    assert_eq!(w.player.log.gather_interactions, 0);
}

#[test]
fn a_full_plan_finishes_while_the_player_only_chops() {
    use spirit_sim::autoplay::{Bot, Goal};
    let mut w = world(ScenarioKind::ProjectQueue);
    plan(&mut w, "outpost_storage", Some(STORE_AT)).unwrap();
    plan(&mut w, "workshop", Some(BENCH_AT)).unwrap();
    plan(&mut w, "forest_crossing", None).unwrap();
    let mut bot = Bot::new(&w);
    for _ in 0..30000 {
        if crossing_built(&w) {
            break;
        }
        bot.play(&mut w, Goal::Chop);
        w.step();
    }
    assert!(crossing_built(&w));
    w.check_conservation().unwrap();
}

// ---------------------------------------------------------------------------
// Demand
// ---------------------------------------------------------------------------

#[test]
fn queued_buildings_create_demand_and_stock_counts_toward_it() {
    let mut w = world(ScenarioKind::ForestAccess);
    let beams = res(&w, "beams");
    w.apply(Command::SetOrderTarget {
        output: beams,
        target: 0,
    })
    .unwrap();
    assert_eq!(w.goal_of(beams), 0);
    plan(&mut w, "forest_crossing", None).unwrap();
    assert_eq!(w.goal_of(beams), 10);
    assert_eq!(w.deficit_of(beams), 10);
    stock(&mut w, "beams", 4);
    assert_eq!(w.deficit_of(beams), 6, "what's in storage counts");
}

#[test]
fn project_needs_add_to_standing_targets_rather_than_replace_them() {
    let mut w = world(ScenarioKind::ForestAccess);
    let beams = res(&w, "beams");
    w.apply(Command::SetOrderTarget {
        output: beams,
        target: 7,
    })
    .unwrap();
    plan(&mut w, "forest_crossing", None).unwrap();
    assert_eq!(w.goal_of(beams), 17);
}

#[test]
fn stock_is_never_promised_to_two_projects() {
    let mut w = world(ScenarioKind::ProjectQueue);
    stock(&mut w, "beams", 10);
    plan(&mut w, "storehouse", None).unwrap();
    plan(&mut w, "outpost_storage", Some(STORE_AT)).unwrap();
    plan(&mut w, "forest_crossing", None).unwrap();
    plan(&mut w, "workshop", Some(BENCH_AT)).unwrap();
    let views = w.plan_views();
    let beams_of = |name: &str| {
        let v = views.iter().find(|v| v.name == name).unwrap();
        v.needs
            .iter()
            .find(|n| n.name == "Structural Beams")
            .unwrap()
            .clone()
    };
    assert_eq!(beams_of("Forest Crossing").have, 10);
    assert_eq!(beams_of("Workshop").have, 0, "the crossing spoke for them");
}

#[test]
fn demand_reaches_all_the_way_to_the_trees_and_then_stops() {
    let mut w = world(ScenarioKind::ForestAccess);
    let beams = res(&w, "beams");
    let hardwood = res(&w, "hardwood");
    w.apply(Command::SetOrderTarget {
        output: beams,
        target: 0,
    })
    .unwrap();
    let store = w.building_at(STORE_AT).unwrap().id;
    w.apply(Command::SetLocalStock {
        building: store,
        resource: hardwood,
        amount: 0,
    })
    .unwrap();
    w.run_ticks(300);
    assert_eq!(
        w.ledger.harvested.get(&hardwood),
        None,
        "nothing wants it yet"
    );
    plan(&mut w, "forest_crossing", None).unwrap();
    assert!(run_until(&mut w, 20000, crossing_built));
    assert_eq!(
        w.ledger.crafted.get(&beams),
        Some(&10),
        "exactly what it needed"
    );
    assert!(w.ledger.harvested[&hardwood] >= 20, "cut for real");
    let cut = w.ledger.harvested[&hardwood];
    w.run_ticks(1200);
    assert!(
        w.ledger.harvested[&hardwood] <= cut + 10,
        "and then it stopped"
    );
}

#[test]
fn a_shortage_stalls_the_plan_and_it_resumes_on_its_own() {
    let mut w = world(ScenarioKind::ForestAccess);
    let beams = res(&w, "beams");
    w.apply(Command::SetOrderEnabled {
        output: beams,
        enabled: false,
    })
    .unwrap();
    plan(&mut w, "forest_crossing", None).unwrap();
    w.run_ticks(1200);
    assert!(!crossing_built(&w));
    let view = &w.plan_views()[0];
    assert_eq!(view.stage, PlanStage::GatheringMaterials);
    assert!(view.reason.as_ref().unwrap().contains("Structural Beams"));
    w.apply(Command::SetOrderEnabled {
        output: beams,
        enabled: true,
    })
    .unwrap();
    assert!(run_until(&mut w, 20000, crossing_built));
}

#[test]
fn the_plan_says_what_it_is_for_and_who_is_on_it() {
    let mut w = world(ScenarioKind::ForestAccess);
    plan(&mut w, "forest_crossing", None).unwrap();
    w.run_ticks(600);
    let v = &w.plan_views()[0];
    assert_eq!(v.name, "Forest Crossing");
    assert!(v.benefit.contains("Hollow Grove"));
    assert!(v.cost.contains("Structural Beams"));
    assert!(!v.workers.is_empty(), "a builder is assigned");
    let beams = v
        .needs
        .iter()
        .find(|n| n.name == "Structural Beams")
        .unwrap();
    assert!(beams.need == 10 && !beams.status.is_empty());
    assert!(w.plannable().iter().all(|k| w.defs.building(*k).manual));
    run_until(&mut w, 20000, crossing_built);
    let last = w.plan_views().pop().unwrap();
    assert_eq!(last.stage, PlanStage::Finished);
}

// ---------------------------------------------------------------------------
// Hauling
// ---------------------------------------------------------------------------

#[test]
fn scored_jobs_cut_a_shared_haulers_walking() {
    let runs = compare_hauling(20);
    let (fixed, scored) = (&runs[0], &runs[1]);
    assert!(
        scored.tiles * 10 < fixed.tiles * 9,
        "{} vs {}",
        scored.tiles,
        fixed.tiles
    );
    assert!(scored.walking_pct < fixed.walking_pct);
    assert!(scored.trips < fixed.trips, "fuller trips");
}

#[test]
fn scoring_does_not_lose_any_work() {
    let runs = compare_hauling(20);
    assert!(runs[1].reached_target.unwrap() <= runs[0].reached_target.unwrap());
}

#[test]
fn a_trip_home_picks_up_nearby_goods_too() {
    let mut w = world(ScenarioKind::RemoteProduction);
    let beams = res(&w, "beams");
    let planks = res(&w, "planks");
    let bench = w.building_at(BENCH_AT).unwrap().id;
    let store = w.building_at(STORE_AT).unwrap().id;
    w.buildings[bench].storage.deposit(beams, 8);
    w.buildings[store].storage.deposit(planks, 6);
    *w.ledger.seeded.entry(beams).or_default() += 8;
    *w.ledger.seeded.entry(planks).or_default() += 6;
    let hauler = spirit_of(&w, JobKind::Haul);
    assert!(
        run_until(&mut w, 900, |w| {
            let bag = &w.spirits[hauler].actor.inventory;
            bag.get(beams) > 0 && bag.get(planks) > 0
        }),
        "one trip carried both"
    );
}

#[test]
fn nearby_work_comes_before_a_long_trip_for_a_small_load() {
    let mut w = world(ScenarioKind::SharedHauler);
    let hauler = spirit_of(&w, JobKind::Haul);
    let beams = res(&w, "beams");
    let planks = res(&w, "planks");
    let far = w.building_at(BENCH_AT).unwrap().id;
    let home = w.building_at(TilePos::new(3, 16)).unwrap().id;
    w.buildings[far].storage.deposit(beams, 2);
    w.buildings[home].storage.deposit(planks, 10);
    *w.ledger.seeded.entry(beams).or_default() += 2;
    *w.ledger.seeded.entry(planks).or_default() += 10;
    w.run_ticks(60);
    assert!(w.spirits[hauler].actor.pos().x < 15, "it stayed home");
}

#[test]
fn a_waiting_worker_makes_a_long_trip_urgent() {
    let mut w = world(ScenarioKind::SharedHauler);
    let hardwood = res(&w, "hardwood");
    let store = w.building_at(STORE_AT).unwrap().id;
    w.buildings[store].storage.deposit(hardwood, 20);
    *w.ledger.seeded.entry(hardwood).or_default() += 20;
    let bench = w.building_at(BENCH_AT).unwrap().id;
    assert!(run_until(&mut w, 3000, |w| w.buildings[bench]
        .storage
        .get(hardwood)
        > 0));
}

#[test]
fn no_goods_are_claimed_twice_by_two_haulers() {
    let mut w = world(ScenarioKind::ForestAccess);
    let beams = res(&w, "beams");
    let bench = w.building_at(BENCH_AT).unwrap().id;
    w.buildings[bench].storage.deposit(beams, 9);
    *w.ledger.seeded.entry(beams).or_default() += 9;
    for _ in 0..1500 {
        w.step();
        for b in &w.buildings {
            assert!(b.storage.reserved(beams) <= b.storage.get(beams));
        }
    }
    w.check_conservation().unwrap();
}

// ---------------------------------------------------------------------------
// The annex
// ---------------------------------------------------------------------------

#[test]
fn an_annex_adds_a_real_place_at_the_bench_beside_it() {
    let mut w = world(ScenarioKind::ProductionBottleneck);
    stock(&mut w, "beams", 8);
    stock(&mut w, "planks", 30);
    let bench = w.station().unwrap().id;
    assert_eq!(w.slots_of(&w.buildings[bench]), 2);
    plan(&mut w, "workshop_annex", None).unwrap();
    assert!(run_until(&mut w, 3000, |w| w.active_project().is_none()));
    assert_eq!(w.slots_of(&w.buildings[bench]), 3);
    assert!(
        run_until(&mut w, 1200, |w| w.slots_used(bench) == 3),
        "it gets used"
    );
}

#[test]
fn the_player_can_train_beside_two_spirits_after_an_annex() {
    let mut w = world(ScenarioKind::ProductionBottleneck);
    stock(&mut w, "beams", 8);
    stock(&mut w, "planks", 30);
    let bench = w.station().unwrap();
    let (at, id) = (bench.pos, bench.id);
    let logs = res(&w, "logs");
    w.player.actor.inventory.add(logs, 10);
    *w.ledger.seeded.entry(logs).or_default() += 10;
    run_until(&mut w, 2000, |w| w.slots_used(id) >= 2);
    assert!(w.apply(Command::PlayerInteract(at)).is_err(), "no room yet");
    plan(&mut w, "workshop_annex", None).unwrap();
    assert!(run_until(&mut w, 3000, |w| w.active_project().is_none()));
    w.settlement.training.keep_bench_place = true;
    w.apply(Command::PlayerInteract(at)).unwrap();
    assert!(w.player.session.is_some());
}

#[test]
fn an_annex_makes_nobody_faster() {
    let w = world(ScenarioKind::ForestAccess);
    let d = w.defs.building(kind(&w, "workshop_annex"));
    assert_eq!((d.capacity, d.capacity_bonus, d.training_bonus), (0, 0, 0));
}

// ---------------------------------------------------------------------------
// What the long game found
// ---------------------------------------------------------------------------

#[test]
fn storehouses_cannot_wall_anyone_in() {
    let mut w = world(ScenarioKind::WorkshopAutomated);
    stock(&mut w, "planks", 200);
    stock(&mut w, "logs", 100);
    let k = kind(&w, "storehouse");
    for _ in 0..3 {
        w.apply(Command::RequestBuilding(k)).unwrap();
        assert!(run_until(&mut w, 3000, |w| w.active_project().is_none()));
    }
    let main = w.main().pos;
    let around: Vec<TilePos> = w.grid.walkable_neighbors(main).collect();
    assert!(!around.is_empty());
    for t in &around {
        assert!(reachable(&w, *t, around[0]), "{t} was cut off");
    }
}

#[test]
fn surplus_goods_leave_room_and_wanted_goods_do_not() {
    let mut w = world(ScenarioKind::WorkshopAutomated);
    let planks = res(&w, "planks");
    let logs = res(&w, "logs");
    let room = w.defs.construction.raw_room;
    let main = w.main_storage;
    let free = w.buildings[main].storage.free();
    stock(&mut w, "planks", free - room + 5);
    w.apply(Command::SetProductionTarget(0)).unwrap();
    assert_eq!(w.main_room_for_test(planks), 0, "nobody wants more");
    assert!(w.main_room_for_test(logs) > 0, "raw materials still fit");
    w.apply(Command::SetProductionTarget(5000)).unwrap();
    assert!(w.main_room_for_test(planks) > 0, "wanted goods may use it");
}

#[test]
fn the_player_is_told_when_storage_is_keeping_its_room() {
    let mut w = world(ScenarioKind::WorkshopAutomated);
    let planks = res(&w, "planks");
    let main = w.main_storage;
    let room = w.defs.construction.raw_room;
    let free = w.buildings[main].storage.free();
    stock(&mut w, "planks", free - room);
    w.apply(Command::SetProductionTarget(0)).unwrap();
    let bag = w.player.actor.inventory.free();
    w.player.actor.inventory.add(planks, bag);
    *w.ledger.seeded.entry(planks).or_default() += bag as u64;
    let at = w.main().pos;
    w.apply(Command::PlayerInteract(at)).unwrap();
    assert!(run_until(&mut w, 300, |w| w.player.actor.queue.is_empty()));
    assert!(
        w.player.activity.contains("keeping its room"),
        "{}",
        w.player.activity
    );
    assert!(w.player.actor.inventory.get(planks) > 0, "nothing was lost");
}

#[test]
fn the_builder_starts_early_when_storage_is_too_full_to_wait() {
    let mut w = world(ScenarioKind::ForestAccess);
    let beams = res(&w, "beams");
    let main = w.main_storage;
    w.apply(Command::SetOrderEnabled {
        output: beams,
        enabled: false,
    })
    .unwrap();
    let free = w.buildings[main].storage.free();
    stock(&mut w, "planks", free - 10);
    plan(&mut w, "forest_crossing", None).unwrap();
    assert!(
        run_until(&mut w, 1500, |w| {
            w.active_project().is_some_and(|p| !p.delivered.is_empty())
        }),
        "what's here started moving"
    );
    let p = w.active_project().unwrap();
    assert!(!p.reserved, "it's still waiting for beams");
    assert!(w.main().storage.free() > 10, "and that made room");
    w.check_conservation().unwrap();
}

#[test]
fn a_bench_is_only_staffed_by_someone_who_can_make_the_product() {
    // A level 5 worker at home can't make beams, so the hardwood has to go
    // to the outpost's level 10 worker.
    let mut w = world(ScenarioKind::ForestAccess);
    let hardwood = res(&w, "hardwood");
    let main = w.main_storage;
    let far = w.building_at(BENCH_AT).unwrap().id;
    let home = w.building_at(TilePos::new(3, 16)).unwrap().id;
    w.buildings[main].storage.deposit(hardwood, 30);
    *w.ledger.seeded.entry(hardwood).or_default() += 30;
    assert!(run_until(&mut w, 3000, |w| w.buildings[far]
        .storage
        .get(hardwood)
        > 0));
    assert_eq!(w.buildings[home].storage.get(hardwood), 0);
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
    l.check_conservation().unwrap();
}

#[test]
fn the_plan_and_reservations_survive_loading() {
    let mut w = world(ScenarioKind::ProjectQueue);
    plan(&mut w, "outpost_storage", Some(STORE_AT)).unwrap();
    plan(&mut w, "workshop", Some(BENCH_AT)).unwrap();
    plan(&mut w, "forest_crossing", None).unwrap();
    w.run_ticks(700);
    let l = round_trip(&w, "project_queue");
    assert_eq!(l.settlement.queue, w.settlement.queue);
    continues_exactly(w, "project_queue");
}

#[test]
fn an_open_crossing_survives_loading() {
    let mut w = ready_to_cross();
    plan(&mut w, "forest_crossing", None).unwrap();
    run_until(&mut w, 3000, crossing_built);
    let l = round_trip(&w, "forest_access");
    assert!(reachable(&l, NEAR_BANK, FAR_BANK));
    continues_exactly(w, "forest_access");
}

#[test]
fn a_half_claimed_project_survives_loading() {
    let mut w = world(ScenarioKind::ForestAccess);
    let main = w.main_storage;
    let free = w.buildings[main].storage.free();
    stock(&mut w, "planks", free - 10);
    plan(&mut w, "forest_crossing", None).unwrap();
    w.run_ticks(300);
    assert!(!w.active_project().unwrap().claimed.is_empty());
    continues_exactly(w, "forest_access");
}

#[test]
fn a_cancelled_project_stays_harmless_across_saves() {
    let mut w = world(ScenarioKind::ProjectQueue);
    plan(&mut w, "outpost_storage", Some(STORE_AT)).unwrap();
    let id = w.active_project().unwrap().id;
    w.apply(Command::CancelProject(id)).unwrap();
    continues_exactly(w, "project_queue");
}

#[test]
fn a_planned_building_of_an_unknown_kind_is_refused() {
    let mut w = world(ScenarioKind::ProjectQueue);
    plan(&mut w, "storehouse", None).unwrap();
    plan(&mut w, "log_rack", None).unwrap();
    let text = encode(&w, "project_queue");
    let json = text.split_once('\n').unwrap().1;
    let k = w.settlement.queue[0].kind.0;
    let damaged = json.replacen(
        &format!("\"kind\":{k},\"site\":null"),
        "\"kind\":999,\"site\":null",
        1,
    );
    assert_ne!(damaged, json);
    assert!(decode(&spirit_sim::save::seal(SAVE_VERSION, &damaged)).is_err());
}

#[test]
fn replays_are_identical() {
    let run = || {
        let mut w = world(ScenarioKind::ProjectQueue);
        plan(&mut w, "outpost_storage", Some(STORE_AT)).unwrap();
        plan(&mut w, "workshop", Some(BENCH_AT)).unwrap();
        plan(&mut w, "forest_crossing", None).unwrap();
        w.run_ticks(5000);
        (w.state_hash(), w.state_text())
    };
    assert_eq!(run(), run());
}

#[test]
fn xp_is_untouched_by_all_of_this() {
    let mut w = world(ScenarioKind::ProjectQueue);
    let spirit_xp: Vec<u64> = w.spirits.iter().map(|s| s.xp()).collect();
    plan(&mut w, "outpost_storage", Some(STORE_AT)).unwrap();
    plan(&mut w, "workshop", Some(BENCH_AT)).unwrap();
    plan(&mut w, "forest_crossing", None).unwrap();
    w.run_ticks(12000);
    assert_eq!(w.ledger.xp_earned, 0, "the player did nothing");
    assert_eq!(
        w.spirits.iter().map(|s| s.xp()).collect::<Vec<_>>(),
        spirit_xp
    );
}

fn fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    String::from_utf8(std::fs::read(path).unwrap()).unwrap()
}

#[test]
fn every_older_save_moves_onto_the_larger_map() {
    for name in [
        "v1_journey.save",
        "v1_workshop.save",
        "v2_journey.save",
        "v2_workshop.save",
        "v3_session.save",
        "v3_expansion.save",
        "v4_outpost.save",
        "v4_project.save",
    ] {
        let (mut w, _) = decode(&fixture(name)).unwrap();
        let site = grove(&w);
        assert_eq!(w.grid.width, 60, "{name}");
        let trees = w.trees.iter().filter(|t| t.site == Some(site)).count();
        assert_eq!(trees, 6, "{name}: the grove is there");
        assert!(!reachable(&w, NEAR_BANK, FAR_BANK), "{name}: and shut");
        w.run_ticks(900);
        w.check_conservation().unwrap();
    }
}

#[test]
fn migrating_changes_nothing_that_already_existed() {
    let (w, meta) = decode(&fixture("v4_outpost.save")).unwrap();
    assert_eq!(meta.version, 4);
    assert_eq!(meta.tick, w.tick);
    assert!(w.building_at(STORE_AT).is_some());
    assert!(w.building_at(BENCH_AT).is_some());
    assert!(w.grid.is_occupied(STORE_AT));
    assert_eq!(w.settlement.queue.len(), 0);
}

#[test]
fn a_v0_8_project_under_way_finishes_after_upgrading() {
    let (mut w, _) = decode(&fixture("v4_project.save")).unwrap();
    assert!(w.active_project().is_some());
    assert!(run_until(&mut w, 3000, |w| w.active_project().is_none()));
    assert!(w.building_at(STORE_AT).unwrap().is_complete());
}

#[test]
fn an_upgraded_v0_8_save_can_build_the_crossing_and_keeps_its_original() {
    let dir = std::env::temp_dir().join(format!("spiritscape_v09_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("world.save"), fixture("v4_outpost.save")).unwrap();
    let store = SaveStore::new(&dir);
    let loaded = store.load(Slot::Manual).unwrap();
    assert!(loaded.warning.unwrap().contains("world.save.v4"));
    let mut w = loaded.world;
    stock(&mut w, "planks", 40);
    stock(&mut w, "beams", 10);
    plan(&mut w, "forest_crossing", None).unwrap();
    assert!(run_until(&mut w, 4000, crossing_built));
    store.write(Slot::Manual, &w, "remote_production").unwrap();
    assert_eq!(store.meta(Slot::Manual).unwrap().version, SAVE_VERSION);
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// The whole road
// ---------------------------------------------------------------------------

/// Slow in a debug build: run with `cargo test --release -- --ignored`.
#[test]
#[ignore]
fn the_normal_game_reaches_the_expansion_with_nothing_granted() {
    let (w, r) = play_to_expansion("normal").unwrap();
    w.check_conservation().unwrap();
    assert!(crossing_built(&w));
    assert!(r.beams_after > 0 && r.hardwood_after > 0);
    let names: Vec<&str> = r.steps.iter().map(|s| s.0.as_str()).collect();
    assert!(names.contains(&"Forest Crossing finished"));
    assert_eq!(w.ledger.xp_separated, 1800 * 2 + 16000 * 2);
}

#[test]
fn normal_costs_give_surplus_planks_a_use() {
    let w = World::new_journey("normal").unwrap();
    let k = kind(&w, "forest_crossing");
    let planks = res(&w, "planks");
    let logs = res(&w, "logs");
    let cost = &w.defs.building(k).cost;
    assert_eq!(cost.iter().find(|c| c.0 == planks).unwrap().1, 160);
    assert_eq!(
        cost.iter().find(|c| c.0 == logs).unwrap().1,
        30,
        "raw costs stay"
    );
}

#[test]
fn a_task_for_a_cancelled_site_does_not_linger() {
    let mut w = world(ScenarioKind::ProjectQueue);
    plan(&mut w, "outpost_storage", Some(STORE_AT)).unwrap();
    let id = w.active_project().unwrap().id;
    w.run_ticks(5);
    w.apply(Command::CancelProject(id)).unwrap();
    let builder = spirit_of(&w, JobKind::Construct);
    assert!(
        !w.spirits[builder]
            .actor
            .queue
            .iter()
            .any(|t| matches!(t, Task::DeliverToSite { .. }))
    );
}
