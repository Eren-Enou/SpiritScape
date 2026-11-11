//! The old carver's cold camp, and the fire you can bring to it.

use spirit_sim::ScenarioKind;
use spirit_sim::grid::TilePos;
use spirit_sim::save::{decode, encode};
use spirit_sim::world::{Command, World};

/// Walks the player up beside the camp, and gives the world time to notice.
fn walk_up(w: &mut World) {
    let at = w.cold_camp_at().unwrap();
    let goal = TilePos::new(at.x - 1, at.y);
    w.apply(Command::PlayerInteract(goal)).unwrap();
    for _ in 0..20_000 {
        w.step();
        if w.player.actor.pos() == goal {
            break;
        }
    }
    assert_eq!(w.player.actor.pos(), goal, "never got there");
    w.run_ticks(10);
}

fn said(w: &World, words: &str) -> usize {
    w.events.iter().filter(|e| e.text.contains(words)).count()
}

#[test]
fn a_new_game_can_walk_to_it() {
    let w = World::new_journey("normal").unwrap();
    let at = w.cold_camp_at().unwrap();
    let start = w.player.actor.pos();
    let path = w.grid.find_path(start, |p| p.manhattan(at) <= 1);
    assert!(
        path.is_some(),
        "no way from {start:?} to the camp at {at:?}"
    );
    let stone = w
        .defs
        .markers
        .iter()
        .find(|m| m.id == "oldgrowth")
        .unwrap()
        .at;
    let near = (stone[0] - at.x).abs() + (stone[1] - at.y).abs();
    assert!(near <= 6, "{near} tiles from the Mossed Stone");
}

#[test]
fn found_before_the_hearth_it_stays_cold() {
    let mut w = World::from_scenario(ScenarioKind::QuietHour);
    walk_up(&mut w);
    assert!(w.settlement.cold_camp.found.is_some());
    assert!(
        !w.cold_camp_lit(),
        "there's no hearth to bring a light from"
    );
    assert_eq!(said(&w, "Nobody ever lit a fire"), 1);
    w.run_ticks(100);
    assert_eq!(said(&w, "Nobody ever lit a fire"), 1, "found twice");
}

#[test]
fn with_the_hearth_lit_you_light_it() {
    let mut w = World::from_scenario(ScenarioKind::ColdCamp);
    assert!(w.hearth().is_some());
    assert!(!w.cold_camp_lit());
    let before = w.ending_story().unwrap().join(" ");
    assert!(!before.contains("lit the fire they never had"), "{before}");
    walk_up(&mut w);
    assert!(w.cold_camp_lit());
    assert_eq!(
        said(&w, "For the first time, there's a fire in the old camp"),
        1
    );
    w.run_ticks(100);
    assert_eq!(said(&w, "For the first time"), 1, "lit twice");
    let after = w.ending_story().unwrap().join(" ");
    assert!(after.contains("lit the fire they never had"), "{after}");
    let notes = w.notes();
    let section = notes.iter().find(|n| n.title == "The cold camp").unwrap();
    assert_eq!(section.lines.len(), 2);
    assert!(
        w.what_happened()
            .iter()
            .any(|l| l.starts_with("The cold camp: lit at"))
    );
}

#[test]
fn it_stays_lit_through_a_save() {
    let mut w = World::from_scenario(ScenarioKind::ColdCamp);
    walk_up(&mut w);
    let back = decode(&encode(&w, "cold_camp")).unwrap().0;
    assert_eq!(back.settlement.cold_camp, w.settlement.cold_camp);
    assert!(back.cold_camp_lit());
}

#[test]
fn nobody_has_found_it_at_the_start() {
    let w = World::from_scenario(ScenarioKind::ColdCamp);
    assert!(w.settlement.cold_camp.found.is_none());
    assert!(
        w.what_happened()
            .iter()
            .any(|l| l == "The cold camp: never found.")
    );
}
