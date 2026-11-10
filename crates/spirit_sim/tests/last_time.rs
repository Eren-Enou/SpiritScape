//! A stone carved in one game, found in the next.

use spirit_sim::ScenarioKind;
use spirit_sim::grid::TilePos;
use spirit_sim::save::{decode, encode};
use spirit_sim::world::{Command, World};

fn words(n: u32) -> Vec<String> {
    vec![
        format!("I gave {n} XP of myself."),
        "Whoever finds this: I'm still here.".into(),
    ]
}

/// Walks the player up to `at`, and gives the world time to notice.
fn walk_to(w: &mut World, at: [i32; 2]) {
    let goal = TilePos::new(at[0], at[1] + 1);
    w.apply(Command::PlayerInteract(goal)).unwrap();
    for _ in 0..3000 {
        w.step();
        if w.player.actor.pos() == goal {
            break;
        }
    }
    assert_eq!(w.player.actor.pos(), goal, "never got there");
    w.run_ticks(10);
}

#[test]
fn a_new_game_has_none_unless_some_were_kept() {
    let w = World::new_journey("normal").unwrap();
    assert!(w.settlement.left_before.is_empty());
    assert!(!w.found_one_from_before());
}

#[test]
fn stones_stand_by_the_first_old_stone_newest_first() {
    let mut w = World::new_journey("normal").unwrap();
    w.leave_from_before(vec![words(3), words(2), words(1), words(0)]);
    let spots = w.defs.your_stone.left_spots.clone();
    assert_eq!(w.settlement.left_before.len(), spots.len());
    assert_eq!(w.settlement.left_before[0].lines, words(3));
    assert_eq!(w.settlement.left_before[0].at, spots[0]);
    let first = w.defs.markers.iter().find(|m| m.id == "first").unwrap();
    for s in &w.settlement.left_before {
        let near = (s.at[0] - first.at[0]).abs() + (s.at[1] - first.at[1]).abs();
        assert!(near <= 3, "{:?} is far from the first stone", s.at);
        assert!(
            w.grid.is_walkable(TilePos::new(s.at[0], s.at[1] + 1)),
            "{:?}",
            s.at
        );
    }
    // Only once: a second call, as loading might make, changes nothing.
    w.leave_from_before(vec![words(9)]);
    assert_eq!(w.settlement.left_before[0].lines, words(3));
}

#[test]
fn whatever_the_file_held_comes_out_short_and_tidy() {
    let mut w = World::new_journey("normal").unwrap();
    let long = "x".repeat(500);
    let mut many: Vec<String> = (0..10).map(|i| format!("  line {i}  ")).collect();
    many.insert(1, String::new());
    many.insert(2, long);
    w.leave_from_before(vec![vec![String::new(), " ".into()], many]);
    let left = &w.settlement.left_before;
    assert_eq!(left.len(), 1, "an empty stone isn't a stone");
    assert_eq!(left[0].lines.len(), 6);
    assert_eq!(left[0].lines[0], "line 0");
    assert_eq!(left[0].lines[1].len(), 200);
}

#[test]
fn walking_up_to_one_finds_it_once() {
    let mut w = World::from_scenario(ScenarioKind::LastTime);
    assert_eq!(w.settlement.left_before.len(), 1);
    let at = w.settlement.left_before[0].at;
    walk_to(&mut w, at);
    assert!(w.found_one_from_before());
    let said = w
        .events
        .iter()
        .filter(|e| e.text.contains("You left this stone, last time"))
        .count();
    assert_eq!(said, 1);
    w.run_ticks(50);
    let said = w
        .events
        .iter()
        .filter(|e| e.text.contains("You left this stone, last time"))
        .count();
    assert_eq!(said, 1, "found twice");
    let notes = w.notes();
    let section = notes
        .iter()
        .find(|n| n.title == "What you left last time")
        .unwrap();
    assert_eq!(section.lines, w.settlement.left_before[0].lines);
}

#[test]
fn a_stone_that_has_not_been_found_is_not_in_the_notes() {
    let w = World::from_scenario(ScenarioKind::LastTime);
    assert!(
        !w.notes()
            .iter()
            .any(|n| n.title == "What you left last time")
    );
}

#[test]
fn finding_one_shows_in_your_own_stone_and_the_ending() {
    let mut w = World::from_scenario(ScenarioKind::Midwinter);
    w.leave_from_before(vec![words(5)]);
    let before = w.stone_words(0).unwrap();
    let at = w.settlement.left_before[0].at;
    walk_to(&mut w, at);
    let after = w.stone_words(0).unwrap();
    assert_eq!(after.len(), before.len() + 1);
    assert!(
        after
            .iter()
            .any(|l| l.contains("I found the one I left before"))
    );
    let ending = w.ending_story().unwrap().join(" ");
    assert!(
        ending.contains("You found the stone you left last time"),
        "{ending}"
    );
}

#[test]
fn found_or_not_survives_a_save() {
    let mut w = World::from_scenario(ScenarioKind::LastTime);
    let back = decode(&encode(&w, "last_time")).unwrap().0;
    assert_eq!(back.settlement.left_before, w.settlement.left_before);
    let at = w.settlement.left_before[0].at;
    walk_to(&mut w, at);
    let back = decode(&encode(&w, "last_time")).unwrap().0;
    assert!(back.found_one_from_before());
}
