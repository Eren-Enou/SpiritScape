//! What was left on the far shore.

use spirit_sim::grid::TilePos;
use spirit_sim::save::{decode, encode};
use spirit_sim::world::{Command, World};

fn game() -> World {
    World::new_journey("normal").unwrap()
}

fn at(w: &World, id: &str) -> TilePos {
    let r = w.defs.relics.iter().find(|r| r.id == id).unwrap();
    TilePos::new(r.at[0], r.at[1])
}

/// Puts the player beside a relic, as if they'd walked.
fn stand_by(w: &mut World, id: &str) {
    let p = at(w, id);
    w.player.actor.mover.pos = TilePos::new(p.x, p.y - 1);
    w.run_ticks(6);
}

#[test]
fn every_relic_lies_on_open_ground_past_the_river() {
    let w = game();
    assert_eq!(w.defs.relics.len(), 4);
    for r in &w.defs.relics {
        let p = TilePos::new(r.at[0], r.at[1]);
        assert!(p.y >= 30, "{}", r.id);
        assert!(w.grid.is_buildable(p), "{} is on something", r.id);
        let beside = TilePos::new(p.x, p.y - 1);
        assert!(w.grid.is_walkable(beside), "{} can't be reached", r.id);
    }
}

#[test]
fn walking_up_to_one_finds_it_once() {
    let mut w = game();
    stand_by(&mut w, "page");
    assert_eq!(w.settlement.relics_found, vec!["page".to_string()]);
    assert!(w.notices.contains("what wanted meant"));
    let total = w.notices.total;
    w.run_ticks(20);
    assert_eq!(w.notices.total, total);
}

#[test]
fn the_axe_head_teaches_you_something_once() {
    let mut w = game();
    let skill = w.defs.skill_id("woodcutting").unwrap();
    let before = w.player.xp(skill);
    stand_by(&mut w, "axe_head");
    assert_eq!(w.player.xp(skill), before + 500);
    w.run_ticks(20);
    assert_eq!(w.player.xp(skill), before + 500);
}

#[test]
fn what_you_found_goes_in_your_notes() {
    let mut w = game();
    stand_by(&mut w, "carving");
    let notes = w.notes();
    let found = notes.iter().find(|s| s.title.contains("over the water"));
    assert!(found.unwrap().lines[0].starts_with("A carving:"));
}

#[test]
fn the_story_says_so_only_if_you_found_them_all() {
    let mut w = game();
    w.settlement.ended_at = Some(1);
    for id in ["axe_head", "carving", "page"] {
        stand_by(&mut w, id);
    }
    let story = |w: &World| w.ending_story().unwrap().join(" ");
    assert!(!story(&w).contains("what they left"));
    stand_by(&mut w, "cairn");
    assert!(story(&w).contains("what they left"));
}

#[test]
fn what_you_found_survives_saving() {
    let mut w = game();
    stand_by(&mut w, "cairn");
    let (loaded, _) = decode(&encode(&w, "new_game")).unwrap();
    assert_eq!(loaded.settlement.relics_found, w.settlement.relics_found);
}

#[test]
fn you_can_reach_one_over_the_ice() {
    let mut w = game();
    let tpm = w.defs.ticks_per_second as u64 * 60;
    w.tick = 95 * tpm;
    w.step();
    let p = at(&w, "axe_head");
    w.apply(Command::PlayerInteract(TilePos::new(p.x, p.y - 1)))
        .unwrap();
    for _ in 0..4000 {
        w.step();
    }
    assert_eq!(w.settlement.relics_found, vec!["axe_head".to_string()]);
}
