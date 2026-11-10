//! Spirits who notice each other, midwinter at the Hearth, and the years.

use spirit_sim::ScenarioKind;
use spirit_sim::grid::TilePos;
use spirit_sim::save::{decode, encode};
use spirit_sim::world::World;

fn quiet() -> World {
    World::from_scenario(ScenarioKind::QuietHour)
}

fn paired(w: &World) -> Vec<(usize, u16)> {
    let each = w
        .spirits
        .iter()
        .flat_map(|s| s.moments.iter().map(|m| (s.id, m.0)));
    each.filter(|(_, m)| w.defs.moments[*m as usize].pair)
        .collect()
}

#[test]
fn spirits_near_each_other_notice_each_other() {
    let mut w = quiet();
    for _ in 0..8 {
        w.run_ticks(36_000);
    }
    let pairs = paired(&w);
    assert!(!pairs.is_empty());
    let (id, m) = pairs[0];
    let other = &w.spirits[id].moment_others[&m];
    assert_ne!(other, &w.spirits[id].name, "not itself");
    assert!(w.spirits.iter().any(|s| &s.name == other));
}

#[test]
fn a_spirit_alone_notices_nobody() {
    let mut w = quiet();
    let far = [TilePos::new(2, 2), TilePos::new(40, 2), TilePos::new(2, 25)];
    for s in &mut w.spirits {
        s.actor.mover.pos = far[s.id % 3];
        s.policy.focus = spirit_sim::entities::Focus::Rest;
    }
    for _ in 0..4 {
        w.run_ticks(36_000);
    }
    assert!(paired(&w).is_empty());
}

#[test]
fn no_moment_is_ever_shown_with_its_placeholder() {
    let mut w = quiet();
    for _ in 0..8 {
        w.run_ticks(36_000);
    }
    for s in &w.spirits {
        for line in w.spirit_story(s) {
            assert!(!line.contains('{'), "{line}");
        }
    }
}

#[test]
fn who_it_was_survives_saving() {
    let mut w = quiet();
    while paired(&w).is_empty() {
        w.run_ticks(6000);
    }
    let (id, _) = paired(&w)[0];
    let (loaded, _) = decode(&encode(&w, "quiet_hour")).unwrap();
    assert_eq!(
        loaded.spirits[id].moment_others,
        w.spirits[id].moment_others
    );
}

/// A real game with the Hearth lit, at the start of the second year.
fn lit() -> World {
    let mut w = World::new_journey("normal").unwrap();
    w.step();
    let kind = w.defs.building_kind_id("hearth_of_spirits").unwrap();
    let main = w.main_storage;
    w.buildings[main].kind = kind;
    w.settlement.ended_at = Some(1);
    w
}

fn to_season(w: &mut World, id: &str, year: u64) {
    let tpm = w.defs.ticks_per_second as u64 * 60;
    let length: u64 = w.defs.seasons.iter().map(|s| s.minutes * tpm).sum();
    let mut start = year * length;
    for s in &w.defs.seasons {
        if s.id == id {
            break;
        }
        start += s.minutes * tpm;
    }
    w.tick = start;
    w.step();
    assert_eq!(w.season().unwrap().id, id);
}

#[test]
fn every_winter_they_go_to_the_hearth() {
    let mut w = lit();
    to_season(&mut w, "autumn", 1);
    assert!(!w.hearth_gathering());
    to_season(&mut w, "winter", 1);
    assert!(w.hearth_gathering());
    assert!(w.notices.contains("Midwinter"));
}

#[test]
fn they_really_do_walk_there() {
    let mut w = lit();
    let hearth = w.hearth().unwrap();
    for s in &mut w.spirits {
        s.actor.mover.pos = TilePos::new(hearth.x + 10, hearth.y + 6);
    }
    to_season(&mut w, "winter", 1);
    w.run_ticks(w.defs.construction.midwinter_ticks - 10);
    for s in &w.spirits {
        assert!(
            s.actor.pos().is_adjacent(hearth),
            "{} at {}",
            s.name,
            s.actor.pos()
        );
        assert_eq!(s.activity.text(), "At the Hearth");
    }
}

#[test]
fn and_then_they_go_back_to_work() {
    let mut w = lit();
    to_season(&mut w, "winter", 1);
    w.run_ticks(w.defs.construction.midwinter_ticks + 1);
    assert_eq!(w.season().unwrap().id, "winter");
    assert!(!w.hearth_gathering());
}

#[test]
fn without_a_hearth_winter_is_just_winter() {
    let mut w = World::new_journey("normal").unwrap();
    w.step();
    to_season(&mut w, "autumn", 1);
    to_season(&mut w, "winter", 1);
    assert!(!w.hearth_gathering());
    assert!(!w.notices.contains("Midwinter"));
}

#[test]
fn each_year_goes_in_your_notes() {
    let mut w = World::new_journey("normal").unwrap();
    w.step();
    to_season(&mut w, "winter", 0);
    let logs = w.defs.resource_id("logs").unwrap();
    *w.ledger.harvested.entry(logs).or_default() += 100;
    to_season(&mut w, "spring", 1);
    assert_eq!(w.settlement.years.len(), 1);
    assert!(w.notices.contains("Another year"));
    let notes = w.notes();
    let years = notes.iter().find(|s| s.title.contains("years")).unwrap();
    assert!(
        years.lines[0].starts_with("Year 1: 100 gathered"),
        "{}",
        years.lines[0]
    );
}

#[test]
fn a_year_shows_only_what_it_added() {
    let mut w = World::new_journey("normal").unwrap();
    w.step();
    let logs = w.defs.resource_id("logs").unwrap();
    to_season(&mut w, "winter", 0);
    *w.ledger.harvested.entry(logs).or_default() += 100;
    to_season(&mut w, "spring", 1);
    to_season(&mut w, "winter", 1);
    *w.ledger.harvested.entry(logs).or_default() += 30;
    to_season(&mut w, "spring", 2);
    let notes = w.notes();
    let years = notes.iter().find(|s| s.title.contains("years")).unwrap();
    assert_eq!(years.lines.len(), 2);
    assert!(
        years.lines[1].starts_with("Year 2: 30 gathered"),
        "{}",
        years.lines[1]
    );
}

#[test]
fn the_naming_tip_points_to_the_right_tab() {
    let w = World::new_journey("normal").unwrap();
    let tip = w.defs.tips.iter().find(|t| t.id == "naming").unwrap();
    assert!(tip.text.contains("Spirits tab (O)"));
    assert!(w.tip_condition_known(&tip.when));
}

#[test]
fn the_midwinter_scenario_starts_just_before_it() {
    let mut w = World::from_scenario(ScenarioKind::Midwinter);
    assert_eq!(w.season().unwrap().id, "autumn");
    assert!(w.hearth().is_some());
    assert!(!w.hearth_gathering());
    w.run_ticks(700);
    assert_eq!(w.season().unwrap().id, "winter");
    assert!(w.hearth_gathering());
    assert!(w.notices.contains("Midwinter"));
}

#[test]
fn other_scenarios_still_have_no_seasons() {
    assert!(
        World::from_scenario(ScenarioKind::KilnYard)
            .season()
            .is_none()
    );
}
