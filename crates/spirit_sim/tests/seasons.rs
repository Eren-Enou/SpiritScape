//! Seasons: what's worth doing changes through the year.

use spirit_sim::ScenarioKind;
use spirit_sim::entities::TreeState;
use spirit_sim::save::{decode, encode};
use spirit_sim::world::World;

fn game() -> World {
    World::new_journey("normal").unwrap()
}

/// Runs to the first tick of a season.
fn run_to(w: &mut World, id: &str) {
    for _ in 0..200_000 {
        if w.season().is_some_and(|s| s.id == id) {
            return;
        }
        w.step();
    }
    panic!("never reached {id}");
}

fn cut(w: &mut World, kind: &str) -> (usize, u64) {
    let t = w
        .trees
        .iter()
        .position(|t| w.defs.tree(t.kind).id == kind)
        .unwrap();
    let at = w.tick + 1000;
    w.trees[t].state = TreeState::Depleted { regrow_at: at };
    (t, at)
}

fn due(w: &World, t: usize) -> Option<u64> {
    match w.trees[t].state {
        TreeState::Depleted { regrow_at } => Some(regrow_at),
        TreeState::Grown { .. } => None,
    }
}

#[test]
fn scenarios_have_no_seasons_so_their_results_stand() {
    let mut w = World::from_scenario(ScenarioKind::ExpandedRemote);
    assert!(w.season().is_none());
    w.run_ticks(100);
    assert!(w.settlement.season_seen.is_none());
}

#[test]
fn a_game_starts_in_spring_and_the_year_goes_round() {
    let mut w = game();
    assert_eq!(w.season().unwrap().id, "spring");
    let tpm = w.defs.ticks_per_second as u64 * 60;
    w.tick = 30 * tpm;
    assert_eq!(w.season().unwrap().id, "summer");
    w.tick = 90 * tpm;
    assert_eq!(w.season().unwrap().id, "winter");
    w.tick = 120 * tpm;
    assert_eq!(w.season().unwrap().id, "spring");
}

#[test]
fn spring_grows_trees_back_twice_as_fast() {
    let mut w = game();
    let (t, at) = cut(&mut w, "tree");
    w.run_ticks(100);
    assert_eq!(due(&w, t), Some(at - 100));
}

#[test]
fn winter_stops_the_woods() {
    let mut w = game();
    run_to(&mut w, "winter");
    let (t, at) = cut(&mut w, "oak");
    w.run_ticks(100);
    assert_eq!(due(&w, t), Some(at + 100));
}

#[test]
fn the_quarry_does_not_care_about_winter() {
    let mut w = game();
    run_to(&mut w, "winter");
    let (t, at) = cut(&mut w, "stone_outcrop");
    w.run_ticks(100);
    assert_eq!(due(&w, t), Some(at));
}

#[test]
fn summer_changes_nothing() {
    let mut w = game();
    run_to(&mut w, "summer");
    let (t, at) = cut(&mut w, "tree");
    w.run_ticks(100);
    assert_eq!(due(&w, t), Some(at));
}

#[test]
fn a_turn_of_the_season_is_announced_once() {
    let mut w = game();
    run_to(&mut w, "summer");
    w.run_ticks(5);
    let count = w
        .notices
        .iter()
        .filter(|n| n.text.starts_with("Summer."))
        .count();
    assert_eq!(count, 1);
    assert!(!w.notices.contains("Spring."), "nothing at the very start");
}

#[test]
fn winter_trees_grow_again_in_spring() {
    let mut w = game();
    run_to(&mut w, "winter");
    let (t, _) = cut(&mut w, "tree");
    run_to(&mut w, "spring");
    w.run_ticks(1200);
    assert!(w.trees[t].is_grown());
}

#[test]
fn a_game_saved_in_winter_carries_on_the_same() {
    let mut w = game();
    run_to(&mut w, "winter");
    w.run_ticks(100);
    let (mut loaded, _) = decode(&encode(&w, "new_game")).unwrap();
    assert_eq!(loaded.season().unwrap().id, "winter");
    w.run_ticks(600);
    loaded.run_ticks(600);
    assert_eq!(w.state_hash(), loaded.state_hash());
}
