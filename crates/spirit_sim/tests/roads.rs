//! Plank roads: paying for them, walking on them, and keeping them in saves.

use spirit_sim::ScenarioKind;
use spirit_sim::autoplay::{compare_roads, lay_roads, walk_seconds};
use spirit_sim::defs::{BuildingKindId, GameDefs, ResourceId};
use spirit_sim::entities::BuildingState;
use spirit_sim::grid::{Grid, MapLayout, Terrain, TilePos};
use spirit_sim::save::{SAVE_VERSION, SaveStore, Slot, decode, encode};
use spirit_sim::world::{Command, World};

const LANE_START: TilePos = TilePos::new(6, 14);
const LANE_END: TilePos = TilePos::new(16, 13);

fn world(kind: ScenarioKind) -> World {
    World::from_scenario(kind)
}

fn res(w: &World, id: &str) -> ResourceId {
    w.defs.resource_id(id).unwrap()
}

fn kind(w: &World, id: &str) -> BuildingKindId {
    w.defs.building_kind_id(id).unwrap()
}

fn plan(w: &mut World, id: &str) -> Result<String, String> {
    let kind = kind(w, id);
    w.apply(Command::QueueBuilding { kind, site: None })
}

fn built(w: &World, id: &str) -> bool {
    let k = kind(w, id);
    w.buildings.iter().any(|b| b.kind == k && b.is_complete())
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

fn fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    String::from_utf8(std::fs::read(path).unwrap()).unwrap()
}

fn grid(text: &str) -> Grid {
    Grid::new(&MapLayout::parse(text).unwrap())
}

#[test]
fn every_road_runs_over_open_ground_and_joins_the_next() {
    let w = world(ScenarioKind::NewGame);
    let mut roads = 0;
    for b in w.defs.buildings.iter().filter(|b| !b.paves.is_empty()) {
        roads += 1;
        for &[x, y] in &b.paves {
            let p = TilePos::new(x, y);
            assert_eq!(w.grid.terrain(p), Some(Terrain::Grass), "{} at {p}", b.id);
            assert!(!w.grid.is_occupied(p), "{} is blocked at {p}", b.id);
        }
        assert!(b.manual && !b.benefit.is_empty(), "{}", b.id);
        for pair in b.paves.windows(2) {
            let (a, c) = (pair[0], pair[1]);
            let step = TilePos::new(a[0], a[1]).manhattan(TilePos::new(c[0], c[1]));
            assert_eq!(step, 1, "{} has a gap", b.id);
        }
    }
    assert_eq!(roads, 4);
}

#[test]
fn a_road_with_corners_that_do_not_line_up_is_refused() {
    let text = spirit_sim::defs::BUILTIN_DEFS.replace(
        "paves = [[6, 14], [6, 13], [16, 13]]",
        "paves = [[6, 14], [7, 13]]",
    );
    let e = GameDefs::from_toml(&text).err().unwrap();
    assert!(e.contains("line up"), "{e}");
}

#[test]
fn a_road_is_built_with_real_planks_and_changes_the_ground() {
    let mut w = world(ScenarioKind::PlankSurplus);
    let planks = res(&w, "planks");
    let before = w.main().storage.get(planks);
    plan(&mut w, "grove_lane").unwrap();
    assert!(run_until(&mut w, 6000, |w| built(w, "grove_lane")));
    assert_eq!(w.main().storage.get(planks), before - 30);
    assert_eq!(w.ledger.consumed.get(&planks).copied(), Some(30));
    assert_eq!(w.grid.terrain(LANE_START), Some(Terrain::Road));
    assert_eq!(w.grid.terrain(LANE_END), Some(Terrain::Road));
    assert!(!w.grid.is_occupied(LANE_START), "the works are gone");
    assert!(w.grid.is_walkable(LANE_START));
}

#[test]
fn the_tile_a_road_was_started_from_is_just_road() {
    let mut w = world(ScenarioKind::PlankSurplus);
    plan(&mut w, "grove_lane").unwrap();
    assert!(w.building_at(LANE_START).is_some(), "the works are there");
    assert!(run_until(&mut w, 6000, |w| built(w, "grove_lane")));
    assert!(w.building_at(LANE_START).is_none());
    w.apply(Command::PlayerInteract(LANE_START)).unwrap();
    assert!(run_until(&mut w, 300, |w| w.player.actor.pos() == LANE_START));
}

#[test]
fn nothing_can_be_built_on_a_road() {
    let mut w = world(ScenarioKind::PlankSurplus);
    lay_roads(&mut w);
    assert!(!w.grid.is_buildable(LANE_END));
    let k = kind(&w, "log_rack");
    let e = w.apply(Command::QueueBuilding {
        kind: k,
        site: Some(LANE_END),
    });
    assert!(e.is_err());
}

#[test]
fn walkers_cross_road_tiles_faster() {
    let g = grid("...\n");
    let mut w = world(ScenarioKind::PlankSurplus);
    assert_eq!(g.step_ticks(TilePos::new(1, 0), 3), 3);
    lay_roads(&mut w);
    assert_eq!(w.grid.step_ticks(LANE_END, 3), 2);
    assert_eq!(w.grid.step_ticks(TilePos::new(0, 0), 3), 3);
    assert_eq!(w.grid.step_ticks(LANE_END, 1), 1, "never instant");
}

#[test]
fn paths_go_out_of_their_way_for_a_road_when_it_pays() {
    let mut g = grid("..........\n..........\n..........\n");
    let (from, to) = (TilePos::new(0, 1), TilePos::new(9, 1));
    let plain = g.find_path(from, |p| p == to).unwrap();
    assert_eq!(plain.len(), 9);
    for x in 0..10 {
        g.pave(TilePos::new(x, 0), 67);
    }
    let path = g.find_path(from, |p| p == to).unwrap();
    assert_eq!(path.len(), 11, "two extra tiles");
    assert!(path.iter().filter(|p| p.y == 0).count() >= 9);
}

#[test]
fn paths_do_not_bother_with_a_road_that_is_not_worth_it() {
    let mut g = grid("....\n....\n....\n....\n....\n....\n");
    let (from, to) = (TilePos::new(0, 3), TilePos::new(1, 3));
    for x in 0..4 {
        g.pave(TilePos::new(x, 0), 67);
    }
    assert_eq!(g.find_path(from, |p| p == to).unwrap(), vec![to]);
}

#[test]
fn a_road_stops_at_what_stands_on_it_and_paths_go_around() {
    let mut w = world(ScenarioKind::PlankSurplus);
    let blocked = TilePos::new(10, 13);
    let rack = kind(&w, "log_rack");
    w.apply(Command::QueueBuilding {
        kind: rack,
        site: Some(blocked),
    })
    .unwrap();
    assert!(run_until(&mut w, 6000, |w| built(w, "log_rack")));
    plan(&mut w, "grove_lane").unwrap();
    assert!(run_until(&mut w, 6000, |w| built(w, "grove_lane")));
    assert_eq!(w.grid.terrain(blocked), Some(Terrain::Grass));
    assert!(w.grid.is_occupied(blocked));
    let path = w.grid.find_path(LANE_START, |p| p == LANE_END).unwrap();
    assert!(!path.contains(&blocked));
}

#[test]
fn work_starts_further_along_when_the_near_end_is_taken() {
    let mut w = world(ScenarioKind::PlankSurplus);
    w.grid.set_occupied(LANE_START, true);
    w.grid.set_occupied(TilePos::new(6, 13), true);
    plan(&mut w, "grove_lane").unwrap();
    let p = w.active_project().expect("started");
    assert_eq!(p.site, TilePos::new(7, 13));
}

#[test]
fn roads_follow_each_other_in_order() {
    let mut w = world(ScenarioKind::PlankSurplus);
    let e = plan(&mut w, "north_road").unwrap_err();
    assert!(e.contains("Grove Lane"), "{e}");
    plan(&mut w, "grove_lane").unwrap();
    plan(&mut w, "north_road").unwrap();
    plan(&mut w, "old_growth_road").unwrap();
    plan(&mut w, "forest_road").unwrap();
    assert!(run_until(&mut w, 40000, |w| {
        ["grove_lane", "north_road", "old_growth_road", "forest_road"]
            .iter()
            .all(|r| built(w, r))
    }));
    let planks = res(&w, "planks");
    assert_eq!(w.ledger.consumed.get(&planks).copied(), Some(190));
}

#[test]
fn a_road_cannot_be_built_twice() {
    let mut w = world(ScenarioKind::PlankSurplus);
    plan(&mut w, "grove_lane").unwrap();
    assert!(plan(&mut w, "grove_lane").is_err());
}

#[test]
fn a_road_can_be_called_off_before_planks_leave_storage() {
    let mut w = world(ScenarioKind::PlankSurplus);
    plan(&mut w, "grove_lane").unwrap();
    let pid = w.active_project().unwrap().id;
    let site = w.active_project().unwrap().site;
    let b = w.active_project().unwrap().building;
    w.apply(Command::CancelProject(pid)).unwrap();
    assert_eq!(w.buildings[b].state, BuildingState::Removed);
    assert!(!w.grid.is_occupied(site));
    assert_eq!(w.grid.terrain(site), Some(Terrain::Grass));
    plan(&mut w, "grove_lane").unwrap();
}

#[test]
fn normal_rules_charge_four_times_the_planks() {
    let normal = GameDefs::builtin_profile("normal").unwrap();
    let total: u32 = normal
        .buildings
        .iter()
        .filter(|b| !b.paves.is_empty())
        .flat_map(|b| b.cost.iter().map(|c| c.1))
        .sum();
    assert_eq!(total, 760);
}

#[test]
fn storage_advice_points_at_a_road_when_planks_are_the_problem() {
    let mut w = world(ScenarioKind::PlankSurplus);
    let planks = res(&w, "planks");
    let free = w.main().storage.free();
    let main = w.main_storage;
    w.buildings[main].storage.deposit(planks, free);
    let advice = w.storage_advice().expect("full");
    assert!(
        advice.contains("plan the Grove Lane in Projects (B)"),
        "{advice}"
    );
}

#[test]
fn storage_advice_stops_offering_roads_when_they_are_all_laid() {
    let mut w = world(ScenarioKind::PlankSurplus);
    let planks = res(&w, "planks");
    for r in ["grove_lane", "north_road", "old_growth_road", "forest_road"] {
        plan(&mut w, r).unwrap();
    }
    assert!(run_until(&mut w, 40000, |w| built(w, "forest_road")));
    let free = w.main().storage.free();
    let main = w.main_storage;
    w.buildings[main].storage.deposit(planks, free);
    let advice = w.storage_advice().expect("full");
    assert!(!advice.contains("Projects (B)"), "{advice}");
}

#[test]
fn the_player_gets_about_faster() {
    let (home, far) = (TilePos::new(6, 15), TilePos::new(35, 4));
    let (without, with) = (
        walk_seconds(false, home, far),
        walk_seconds(true, home, far),
    );
    assert!(with < without * 0.8, "{with} against {without}");
}

#[test]
fn nothing_changes_until_a_road_exists() {
    let mut a = world(ScenarioKind::ExpandedCentral);
    let mut b = world(ScenarioKind::ExpandedCentral);
    a.run_ticks(3000);
    b.run_ticks(3000);
    assert_eq!(a.state_hash(), b.state_hash());
    let g = &a.grid;
    let path = g.find_path(LANE_START, |p| p == LANE_END).unwrap();
    assert_eq!(path.len() as u32, LANE_START.manhattan(LANE_END));
}

#[test]
fn roads_repeat_exactly() {
    let run = || {
        let mut w = world(ScenarioKind::ExpandedRemote);
        lay_roads(&mut w);
        w.run_ticks(6000);
        w.state_hash()
    };
    assert_eq!(run(), run());
}

#[test]
fn roads_do_not_lose_anything_in_a_long_run() {
    let mut w = world(ScenarioKind::ExpandedRemote);
    lay_roads(&mut w);
    for _ in 0..12000 {
        w.step();
    }
    w.check_conservation().unwrap();
}

#[test]
fn a_road_survives_saving_and_continues_identically() {
    let mut w = world(ScenarioKind::PlankSurplus);
    plan(&mut w, "grove_lane").unwrap();
    assert!(run_until(&mut w, 6000, |w| built(w, "grove_lane")));
    plan(&mut w, "north_road").unwrap();
    w.run_ticks(200);
    let (mut loaded, _) = decode(&encode(&w, "plank_surplus")).unwrap();
    assert_eq!(loaded.grid.terrain(LANE_END), Some(Terrain::Road));
    assert_eq!(loaded.grid.step_ticks(LANE_END, 3), 2);
    w.run_ticks(1500);
    loaded.run_ticks(1500);
    assert_eq!(w.state_hash(), loaded.state_hash());
}

#[test]
fn a_walker_saved_in_mid_step_on_a_road_arrives_on_the_same_tick() {
    let mut w = world(ScenarioKind::ExpandedCentral);
    lay_roads(&mut w);
    w.run_ticks(301);
    let (mut loaded, _) = decode(&encode(&w, "expanded_central")).unwrap();
    w.run_ticks(600);
    loaded.run_ticks(600);
    assert_eq!(w.state_hash(), loaded.state_hash());
}

#[test]
fn v0_9_saves_load_and_can_lay_roads() {
    for name in ["v5_plan.save", "v5_crossing.save"] {
        let (mut w, meta) = decode(&fixture(name)).unwrap();
        assert_eq!(meta.version, 5, "{name}");
        assert_eq!(meta.tick, w.tick);
        w.run_ticks(600);
        w.check_conservation().unwrap();
        assert!(w.plannable().contains(&kind(&w, "grove_lane")), "{name}");
    }
}

#[test]
fn a_v0_9_plan_under_way_finishes_after_upgrading() {
    let (mut w, _) = decode(&fixture("v5_plan.save")).unwrap();
    assert!(w.active_project().is_some());
    assert_eq!(w.settlement.queue.len(), 1);
    assert!(run_until(&mut w, 30000, |w| built(w, "forest_crossing")));
}

#[test]
fn an_open_crossing_stays_open_and_the_original_save_is_kept() {
    let dir = std::env::temp_dir().join(format!("spiritscape_v10_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("world.save"), fixture("v5_crossing.save")).unwrap();
    let store = SaveStore::new(&dir);
    let loaded = store.load(Slot::Manual).unwrap();
    assert!(loaded.warning.unwrap().contains("world.save.v5"));
    let w = loaded.world;
    assert_eq!(w.grid.terrain(TilePos::new(48, 4)), Some(Terrain::Bridge));
    store.write(Slot::Manual, &w, "forest_access").unwrap();
    assert_eq!(store.meta(Slot::Manual).unwrap().version, SAVE_VERSION);
    assert!(dir.join("world.save.v5").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn roads_help_a_settlement_whose_hauler_is_the_limit() {
    let runs = compare_roads(5, 15);
    let find = |n: &str| runs.iter().find(|r| r.name == n).unwrap();
    let (without, with) = (
        find("Expanded, central"),
        find("Expanded, central, with roads"),
    );
    assert!(with.hauler_walking_pct < without.hauler_walking_pct);
}

/// A reference player from a new game. Slow, so it's release only.
#[test]
#[ignore]
fn the_normal_route_spends_its_planks_on_roads_and_ends_with_room() {
    let (w, r) = spirit_sim::autoplay::play_to_expansion("normal").unwrap();
    w.check_conservation().unwrap();
    assert_eq!(r.roads.len(), 4);
    let planks = r.consumed.iter().find(|(n, _)| n == "Planks").unwrap().1;
    assert!(planks >= 1200, "{planks}");
    let main = &w.main().storage;
    assert!(main.free() > main.capacity / 3, "{} free", main.free());
}
