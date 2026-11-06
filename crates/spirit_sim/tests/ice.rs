//! The south river in winter: a way across, for anyone who'll risk it.

use spirit_sim::ScenarioKind;
use spirit_sim::grid::TilePos;
use spirit_sim::save::{decode, encode};
use spirit_sim::world::{Command, World};

const ICE: TilePos = TilePos::new(12, 28);
const FAR: TilePos = TilePos::new(12, 31);

fn game() -> World {
    World::new_journey("normal").unwrap()
}

/// Moves the clock to the first minute of a season.
fn to_season(w: &mut World, id: &str) {
    let tpm = w.defs.ticks_per_second as u64 * 60;
    let mut start = 0;
    for s in &w.defs.seasons {
        if s.id == id {
            break;
        }
        start += s.minutes * tpm;
    }
    // The year after, so spring isn't the moment the game began.
    let year: u64 = w.defs.seasons.iter().map(|s| s.minutes * tpm).sum();
    w.tick = year + start;
    w.step();
    assert_eq!(w.season().unwrap().id, id);
}

fn walk(w: &mut World, to: TilePos) {
    w.apply(Command::PlayerInteract(to)).unwrap();
    for _ in 0..4000 {
        if w.player.actor.pos() == to {
            return;
        }
        w.step();
    }
    panic!("never got to {to}: at {}", w.player.actor.pos());
}

/// The last tick of winter, with the thaw about to happen.
fn to_the_thaw(w: &mut World) {
    let tpm = w.defs.ticks_per_second as u64 * 60;
    let spring: u64 = w.defs.seasons.iter().map(|s| s.minutes * tpm).sum();
    w.tick = (w.tick / spring + 1) * spring - 1;
}

#[test]
fn the_river_freezes_in_winter_and_only_then() {
    let mut w = game();
    assert!(!w.grid.is_ice(ICE));
    to_season(&mut w, "winter");
    assert!(w.grid.is_ice(ICE));
    assert!(
        !w.grid.is_walkable(ICE),
        "not as far as a spirit's concerned"
    );
    to_season(&mut w, "spring");
    assert!(!w.grid.is_ice(ICE));
}

#[test]
fn the_ravine_does_not_freeze() {
    let mut w = game();
    to_season(&mut w, "winter");
    assert!(!w.grid.is_ice(TilePos::new(21, 15)));
    assert!(!w.grid.is_ice(TilePos::new(50, 15)), "nor does the lake");
}

#[test]
fn you_can_walk_to_the_far_shore_in_winter() {
    let mut w = game();
    to_season(&mut w, "winter");
    walk(&mut w, FAR);
    w.check_conservation().unwrap();
}

/// The rescue for anyone walled in used to carry you straight back.
#[test]
fn you_can_stay_out_there_for_as_long_as_it_holds() {
    let mut w = game();
    to_season(&mut w, "winter");
    walk(&mut w, FAR);
    w.run_ticks(3000);
    assert_eq!(w.player.actor.pos(), FAR);
    assert!(!w.notices.contains("walled-in"));
}

#[test]
fn standing_on_the_ice_itself_is_allowed_too() {
    let mut w = game();
    to_season(&mut w, "winter");
    walk(&mut w, ICE);
    w.run_ticks(3000);
    assert_eq!(w.player.actor.pos(), ICE);
}

#[test]
fn you_can_walk_home_again() {
    let mut w = game();
    to_season(&mut w, "winter");
    walk(&mut w, FAR);
    walk(&mut w, TilePos::new(12, 26));
    assert!(!w.notices.contains("walled-in"));
}

#[test]
fn the_game_tells_you_the_first_winter() {
    let mut w = game();
    w.run_ticks(200);
    to_season(&mut w, "winter");
    w.run_ticks(300);
    assert!(w.notices.contains("frozen over"));
}

#[test]
fn but_not_in_summer() {
    let mut w = game();
    to_season(&mut w, "summer");
    assert!(w.apply(Command::PlayerInteract(ICE)).is_err());
    let _ = w.apply(Command::PlayerInteract(FAR));
    w.run_ticks(2000);
    assert!(w.player.actor.pos().y < 28);
}

#[test]
fn nobody_walks_on_the_ice_who_does_not_have_to() {
    let mut w = game();
    to_season(&mut w, "winter");
    let mut rows = Vec::new();
    w.apply(Command::PlayerInteract(TilePos::new(8, 26)))
        .unwrap();
    for _ in 0..2000 {
        w.step();
        rows.push(w.player.actor.pos().y);
    }
    assert!(rows.iter().all(|y| *y < 28));
}

#[test]
fn a_spirit_will_not_follow() {
    let mut w = World::new_journey("normal").unwrap();
    to_season(&mut w, "winter");
    let path = w.grid.find_path(TilePos::new(12, 26), |p| p == FAR);
    assert_eq!(
        path, None,
        "spirits look for a way on land, and there isn't one"
    );
}

#[test]
fn the_ice_warns_you_once() {
    let mut w = game();
    to_season(&mut w, "winter");
    walk(&mut w, FAR);
    assert!(!w.notices.contains("groaning"));
    to_the_thaw(&mut w);
    let tpm = w.defs.ticks_per_second as u64 * 60;
    w.tick -= 2 * tpm;
    w.step();
    assert!(w.notices.contains("groaning"));
    let total = w.notices.total;
    w.step();
    assert_eq!(w.notices.total, total);
}

#[test]
fn staying_home_means_no_warning() {
    let mut w = game();
    to_season(&mut w, "winter");
    to_the_thaw(&mut w);
    let tpm = w.defs.ticks_per_second as u64 * 60;
    w.tick -= 2 * tpm;
    w.step();
    assert!(!w.notices.contains("groaning"));
}

#[test]
fn the_thaw_brings_you_home_but_not_your_load() {
    let mut w = game();
    to_season(&mut w, "winter");
    walk(&mut w, FAR);
    let logs = w.defs.resource_id("logs").unwrap();
    w.player.actor.inventory.add(logs, 5);
    *w.ledger.seeded.entry(logs).or_default() += 5;
    to_the_thaw(&mut w);
    w.run_ticks(3);
    assert!(w.player.actor.pos().y < 28, "at {}", w.player.actor.pos());
    assert_eq!(w.player.actor.inventory.total(), 0);
    assert!(w.notices.contains("what you were carrying"));
    w.check_conservation().unwrap();
    w.run_ticks(100);
}

#[test]
fn being_caught_empty_handed_says_so() {
    let mut w = game();
    to_season(&mut w, "winter");
    walk(&mut w, FAR);
    to_the_thaw(&mut w);
    w.run_ticks(3);
    assert!(w.notices.contains("only just got back"));
}

#[test]
fn a_crossing_means_you_can_stay() {
    let mut w = game();
    to_season(&mut w, "winter");
    walk(&mut w, FAR);
    w.grid.open_crossing(TilePos::new(12, 28));
    w.grid.open_crossing(TilePos::new(12, 29));
    to_the_thaw(&mut w);
    w.run_ticks(3);
    assert_eq!(w.player.actor.pos(), FAR);
}

#[test]
fn scenarios_have_no_ice() {
    let mut w = World::from_scenario(ScenarioKind::KilnYard);
    w.tick = 100_000;
    w.step();
    assert!(!w.grid.is_ice(ICE));
}

#[test]
fn a_save_in_winter_loads_with_the_river_frozen() {
    let mut w = game();
    to_season(&mut w, "winter");
    walk(&mut w, TilePos::new(12, 29));
    let (mut loaded, _) = decode(&encode(&w, "new_game")).unwrap();
    assert!(loaded.grid.is_ice(ICE));
    assert_eq!(loaded.player.actor.pos(), TilePos::new(12, 29));
    loaded.run_ticks(20);
    assert_eq!(loaded.state_hash(), {
        w.run_ticks(20);
        w.state_hash()
    });
}
