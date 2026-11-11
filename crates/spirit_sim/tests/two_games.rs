//! Two real games in a row: the first carves a stone at its end, and the
//! second finds it. Each piece has its own tests. This plays them as one
//! story, the way a person would, with the reference player. Release only:
//!   cargo test --release -p spirit_sim --test two_games -- --ignored

use spirit_sim::autoplay::play_all_goals;
use spirit_sim::grid::TilePos;
use spirit_sim::world::{Command, World};

/// Walks the player up beside `at`, and gives the world time to notice.
fn walk_up_to(w: &mut World, at: [i32; 2]) {
    let goal = TilePos::new(at[0], at[1] + 1);
    w.apply(Command::PlayerInteract(goal)).unwrap();
    for _ in 0..20_000 {
        w.step();
        if w.player.actor.pos() == goal {
            break;
        }
    }
    assert_eq!(w.player.actor.pos(), goal, "never got to {at:?}");
    w.run_ticks(10);
}

#[test]
#[ignore = "plays a whole game; run in release"]
fn a_stone_from_one_real_game_is_found_in_the_next() {
    // The first game, played to the Hearth.
    let (mut first, report) = play_all_goals(20).unwrap();
    assert!(report.missed.is_empty(), "{:?}", report.missed);
    assert!(first.journey.is_some(), "a real game");
    assert!(first.can_carve_stone(), "the Hearth is lit, so it's over");
    first.apply(Command::CarveStone(2)).unwrap();
    let stone = first.settlement.your_stone.clone().unwrap();
    assert!(stone.lines[0].starts_with("I gave "), "{:?}", stone.lines);
    assert!(
        stone.lines[0].contains("We built a hearth"),
        "{:?}",
        stone.lines
    );
    let ending = first.ending_story().unwrap().join(" ");
    assert!(ending.contains("You left a stone of your own"), "{ending}");

    // The next game, which the game window gives what the first one kept.
    let mut second = World::new_journey("normal").unwrap();
    second.leave_from_before(vec![stone.lines.clone()]);
    let left = second.settlement.left_before.clone();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].lines, stone.lines);
    assert!(!second.found_one_from_before());
    walk_up_to(&mut second, left[0].at);
    assert!(
        second.found_one_from_before(),
        "walked up and didn't find it"
    );
    let said = second
        .events
        .iter()
        .find(|e| e.text.contains("You left this stone, last time"))
        .map(|e| e.text.clone())
        .unwrap();
    assert!(said.contains(&stone.lines[0]), "{said}");
    let notes = second.notes();
    let section = notes
        .iter()
        .find(|n| n.title == "What you left last time")
        .unwrap();
    assert_eq!(section.lines, stone.lines);
}

#[test]
#[ignore = "plays a whole game; run in release"]
fn the_reference_game_leaves_the_old_stones_story_in_one_piece() {
    let (w, _) = play_all_goals(20).unwrap();
    // Whether the reference player has read the Last Stone and met the last
    // helper says nothing about a person, but whatever happened must hang
    // together: it can only be waiting, or answered, if it can be seen.
    if w.settlement.last_helper.is_some() {
        assert!(w.last_helper_seen());
    }
    if w.last_helper_seen() {
        assert!(w.last_helper_at().is_some());
    }
    // The ending reads in one piece, with no placeholder showing.
    let ending = w.ending_story().unwrap().join(" ");
    assert!(!ending.contains('{'), "{ending}");
    assert!(!ending.contains('}'), "{ending}");
}
