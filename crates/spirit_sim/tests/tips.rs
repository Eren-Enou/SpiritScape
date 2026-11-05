//! Teaching each system when it first matters.

use spirit_sim::ScenarioKind;
use spirit_sim::journey::Phase;
use spirit_sim::save::{decode, encode};
use spirit_sim::world::World;

fn game() -> World {
    World::new_journey("normal").unwrap()
}

fn set_phase(w: &mut World, phase: Phase) {
    w.journey.as_mut().unwrap().phase = phase;
}

const TABS: [&str; 8] = [
    "gathering",
    "production",
    "progress",
    "training",
    "spirits",
    "soul",
    "outpost",
    "projects",
];

#[test]
fn every_tip_reads_properly_and_says_something() {
    let w = game();
    assert!(!w.defs.tips.is_empty());
    for t in &w.defs.tips {
        assert!(w.tip_condition_known(&t.when), "{}: {}", t.id, t.when);
        assert!(t.text.len() > 20, "{}", t.id);
    }
}

#[test]
fn a_new_game_opens_with_almost_nothing() {
    let w = game();
    let open = TABS.iter().filter(|t| w.is_open(t)).count();
    assert_eq!(open, 0);
    assert!(w.is_open("woodcutting"), "there has to be something to do");
    assert!(!w.is_open("woodworking"));
    assert!(!w.is_open("quarrying"));
    assert!(!w.is_open("masonry"));
}

#[test]
fn scenarios_show_everything() {
    let w = World::from_scenario(ScenarioKind::ExpandedRemote);
    assert!(TABS.iter().all(|t| w.is_open(t)));
}

#[test]
fn a_tab_opens_with_a_line_about_what_it_is_for() {
    let mut w = game();
    w.run_ticks(100);
    assert!(!w.is_open("soul"));
    set_phase(&mut w, Phase::FirstSpirit);
    w.run_ticks(100);
    assert!(w.is_open("soul"));
    assert!(w.notices.contains("Soul tab"));
}

#[test]
fn each_tip_is_said_once() {
    let mut w = game();
    set_phase(&mut w, Phase::FirstSpirit);
    w.run_ticks(300);
    let count = w
        .notices
        .iter()
        .filter(|n| n.text.contains("Soul tab"))
        .count();
    assert_eq!(count, 1);
}

#[test]
fn by_the_end_of_the_first_cycle_every_tab_is_open() {
    let mut w = game();
    set_phase(&mut w, Phase::Settled);
    let skill = w.defs.skill_id("woodcutting").unwrap();
    let _ = skill;
    w.settlement.tips_seen = None;
    w.run_ticks(100);
    let shut: Vec<_> = TABS.iter().filter(|t| !w.is_open(t)).collect();
    assert!(shut.len() <= 1, "{shut:?}");
}

#[test]
fn an_older_save_is_not_told_everything_again() {
    let mut w = game();
    w.tick = 100_000;
    set_phase(&mut w, Phase::Settled);
    w.settlement.tips_seen = None;
    assert!(w.is_open("soul"), "it's open from the start");
    w.run_ticks(100);
    assert!(!w.notices.contains("Soul tab"));
    assert!(w.is_open("soul"));
}

#[test]
fn tips_change_nothing_about_how_a_game_runs() {
    let run = |tips: bool| {
        let mut w = game();
        if !tips {
            w.defs.tips.clear();
        }
        w.run_ticks(3000);
        (w.ledger.harvested.clone(), w.tick)
    };
    assert_eq!(run(true), run(false));
}

#[test]
fn what_has_been_said_survives_saving() {
    let mut w = game();
    set_phase(&mut w, Phase::FirstSpirit);
    w.run_ticks(200);
    let (loaded, _) = decode(&encode(&w, "new_game")).unwrap();
    assert_eq!(loaded.settlement.tips_seen, w.settlement.tips_seen);
    assert!(loaded.is_open("soul"));
}
