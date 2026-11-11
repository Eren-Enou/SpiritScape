//! The play report's account of what happened in a game.

use spirit_sim::ScenarioKind;
use spirit_sim::world::{Command, World};

fn said(w: &World) -> String {
    w.what_happened().join("\n")
}

#[test]
fn a_new_game_has_nothing_to_tell_yet() {
    let w = World::new_journey("normal").unwrap();
    let s = said(&w);
    assert!(s.contains("Goals met: 0 of 19."), "{s}");
    assert!(s.contains("Spirits made: none yet."), "{s}");
    assert!(s.contains("Old stones read: 0 of 7."), "{s}");
    assert!(s.contains("The last helper: never seen."), "{s}");
    assert!(s.contains("Ending: not yet."), "{s}");
    assert!(!s.contains("Carved a stone"), "{s}");
}

#[test]
fn it_tells_what_was_made_kept_and_carved() {
    let mut w = World::from_scenario(ScenarioKind::Midwinter);
    w.spirits[0].name = "Fernwhistle".into();
    w.spirits[0].renamed = true;
    w.form_bond(0, 1);
    w.apply(Command::CarveStone(1)).unwrap();
    let s = said(&w);
    assert!(s.contains("Fernwhistle (named by you)"), "{s}");
    assert!(s.contains("Keeping company: Fernwhistle and"), "{s}");
    assert!(s.contains("Ending: the Hearth, lit at 0:00."), "{s}");
    assert!(s.contains("Carved a stone at"), "{s}");
    assert!(s.contains("they look up, if you let them"), "{s}");
}

#[test]
fn it_tells_what_became_of_the_last_helper() {
    let mut w = World::from_scenario(ScenarioKind::LastStone);
    assert!(said(&w).contains("The last helper: seen, never met."));
    let [x, y] = w.defs.last_helper.at;
    w.player.actor = spirit_sim::entities::Actor::new(
        spirit_sim::grid::TilePos::new(x - 1, y),
        w.defs.player.move_ticks,
        w.defs.player.carry_capacity,
    );
    w.run_ticks(10);
    assert!(said(&w).contains("met, and still waiting for an answer"));
    w.apply(Command::AnswerHelper(false)).unwrap();
    assert!(said(&w).contains("The last helper: left by its stone."));
}

#[test]
fn the_play_report_carries_it() {
    let w = World::from_scenario(ScenarioKind::Midwinter);
    let report = w.play_report();
    assert!(report.contains("\nWhat happened\n"), "{report}");
    assert!(report.contains("  Ending: the Hearth"), "{report}");
}

#[test]
fn after_the_boat_it_says_how_many_crossed() {
    let mut w = World::from_scenario(ScenarioKind::LastBoat);
    w.apply(Command::CallHome).unwrap();
    assert!(said(&w).contains("still boarding"));
    for _ in 0..20_000 {
        if w.homecoming_finished() {
            break;
        }
        w.step();
    }
    let s = said(&w);
    assert!(s.contains("Ending: the Still Boat, called at"), "{s}");
    assert!(s.contains("crossed."), "{s}");
}
