//! A stone of your own, carved once it's over.

use spirit_sim::ScenarioKind;
use spirit_sim::save::{decode, encode};
use spirit_sim::world::{Command, World};

/// The Hearth is lit here, so the game has already ended.
fn lit() -> World {
    World::from_scenario(ScenarioKind::Midwinter)
}

fn ending(w: &World) -> String {
    w.ending_story().unwrap().join(" ")
}

#[test]
fn there_is_nothing_to_carve_before_it_is_over() {
    for kind in [
        ScenarioKind::QuietHour,
        ScenarioKind::LastStage,
        ScenarioKind::LastBoat,
    ] {
        let mut w = World::from_scenario(kind);
        assert!(!w.can_carve_stone(), "{kind:?}");
        assert!(w.apply(Command::CarveStone(0)).is_err(), "{kind:?}");
        assert!(w.settlement.your_stone.is_none());
    }
}

#[test]
fn after_the_hearth_you_can_carve_one_stone_where_you_stand() {
    let mut w = lit();
    assert!(w.can_carve_stone());
    let at = w.player.actor.pos();
    w.apply(Command::CarveStone(1)).unwrap();
    let stone = w.settlement.your_stone.clone().unwrap();
    assert_eq!(stone.at, [at.x, at.y]);
    assert_eq!(stone.choice, 1);
    assert!(
        stone.lines[0].contains("We built a hearth"),
        "{:?}",
        stone.lines
    );
    let last = &w.defs.your_stone.choices[1].text;
    assert_eq!(stone.lines.last(), Some(last));
    assert!(!w.can_carve_stone());
    assert!(w.apply(Command::CarveStone(0)).is_err());
    assert_eq!(w.settlement.your_stone, Some(stone));
}

#[test]
fn what_it_says_comes_from_what_you_did() {
    let mut w = lit();
    let made: Vec<u64> = w
        .spirits
        .iter()
        .map(|s| s.gift_xp)
        .filter(|x| *x > 0)
        .collect();
    let xp: u64 = made.iter().sum();
    let first = &w.stone_words(0).unwrap()[0];
    assert!(
        first.contains(&format!(
            "I gave {xp} XP of myself to make {} helpers",
            made.len()
        )),
        "{first}"
    );
    // No companions and no old stones read: just the first line and the last.
    assert_eq!(w.stone_words(0).unwrap().len(), 2);
    w.form_bond(0, 1);
    let (a, b) = (w.spirits[0].name.clone(), w.spirits[1].name.clone());
    let words = w.stone_words(0).unwrap();
    assert_eq!(
        words[1],
        format!("{a} and {b} kept each other company. I saw it.")
    );
    w.settlement.markers_read = w.defs.markers.iter().map(|m| m.id.clone()).collect();
    let words = w.stone_words(2).unwrap();
    assert_eq!(words.len(), 4, "{words:?}");
    assert!(words[2].contains("I read every stone"));
    assert!(w.stone_words(99).is_none());
}

#[test]
fn a_line_that_does_not_exist_carves_nothing() {
    let mut w = lit();
    assert!(w.apply(Command::CarveStone(99)).is_err());
    assert!(w.settlement.your_stone.is_none());
    assert!(w.can_carve_stone());
}

#[test]
fn both_endings_and_the_notes_remember_it() {
    let mut w = lit();
    assert!(!ending(&w).contains("stone of your own"));
    assert!(!w.notes().iter().any(|n| n.title == "Your stone"));
    w.apply(Command::CarveStone(2)).unwrap();
    assert!(ending(&w).contains("You left a stone of your own"));
    let notes = w.notes();
    let section = notes.iter().find(|n| n.title == "Your stone").unwrap();
    assert_eq!(
        section.lines,
        w.settlement.your_stone.as_ref().unwrap().lines
    );
}

#[test]
fn after_the_boat_it_says_you_took_it_back() {
    let mut w = World::from_scenario(ScenarioKind::LastBoat);
    w.apply(Command::CallHome).unwrap();
    assert!(!w.can_carve_stone(), "not while they're still boarding");
    for _ in 0..20_000 {
        if w.homecoming_finished() {
            break;
        }
        w.step();
    }
    assert!(w.homecoming_finished());
    w.apply(Command::CarveStone(0)).unwrap();
    let lines = &w.settlement.your_stone.as_ref().unwrap().lines;
    assert!(
        lines[0].contains("I gave 1542 XP of myself to make 2 helpers"),
        "{lines:?}"
    );
    assert!(lines[0].contains("called them home"), "{lines:?}");
    assert!(ending(&w).contains("You left a stone of your own"));
}

#[test]
fn a_stone_survives_a_save() {
    let mut w = lit();
    w.apply(Command::CarveStone(1)).unwrap();
    let back = decode(&encode(&w, "midwinter")).unwrap().0;
    assert_eq!(back.settlement.your_stone, w.settlement.your_stone);
    assert!(!back.can_carve_stone());
}
