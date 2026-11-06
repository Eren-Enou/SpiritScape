//! Small things spirits do. They must never change anything.

use spirit_sim::ScenarioKind;
use spirit_sim::save::{decode, encode};
use spirit_sim::world::{Command, World};

fn quiet() -> World {
    World::from_scenario(ScenarioKind::QuietHour)
}

fn hour(w: &mut World) {
    w.run_ticks(36_000);
}

fn all(w: &World) -> Vec<u16> {
    let each = w.spirits.iter().flat_map(|s| s.moments.iter().map(|m| m.0));
    each.collect()
}

fn id_of(w: &World, m: u16) -> &str {
    &w.defs.moments[m as usize].id
}

/// Everything a moment could have disturbed.
fn the_world(w: &World) -> String {
    let mut out = format!("{} {:?}", w.player.actor.pos(), w.ledger.harvested);
    for b in &w.buildings {
        out += &format!(" {:?}", b.storage.contents());
    }
    for s in &w.spirits {
        let (at, bag) = (s.actor.pos(), s.actor.inventory.items());
        out += &format!(" {at} {bag:?} {} {}", s.trips, s.actor.queue.len());
    }
    for t in &w.trees {
        out += &format!(" {}", t.logs_left());
    }
    out
}

#[test]
fn every_moment_names_something_real() {
    let w = quiet();
    assert!(w.defs.moments.len() >= 20);
    for m in &w.defs.moments {
        assert!(m.text.contains("{name}"), "{}", m.id);
        if let Some(t) = &m.character {
            assert!(w.defs.traits.iter().any(|d| &d.id == t), "{}", m.id);
        }
        if let Some(s) = &m.season {
            assert!(w.defs.seasons.iter().any(|d| &d.id == s), "{}", m.id);
        }
        if let Some(s) = &m.skill {
            assert!(w.defs.skill_id(s).is_some(), "{}", m.id);
        }
    }
}

#[test]
fn moments_change_nothing_in_the_world() {
    let mut with = quiet();
    let mut without = quiet();
    without.settlement.moments = false;
    for _ in 0..7 {
        with.run_ticks(3600);
        without.run_ticks(3600);
    }
    assert!(!all(&with).is_empty());
    assert!(all(&without).is_empty());
    // The state hash counts log entries, and a moment is one, so this compares
    // the world itself.
    assert_eq!(the_world(&with), the_world(&without));
}

#[test]
fn scenarios_are_left_alone_unless_they_ask() {
    let mut w = World::from_scenario(ScenarioKind::KilnYard);
    hour(&mut w);
    assert!(all(&w).is_empty());
}

#[test]
fn nobody_does_the_same_thing_twice() {
    let mut w = quiet();
    for _ in 0..6 {
        hour(&mut w);
    }
    for s in &w.spirits {
        let mut seen: Vec<u16> = s.moments.iter().map(|m| m.0).collect();
        let count = seen.len();
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), count, "{}", s.name);
    }
}

#[test]
fn only_a_named_spirit_does_what_a_named_spirit_does() {
    let mut w = quiet();
    for _ in 0..6 {
        hour(&mut w);
    }
    for m in all(&w) {
        assert!(!w.defs.moments[m as usize].named, "{}", id_of(&w, m));
    }
    let mut w = quiet();
    for id in 0..w.spirits.len() {
        let name = format!("Named {id}");
        w.apply(Command::RenameSpirit { id, name }).unwrap();
    }
    for _ in 0..6 {
        hour(&mut w);
    }
    assert!(all(&w).iter().any(|m| w.defs.moments[*m as usize].named));
}

#[test]
fn a_spirit_you_never_made_has_nothing_of_yours() {
    let mut w = quiet();
    for _ in 0..6 {
        hour(&mut w);
    }
    let hauler = w.spirits.iter().find(|s| s.gift_xp == 0).unwrap();
    for (m, _) in &hauler.moments {
        assert!(!w.defs.moments[*m as usize].gifted, "{}", id_of(&w, *m));
    }
}

#[test]
fn a_moment_shows_for_a_while_and_then_stops() {
    let mut w = quiet();
    while all(&w).is_empty() {
        w.step();
    }
    let s = w.spirits.iter().find(|s| !s.moments.is_empty()).unwrap();
    let text = w.spirit_moment(s).unwrap();
    assert!(text.contains(&s.name), "{text}");
    assert!(!text.contains("{name}"));
    let id = s.id;
    let shown = w.defs.construction.moment_shown_ticks;
    w.run_ticks(shown + 1);
    assert_eq!(w.spirit_moment(&w.spirits[id]), None);
}

#[test]
fn moments_become_part_of_a_spirits_story_and_survive_saving() {
    let mut w = quiet();
    while all(&w).is_empty() {
        w.step();
    }
    let s = w.spirits.iter().find(|s| !s.moments.is_empty()).unwrap();
    let last = w.spirit_story(s).pop().unwrap();
    assert!(last.contains(&s.name), "{last}");
    let (loaded, _) = decode(&encode(&w, "quiet_hour")).unwrap();
    assert_eq!(loaded.spirits[s.id].moments, s.moments);
}

#[test]
fn a_new_name_is_used_from_then_on() {
    let mut w = quiet();
    while all(&w).is_empty() {
        w.step();
    }
    let id = w
        .spirits
        .iter()
        .position(|s| !s.moments.is_empty())
        .unwrap();
    let name = "Bartholomew".to_string();
    w.apply(Command::RenameSpirit { id, name }).unwrap();
    assert!(
        w.spirit_moment(&w.spirits[id])
            .unwrap()
            .contains("Bartholomew")
    );
}
