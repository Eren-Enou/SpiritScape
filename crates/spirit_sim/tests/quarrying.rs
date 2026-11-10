//! Quarrying: a second gathering skill, and the causeway it pays for.

use spirit_sim::ScenarioKind;
use spirit_sim::autoplay::{QUARRY_ACROSS, QUARRY_HOME, stone_time, walk_across};
use spirit_sim::defs::{JobKind, ResourceId, SkillId};
use spirit_sim::grid::{Terrain, TilePos};
use spirit_sim::save::{SAVE_VERSION, SaveStore, Slot, decode, encode};
use spirit_sim::world::{Command, World};

const OUTCROP: TilePos = TilePos::new(4, 22);

fn world() -> World {
    World::from_scenario(ScenarioKind::QuarryStart)
}

fn res(w: &World, id: &str) -> ResourceId {
    w.defs.resource_id(id).unwrap()
}

fn quarrying(w: &World) -> SkillId {
    w.defs.skill_id("quarrying").unwrap()
}

fn plan_causeway(w: &mut World) {
    let kind = w.defs.building_kind_id("stone_causeway").unwrap();
    w.apply(Command::QueueBuilding { kind, site: None })
        .unwrap();
}

fn causeway_built(w: &World) -> bool {
    let kind = w.defs.building_kind_id("stone_causeway").unwrap();
    w.buildings
        .iter()
        .any(|b| b.kind == kind && b.is_complete())
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

fn outcrops(w: &World) -> usize {
    let site = w.defs.site_id("south_quarry").unwrap();
    w.trees.iter().filter(|t| t.site == Some(site)).count()
}

#[test]
fn the_quarry_is_on_the_map_and_reachable_from_home() {
    let w = world();
    assert_eq!(outcrops(&w), 6);
    let field = w.grid.distance_field(&[QUARRY_HOME]);
    assert!(w.grid.approach_distance(&field, OUTCROP).unwrap() < 12);
}

#[test]
fn the_player_cuts_stone_and_earns_quarrying_xp() {
    let mut w = world();
    let (stone, skill) = (res(&w, "stone"), quarrying(&w));
    let before = w.player.xp(skill);
    w.apply(Command::PlayerInteract(OUTCROP)).unwrap();
    assert!(run_until(&mut w, 900, |w| w
        .player
        .actor
        .inventory
        .get(stone)
        >= 2));
    assert_eq!(w.player.xp(skill), before + 36);
}

#[test]
fn woodcutting_xp_does_not_move_when_cutting_stone() {
    let mut w = world();
    let wc = w.defs.skill_id("woodcutting").unwrap();
    let before = w.player.xp(wc);
    w.apply(Command::PlayerInteract(OUTCROP)).unwrap();
    w.run_ticks(300);
    assert_eq!(w.player.xp(wc), before);
}

#[test]
fn soul_separation_makes_a_quarrying_spirit() {
    let mut w = world();
    let skill = quarrying(&w);
    let count = w.spirits.len();
    w.apply(Command::SoulSeparate { skill, xp: 174 }).unwrap();
    assert_eq!(w.spirits.len(), count + 1);
    let s = w.spirits.last().unwrap();
    assert_eq!(w.defs.role(s.role).id, "quarrier");
}

#[test]
fn a_quarrying_spirit_rests_until_stone_is_wanted() {
    let mut w = world();
    let stone = res(&w, "stone");
    w.run_ticks(600);
    assert_eq!(w.ledger.harvested.get(&stone).copied().unwrap_or(0), 0);
}

#[test]
fn a_plan_sends_the_spirit_to_the_quarry_and_opens_the_river() {
    let mut w = world();
    assert!(!w.grid.is_walkable(TilePos::new(21, 20)));
    plan_causeway(&mut w);
    assert!(run_until(&mut w, 12000, causeway_built));
    assert_eq!(w.grid.terrain(TilePos::new(21, 20)), Some(Terrain::Bridge));
    assert_eq!(w.grid.terrain(TilePos::new(22, 20)), Some(Terrain::Bridge));
    let stone = res(&w, "stone");
    assert_eq!(w.ledger.consumed.get(&stone).copied(), Some(60));
}

#[test]
fn stone_stops_coming_once_the_causeway_is_paid_for() {
    let mut w = world();
    plan_causeway(&mut w);
    assert!(run_until(&mut w, 12000, causeway_built));
    let stone = res(&w, "stone");
    let cut = w.ledger.harvested.get(&stone).copied();
    w.run_ticks(600);
    let later = w.ledger.harvested.get(&stone).copied();
    assert!(later.unwrap() <= cut.unwrap() + 10);
}

#[test]
fn woodcutters_leave_the_outcrops_alone() {
    let mut w = World::from_scenario(ScenarioKind::Demo);
    let stone = res(&w, "stone");
    w.run_ticks(3000);
    assert_eq!(w.ledger.harvested.get(&stone).copied().unwrap_or(0), 0);
}

#[test]
fn a_plan_says_nobody_can_gather_stone_without_a_spirit() {
    let mut w = World::from_scenario(ScenarioKind::PlankSurplus);
    let spare = w.spirits.iter().any(|s| {
        w.defs.role(s.role).has_job(JobKind::Gather) && w.defs.role(s.role).id == "quarrier"
    });
    assert!(!spare);
    plan_causeway(&mut w);
    let view = w.plan_views();
    let line = view[0].needs.iter().find(|n| n.name == "Stone").unwrap();
    assert!(line.status.contains("nobody can gather"), "{}", line.status);
}

#[test]
fn the_causeway_shortens_the_walk_to_the_distant_forest() {
    let (before, after) = (walk_across(false), walk_across(true));
    assert!(after * 10.0 < before * 8.0, "{after} against {before}");
    let w = world();
    assert_eq!(w.grid.terrain(QUARRY_ACROSS), Some(Terrain::Grass));
}

#[test]
fn a_spirit_builds_the_causeway_alone_and_the_player_can_speed_it_up() {
    let (helped, alone) = (stone_time(false).unwrap(), stone_time(true).unwrap());
    assert!(helped < alone, "{helped} against {alone}");
}

#[test]
fn stone_survives_saving_and_continues_identically() {
    let mut w = world();
    plan_causeway(&mut w);
    w.run_ticks(1200);
    let (mut loaded, _) = decode(&encode(&w, "quarry_start")).unwrap();
    w.run_ticks(1200);
    loaded.run_ticks(1200);
    assert_eq!(w.state_hash(), loaded.state_hash());
}

#[test]
fn older_saves_gain_the_quarry_and_keep_what_they_had() {
    for name in [
        "v7_panels.save",
        "v6_road.save",
        "v5_crossing.save",
        "v4_outpost.save",
        "v1_journey.save",
    ] {
        let (mut w, _) = decode(&fixture(name)).unwrap();
        assert_eq!(outcrops(&w), 6, "{name}");
        let at: Vec<_> = w.trees.iter().map(|t| t.pos).collect();
        let mut unique = at.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(at.len(), unique.len(), "{name}: nothing doubled");
        assert_eq!(w.player.xp(quarrying(&w)), 0, "{name}");
        w.run_ticks(600);
        w.check_conservation().unwrap();
    }
}

#[test]
fn an_upgraded_save_can_build_the_causeway_and_keeps_its_original() {
    let dir = std::env::temp_dir().join(format!("spiritscape_v15_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("world.save"), fixture("v7_panels.save")).unwrap();
    let store = SaveStore::new(&dir);
    let loaded = store.load(Slot::Manual).unwrap();
    assert!(loaded.warning.unwrap().contains("world.save.v7"));
    let mut w = loaded.world;
    plan_causeway(&mut w);
    assert!(!w.plan_views().is_empty());
    store.write(Slot::Manual, &w, "panel_workshop").unwrap();
    assert_eq!(store.meta(Slot::Manual).unwrap().version, SAVE_VERSION);
    let _ = std::fs::remove_dir_all(&dir);
}
