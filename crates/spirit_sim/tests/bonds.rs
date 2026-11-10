//! Spirits who keep company: how it starts, and what remembers it.

use spirit_sim::ScenarioKind;
use spirit_sim::save::{decode, encode};
use spirit_sim::world::{Command, World};

/// A quiet hour where something small happens every second instead of every
/// ten minutes, so shared moments pile up quickly.
fn busy_hour() -> World {
    let mut w = World::from_scenario(ScenarioKind::QuietHour);
    w.defs.construction.moment_ticks = 10;
    w
}

#[test]
fn spirits_who_share_moments_come_to_keep_company() {
    let mut w = busy_hour();
    assert!(w.bonds().is_empty());
    w.run_ticks(3000);
    let bonds = w.bonds();
    assert!(!bonds.is_empty(), "nobody kept company");
    for b in &bonds {
        assert_eq!(w.spirits[b.a].companion.map(|c| c.0), Some(b.b));
        assert_eq!(w.spirits[b.b].companion.map(|c| c.0), Some(b.a));
        let shared = w
            .settlement
            .shared_moments
            .iter()
            .find(|p| p.0 == b.a && p.1 == b.b)
            .map_or(0, |p| p.2);
        assert!(shared >= w.defs.bonds.moments, "{shared}");
    }
    let said = w
        .events
        .iter()
        .any(|e| e.text.contains("keep each other company"));
    assert!(said);
}

#[test]
fn nobody_keeps_company_when_the_rule_is_off() {
    let mut w = busy_hour();
    w.defs.bonds.moments = 0;
    w.run_ticks(3000);
    assert!(w.bonds().is_empty());
    assert!(!w.settlement.shared_moments.is_empty());
}

#[test]
fn each_spirit_keeps_company_with_one_other() {
    let mut w = World::from_scenario(ScenarioKind::QuietHour);
    assert!(w.form_bond(0, 1));
    assert!(!w.form_bond(0, 2));
    assert!(!w.form_bond(2, 1));
    assert!(!w.form_bond(2, 2));
    assert!(!w.form_bond(2, 99));
    assert_eq!(w.bonds().len(), 1);
    // Still forming, as moments keep coming, changes nothing.
    let mut busy = busy_hour();
    busy.run_ticks(6000);
    for s in &busy.spirits {
        let mine = busy
            .bonds()
            .iter()
            .filter(|b| b.a == s.id || b.b == s.id)
            .count();
        assert!(mine <= 1, "{} has {mine} companions", s.name);
    }
}

#[test]
fn a_story_names_its_companion_as_it_is_called_now() {
    let mut w = World::from_scenario(ScenarioKind::QuietHour);
    w.run_ticks(600);
    assert!(w.form_bond(0, 1));
    w.spirits[1].name = "Fernwhistle".into();
    let story = w.spirit_story(&w.spirits[0]).join(" ");
    assert!(
        story.contains("Has kept company with Fernwhistle since 0:01"),
        "{story}"
    );
    let other = w.spirit_story(&w.spirits[2]).join(" ");
    assert!(!other.contains("kept company"), "{other}");
}

#[test]
fn the_notes_say_who_keeps_company() {
    let mut w = World::from_scenario(ScenarioKind::QuietHour);
    assert!(!w.notes().iter().any(|n| n.title == "Who keeps company"));
    w.form_bond(0, 1);
    let notes = w.notes();
    let section = notes
        .iter()
        .find(|n| n.title == "Who keeps company")
        .unwrap();
    assert_eq!(section.lines.len(), 1);
    assert!(
        section.lines[0].starts_with("0:00: "),
        "{:?}",
        section.lines
    );
}

#[test]
fn the_hearth_remembers_who_stood_together() {
    let mut w = World::from_scenario(ScenarioKind::Midwinter);
    let before = w.ending_story().unwrap().join(" ");
    assert!(!before.contains("stood together"), "{before}");
    w.form_bond(0, 1);
    let after = w.ending_story().unwrap().join(" ");
    let (a, b) = (&w.spirits[0].name, &w.spirits[1].name);
    assert!(
        after.contains(&format!("{a} and {b} stood together by the Hearth.")),
        "{after}"
    );
}

#[test]
fn companions_cross_together() {
    let mut w = World::from_scenario(ScenarioKind::LastBoat);
    let n = w.spirits.len();
    assert!(n >= 3, "{n}");
    // The last two, so the boat has to choose them out of their usual order.
    let (a, b) = (0, n - 1);
    let names = (w.spirits[a].name.clone(), w.spirits[b].name.clone());
    assert!(w.form_bond(a, b));
    w.apply(Command::CallHome).unwrap();
    let mut order = Vec::new();
    for _ in 0..20_000 {
        if w.homecoming_finished() {
            break;
        }
        w.step();
        w.check_conservation().unwrap();
        for s in &w.spirits {
            if s.aboard && !order.contains(&s.id) {
                order.push(s.id);
            }
        }
    }
    assert!(w.homecoming_finished());
    let ia = order.iter().position(|&s| s == a).unwrap();
    let ib = order.iter().position(|&s| s == b).unwrap();
    assert_eq!(ia.abs_diff(ib), 1, "they boarded apart: {order:?}");
    let story = w.ending_story().unwrap().join(" ");
    let one = format!("{} and {} crossed together.", names.0, names.1);
    let other = format!("{} and {} crossed together.", names.1, names.0);
    assert!(story.contains(&one) || story.contains(&other), "{story}");
}

#[test]
fn companions_survive_a_save() {
    let mut w = busy_hour();
    w.run_ticks(3000);
    let bonds = w.bonds();
    assert!(!bonds.is_empty());
    let back = decode(&encode(&w, "quiet_hour")).unwrap().0;
    assert_eq!(back.bonds(), bonds);
    assert_eq!(back.settlement.shared_moments, w.settlement.shared_moments);
}

#[test]
fn old_friends_start_out_keeping_company() {
    let w = World::from_scenario(ScenarioKind::OldFriends);
    assert_eq!(w.bonds().len(), 1);
    let story = w.spirit_story(&w.spirits[0]).join(" ");
    assert!(story.contains("Has kept company with"), "{story}");
}
