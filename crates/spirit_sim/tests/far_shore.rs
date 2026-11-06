//! The Far Shore: somewhere the spirits won't go.

use spirit_sim::ScenarioKind;
use spirit_sim::grid::TilePos;
use spirit_sim::save::{SAVE_VERSION, SaveStore, Slot, decode, encode};
use spirit_sim::world::{Command, World};

const NEAR: TilePos = TilePos::new(12, 27);
const FAR: TilePos = TilePos::new(12, 30);

fn fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    String::from_utf8(std::fs::read(path).unwrap()).unwrap()
}

fn open(w: &mut World) {
    w.grid.open_crossing(TilePos::new(12, 28));
    w.grid.open_crossing(TilePos::new(12, 29));
}

fn reachable(w: &World) -> bool {
    w.grid.distance_field(&[NEAR]).get(FAR).is_some()
}

fn stilltrees(w: &World) -> usize {
    let at = w
        .defs
        .trees
        .iter()
        .position(|t| t.id == "stilltree")
        .unwrap();
    w.trees.iter().filter(|t| t.kind.index() == at).count()
}

#[test]
fn the_shore_is_shut_until_the_crossing_is_built() {
    let mut w = World::from_scenario(ScenarioKind::MasonYard);
    assert!(!reachable(&w));
    assert_eq!(stilltrees(&w), 10);
    open(&mut w);
    assert!(reachable(&w));
}

#[test]
fn the_crossing_needs_keystones_so_masonry_comes_first() {
    let w = World::from_scenario(ScenarioKind::ExpandedCentral);
    let kind = w.defs.building_kind_id("south_crossing").unwrap();
    assert!(!w.plannable().contains(&kind), "no yard yet");
    let keys = w.defs.resource_id("keystones").unwrap();
    assert!(w.defs.building(kind).cost.iter().any(|c| c.0 == keys));
}

#[test]
fn no_spirit_will_cut_a_stilltree() {
    let mut w = World::from_scenario(ScenarioKind::ExpandedCentral);
    open(&mut w);
    let stillwood = w.defs.resource_id("stillwood").unwrap();
    let at = w
        .defs
        .trees
        .iter()
        .position(|t| t.id == "stilltree")
        .unwrap();
    let kind = w.trees.iter().find(|t| t.kind.index() == at).unwrap().kind;
    for s in &w.spirits {
        assert!(!w.spirit_can_cut_for_test(s, kind), "{}", s.name);
    }
    w.run_ticks(3000);
    assert_eq!(w.ledger.harvested.get(&stillwood).copied().unwrap_or(0), 0);
}

#[test]
fn the_player_can_cut_one_at_woodcutting_twelve() {
    let mut w = World::new_journey("development").unwrap();
    open(&mut w);
    let at = w
        .defs
        .trees
        .iter()
        .position(|t| t.id == "stilltree")
        .unwrap();
    let kind = w.trees.iter().find(|t| t.kind.index() == at).unwrap().kind;
    assert!(!w.player_can_cut(kind));
    assert_eq!(w.defs.tree(kind).level_required, 12);
}

#[test]
fn every_tile_south_of_the_river_stays_off_the_mainland_until_it_opens() {
    let w = World::from_scenario(ScenarioKind::PlankSurplus);
    let land = w.grid.mainland();
    assert!(w.grid.on_land(&land, NEAR));
    assert!(!w.grid.on_land(&land, FAR));
}

#[test]
fn nobody_on_the_mainland_is_mistaken_for_stranded() {
    let mut w = World::from_scenario(ScenarioKind::PlankSurplus);
    w.run_ticks(300);
    assert!(!w.notices.contains("climbed out"));
}

#[test]
fn older_saves_grow_south_and_keep_what_they_had() {
    for name in [
        "v8_yard.save",
        "v8_journey.save",
        "v7_panels.save",
        "v5_crossing.save",
        "v1_journey.save",
    ] {
        let (mut w, _) = decode(&fixture(name)).unwrap();
        assert_eq!(w.grid.height, 40, "{name}");
        assert_eq!(stilltrees(&w), 10, "{name}");
        let at: Vec<_> = w.trees.iter().map(|t| t.pos).collect();
        let mut unique = at.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(at.len(), unique.len(), "{name}: nothing doubled");
        w.run_ticks(600);
        w.check_conservation().unwrap();
    }
}

#[test]
fn a_v0_31_yard_carries_on_after_upgrading() {
    let (mut w, meta) = decode(&fixture("v8_yard.save")).unwrap();
    assert_eq!(meta.version, 8);
    let dressed = w.defs.resource_id("dressed_stone").unwrap();
    w.apply(Command::SetOrderTarget {
        output: dressed,
        target: 40,
    })
    .unwrap();
    let before = w.ledger.crafted.get(&dressed).copied().unwrap_or(0);
    w.run_ticks(1500);
    assert!(w.ledger.crafted.get(&dressed).copied().unwrap_or(0) > before);
}

#[test]
fn an_upgraded_save_keeps_its_original() {
    let dir = std::env::temp_dir().join(format!("spiritscape_v32_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("world.save"), fixture("v8_yard.save")).unwrap();
    let store = SaveStore::new(&dir);
    let loaded = store.load(Slot::Manual).unwrap();
    assert!(loaded.warning.unwrap().contains("world.save.v8"));
    store
        .write(Slot::Manual, &loaded.world, "mason_yard")
        .unwrap();
    assert_eq!(store.meta(Slot::Manual).unwrap().version, SAVE_VERSION);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_carry_bonus_from_the_lantern_survives_saving() {
    let mut w = World::from_scenario(ScenarioKind::MasonYard);
    w.settlement.carry_bonus = 2;
    let (loaded, _) = decode(&encode(&w, "mason_yard")).unwrap();
    assert_eq!(loaded.settlement.carry_bonus, 2);
    w.step();
}
