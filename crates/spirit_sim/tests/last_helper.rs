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
    let home = w.main().pos;
    assert_eq!(
        w.last_helper_at(),
        Some(TilePos::new(home.x + 1, home.y + 1))
    );
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
        assert!(!w.ending_story().unwrap().join(" ").contains("old helpers"));
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
