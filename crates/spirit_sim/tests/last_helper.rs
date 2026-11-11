//! The last of the old carver's helpers, still out by the Last Stone.

use spirit_sim::ScenarioKind;
use spirit_sim::grid::TilePos;
use spirit_sim::last_helper::HelperAnswer;
use spirit_sim::save::{decode, encode};
use spirit_sim::world::{Command, World};

fn by_the_stone() -> World {
    World::from_scenario(ScenarioKind::LastStone)
}

/// Walks the player up beside it, and gives it time to notice.
fn walk_up(w: &mut World) {
    let [x, y] = w.defs.last_helper.at;
    let goal = TilePos::new(x - 1, y);
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

fn answer(w: &World) -> Option<HelperAnswer> {
    w.settlement.last_helper.as_ref().map(|h| h.answer)
}

#[test]
fn it_can_only_be_seen_once_the_last_stone_is_read() {
    let mut w = by_the_stone();
    assert!(w.last_helper_seen());
    assert!(w.last_helper_at().is_some());
    w.settlement.markers_read.clear();
    assert!(!w.last_helper_seen());
    assert_eq!(w.last_helper_at(), None);
    let fresh = World::new_journey("normal").unwrap();
    assert_eq!(fresh.last_helper_at(), None);
}

#[test]
fn it_stands_on_open_ground_by_its_stone() {
    let w = by_the_stone();
    let [x, y] = w.defs.last_helper.at;
    assert!(w.grid.is_walkable(TilePos::new(x - 1, y)));
    let stone = w
        .defs
        .markers
        .iter()
        .find(|m| m.id == w.defs.last_helper.after_stone)
        .unwrap();
    let near = (stone.at[0] - x).abs() + (stone.at[1] - y).abs();
    assert!(near <= 4, "{near} tiles from its stone");
}

#[test]
fn walking_up_to_it_asks_a_question_once() {
    let mut w = by_the_stone();
    assert_eq!(w.last_helper_question(), None);
    assert!(w.last_helper_carving());
    walk_up(&mut w);
    assert_eq!(answer(&w), Some(HelperAnswer::Waiting));
    assert!(w.last_helper_question().is_some());
    assert!(!w.last_helper_carving(), "it looks up");
    let found = w
        .events
        .iter()
        .filter(|e| e.text.contains("It stops, and looks up at you"))
        .count();
    assert_eq!(found, 1);
    w.run_ticks(100);
    let found = w
        .events
        .iter()
        .filter(|e| e.text.contains("It stops, and looks up at you"))
        .count();
    assert_eq!(found, 1, "asked twice");
}

#[test]
fn asked_home_it_stands_by_the_fire_and_does_no_work() {
    let mut w = by_the_stone();
    let spirits = w.spirits.len();
    assert!(
        w.apply(Command::AnswerHelper(true)).is_err(),
        "nothing to answer yet"
    );
    walk_up(&mut w);
    w.apply(Command::AnswerHelper(true)).unwrap();
    assert_eq!(answer(&w), Some(HelperAnswer::Came));
    assert_eq!(w.last_helper_question(), None);
    assert!(!w.last_helper_carving());
    // It walks home from its stone, rather than appearing there.
    let [x, y] = w.defs.last_helper.at;
    assert_eq!(w.last_helper_at(), Some(TilePos::new(x, y)));
    assert!(w.last_helper_walking());
    assert_eq!(w.spirits.len(), spirits, "it isn't one of your workers");
    assert!(
        w.apply(Command::AnswerHelper(false)).is_err(),
        "it's for good"
    );
}

#[test]
fn left_where_it_is_it_goes_back_to_carving() {
    let mut w = by_the_stone();
    walk_up(&mut w);
    w.apply(Command::AnswerHelper(false)).unwrap();
    assert_eq!(answer(&w), Some(HelperAnswer::Stayed));
    assert!(w.last_helper_carving());
    let [x, y] = w.defs.last_helper.at;
    assert_eq!(w.last_helper_at(), Some(TilePos::new(x, y)));
    let notes = w.notes();
    let section = notes.iter().find(|n| n.title == "The last helper").unwrap();
    assert!(section.lines.iter().any(|l| l.contains("carving again")));
}

#[test]
fn the_hearth_remembers_what_you_did() {
    for (come, words) in [
        (true, "stood by your hearth"),
        (false, "still out by its stone"),
    ] {
        let mut w = World::from_scenario(ScenarioKind::Midwinter);
        w.settlement.markers_read.push("hollow".into());
        let [x, y] = w.defs.last_helper.at;
        w.player.actor = spirit_sim::entities::Actor::new(
            TilePos::new(x - 1, y),
            w.defs.player.move_ticks,
            w.defs.player.carry_capacity,
        );
        w.run_ticks(10);
        // Met but not yet answered, it's still part of the ending.
        let before = w.ending_story().unwrap().join(" ");
        assert!(before.contains("waiting for an answer"), "{before}");
        assert!(!before.contains(words), "{before}");
        w.apply(Command::AnswerHelper(come)).unwrap();
        let story = w.ending_story().unwrap().join(" ");
        assert!(story.contains(words), "{story}");
        if come {
            let hearth = w.hearth().unwrap();
            assert_eq!(
                w.last_helper_at(),
                Some(TilePos::new(hearth.x + 1, hearth.y + 1))
            );
        }
    }
}

#[test]
fn asked_home_it_crosses_on_the_boat() {
    let mut w = World::from_scenario(ScenarioKind::LastBoat);
    w.settlement.markers_read.push("hollow".into());
    let [x, y] = w.defs.last_helper.at;
    w.player.actor = spirit_sim::entities::Actor::new(
        TilePos::new(x - 1, y),
        w.defs.player.move_ticks,
        w.defs.player.carry_capacity,
    );
    w.run_ticks(10);
    w.apply(Command::AnswerHelper(true)).unwrap();
    w.apply(Command::CallHome).unwrap();
    for _ in 0..20_000 {
        if w.homecoming_finished() {
            break;
        }
        w.step();
    }
    assert!(w.homecoming_finished());
    assert_eq!(w.last_helper_at(), None, "gone with the rest");
    let story = w.ending_story().unwrap().join(" ");
    assert!(story.contains("crossed with them"), "{story}");
}

#[test]
fn what_you_answered_survives_a_save() {
    let mut w = by_the_stone();
    walk_up(&mut w);
    let back = decode(&encode(&w, "last_stone")).unwrap().0;
    assert_eq!(back.settlement.last_helper, w.settlement.last_helper);
    w.apply(Command::AnswerHelper(true)).unwrap();
    let back = decode(&encode(&w, "last_stone")).unwrap().0;
    assert_eq!(answer(&back), Some(HelperAnswer::Came));
}

#[test]
fn asked_home_it_walks_there_slowly_and_stops_by_the_fire() {
    let mut w = by_the_stone();
    walk_up(&mut w);
    w.apply(Command::AnswerHelper(true)).unwrap();
    let start = w.last_helper_at().unwrap();
    let home = w.main().pos;
    let steps = w
        .settlement
        .last_helper
        .as_ref()
        .unwrap()
        .walker
        .as_ref()
        .unwrap()
        .path
        .len();
    assert!(steps > 10, "a real walk: {steps} tiles");
    // Half the player's pace.
    let per_tile = w.defs.player.move_ticks as u64 * 2;
    w.run_ticks(per_tile * 3);
    let after = w.last_helper_at().unwrap();
    assert_eq!(
        start.manhattan(after),
        3,
        "three tiles in that time: {start:?} -> {after:?}"
    );
    for _ in 0..(steps as u64 + 5) {
        w.run_ticks(per_tile);
    }
    assert!(!w.last_helper_walking());
    let end = w.last_helper_at().unwrap();
    assert!(
        end.manhattan(home) <= 2,
        "{end:?} isn't by the fire at {home:?}"
    );
    assert!(w.grid.is_walkable(end) || w.grid.terrain(end).is_some());
    // The walk survives a save part-way.
    let mut w = by_the_stone();
    walk_up(&mut w);
    w.apply(Command::AnswerHelper(true)).unwrap();
    w.run_ticks(per_tile * 4 + 3);
    let back = decode(&encode(&w, "last_stone")).unwrap().0;
    assert_eq!(back.settlement.last_helper, w.settlement.last_helper);
}

#[test]
fn reading_the_last_stone_from_any_side_is_enough_to_meet_it() {
    let probe = World::new_journey("normal").unwrap();
    let stone = probe
        .defs
        .markers
        .iter()
        .find(|m| m.id == probe.defs.last_helper.after_stone)
        .unwrap()
        .at;
    let mut sides = 0;
    for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
        let at = TilePos::new(stone[0] + dx, stone[1] + dy);
        if !probe.grid.is_walkable(at) {
            continue;
        }
        sides += 1;
        let mut w = World::new_journey("normal").unwrap();
        w.player.actor = spirit_sim::entities::Actor::new(
            at,
            w.defs.player.move_ticks,
            w.defs.player.carry_capacity,
        );
        assert!(!w.last_helper_seen());
        w.run_ticks(10);
        assert!(w.last_helper_seen(), "the stone wasn't read from {at:?}");
        assert_eq!(
            answer(&w),
            Some(HelperAnswer::Waiting),
            "read from {at:?} but never met it"
        );
    }
    assert!(sides >= 2, "{sides}");
}
