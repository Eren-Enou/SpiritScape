//! Nobody stays walled in, whatever an older save holds.

use spirit_sim::ScenarioKind;
use spirit_sim::defs::JobKind;
use spirit_sim::grid::{Grid, MapLayout, TilePos};
use spirit_sim::save::{decode, encode};
use spirit_sim::world::{Command, World};

const POCKET: TilePos = TilePos::new(6, 16);

fn builder(w: &World) -> usize {
    let found = w.spirits.iter().position(|s| {
        let role = w.defs.role(s.role);
        role.has_job(JobKind::Construct)
    });
    found.unwrap()
}

fn is_free(w: &World, p: TilePos) -> bool {
    w.grid.on_land(&w.grid.mainland(), p)
}

#[test]
fn the_mainland_is_the_largest_stretch_of_open_ground() {
    let g = Grid::new(&MapLayout::parse("..#....\n..#....\n").unwrap());
    let land = g.mainland();
    assert!(g.on_land(&land, TilePos::new(3, 0)));
    assert!(!g.on_land(&land, TilePos::new(0, 0)));
    assert_eq!(
        g.nearest_land(&land, TilePos::new(1, 0)),
        Some(TilePos::new(3, 0))
    );
}

#[test]
fn every_scenario_but_this_one_starts_with_everyone_on_open_ground() {
    for kind in ScenarioKind::ALL {
        let w = World::from_scenario(kind);
        let land = w.grid.mainland();
        let cut_off = w
            .spirits
            .iter()
            .filter(|s| !w.grid.on_land(&land, s.actor.pos()))
            .count();
        let expected = usize::from(kind == ScenarioKind::WalledIn);
        assert_eq!(cut_off, expected, "{}", kind.key());
        assert!(
            w.grid.on_land(&land, w.player.actor.pos()),
            "{}",
            kind.key()
        );
    }
}

#[test]
fn a_walled_in_builder_climbs_out_and_stays_on_open_ground() {
    let mut w = World::from_scenario(ScenarioKind::WalledIn);
    let b = builder(&w);
    assert_eq!(w.spirits[b].actor.pos(), POCKET);
    w.run_ticks(101);
    let at = w.spirits[b].actor.pos();
    assert_ne!(at, POCKET);
    assert!(is_free(&w, at));
    assert!(w.notices.contains("climbed out"));
    w.check_conservation().unwrap();
}

#[test]
fn a_rescued_builder_can_finish_a_project() {
    let mut w = World::from_scenario(ScenarioKind::WalledIn);
    let kind = w.defs.building_kind_id("storehouse").unwrap();
    w.apply(Command::QueueBuilding { kind, site: None })
        .unwrap();
    let mut done = false;
    for _ in 0..3000 {
        w.step();
        if w.buildings
            .iter()
            .any(|x| x.kind == kind && x.is_complete())
        {
            done = true;
            break;
        }
    }
    assert!(done, "{:?}", w.active_project().map(|p| p.blocked.clone()));
}

#[test]
fn what_it_was_carrying_stays_with_it() {
    let mut w = World::from_scenario(ScenarioKind::WalledIn);
    let b = builder(&w);
    let logs = w.defs.resource_id("logs").unwrap();
    let main = w.main_storage;
    let taken = w.buildings[main].storage.withdraw_available(logs, 5);
    w.spirits[b].actor.inventory.add(logs, taken);
    w.run_ticks(10);
    assert_eq!(w.spirits[b].actor.inventory.get(logs), 5);
    w.check_conservation().unwrap();
}

#[test]
fn a_walled_in_player_climbs_out_too() {
    let mut w = World::from_scenario(ScenarioKind::WalledIn);
    w.player.actor.mover.pos = POCKET;
    w.run_ticks(101);
    assert_ne!(w.player.actor.pos(), POCKET);
    assert!(w.notices.contains("You climbed out"));
}

#[test]
fn a_save_with_someone_walled_in_recovers_after_loading() {
    let w = World::from_scenario(ScenarioKind::WalledIn);
    let (mut loaded, _) = decode(&encode(&w, "walled_in")).unwrap();
    loaded.run_ticks(101);
    assert!(is_free(
        &loaded,
        loaded.spirits[builder(&loaded)].actor.pos()
    ));
}

#[test]
fn nobody_on_open_ground_is_moved() {
    let mut w = World::from_scenario(ScenarioKind::PlankSurplus);
    w.run_ticks(1500);
    assert!(!w.notices.contains("climbed out"));
}

#[test]
fn the_grove_is_not_mistaken_for_home_before_the_crossing() {
    let w = World::from_scenario(ScenarioKind::ForestAccess);
    let land = w.grid.mainland();
    assert!(!w.grid.on_land(&land, TilePos::new(55, 5)));
    assert!(w.grid.on_land(&land, TilePos::new(6, 15)));
}
