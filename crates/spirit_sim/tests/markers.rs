//! The old stones: a story to find.

use spirit_sim::ScenarioKind;
use spirit_sim::grid::TilePos;
use spirit_sim::save::{decode, encode};
use spirit_sim::world::{Command, World};

fn world() -> World {
    World::from_scenario(ScenarioKind::QuarryStart)
}

fn visit(w: &mut World, x: i32, y: i32) {
    w.player.actor.mover.pos = TilePos::new(x, y);
    w.run_ticks(6);
}

#[test]
fn every_stone_stands_on_open_ground_and_can_be_reached() {
    let w = World::from_scenario(ScenarioKind::ExpandedCentral);
    let field = w.grid.distance_field(&[TilePos::new(6, 15)]);
    for m in &w.defs.markers {
        let p = TilePos::new(m.at[0], m.at[1]);
        assert!(w.grid.is_buildable(p), "{}", m.id);
        assert!(
            field.get(TilePos::new(m.at[0], m.at[1] + 1)).is_some(),
            "{}",
            m.id
        );
    }
}

#[test]
fn walking_up_to_a_stone_reads_it_once() {
    let mut w = world();
    let [x, y] = w.defs.markers[0].at;
    let title = w.defs.markers[0].title.clone();
    visit(&mut w, x, y + 1);
    assert_eq!(w.markers_read().len(), 1);
    assert!(w.notices.contains(&title));
    let count = w.notices.iter().filter(|n| n.text.contains(&title)).count();
    w.run_ticks(60);
    assert_eq!(
        w.notices.iter().filter(|n| n.text.contains(&title)).count(),
        count
    );
}

#[test]
fn nothing_is_read_from_a_distance() {
    let mut w = world();
    let [x, y] = w.defs.markers[0].at;
    visit(&mut w, x, y + 3);
    assert!(w.markers_read().is_empty());
}

#[test]
fn spirits_walking_past_read_nothing() {
    let mut w = world();
    let [x, y] = w.defs.markers[0].at;
    w.spirits[0].actor.mover.pos = TilePos::new(x, y + 1);
    w.run_ticks(6);
    assert!(w.markers_read().is_empty());
}

#[test]
fn the_last_stone_is_beyond_the_ravine() {
    let w = world();
    let last = w.defs.markers.last().unwrap();
    let start = TilePos::new(6, 15);
    let field = w.grid.distance_field(&[start]);
    let at = TilePos::new(last.at[0], last.at[1] + 1);
    assert!(field.get(at).is_none(), "shut until the crossing");
}

#[test]
fn the_stones_count_toward_a_goal() {
    let mut w = world();
    for i in 0..w.defs.markers.len() {
        let [x, y] = w.defs.markers[i].at;
        visit(&mut w, x, y + 1);
    }
    let all = w.ambitions();
    let goal = all
        .iter()
        .find(|a| a.title == "Read the old stones")
        .unwrap();
    assert!(goal.done);
}

#[test]
fn stones_read_are_remembered_across_a_save() {
    let mut w = world();
    let [x, y] = w.defs.markers[0].at;
    visit(&mut w, x, y + 1);
    let (loaded, _) = decode(&encode(&w, "quarry_start")).unwrap();
    assert_eq!(loaded.markers_read().len(), 1);
}

#[test]
fn reading_stones_changes_how_the_ending_reads() {
    let mut w = World::from_scenario(ScenarioKind::LastStage);
    for i in 0..w.defs.markers.len() {
        let [x, y] = w.defs.markers[i].at;
        visit(&mut w, x, y + 1);
    }
    w.player.actor.mover.pos = TilePos::new(7, 14);
    let kind = w.defs.building_kind_id("hearth_of_spirits").unwrap();
    w.apply(Command::QueueBuilding { kind, site: None })
        .unwrap();
    for _ in 0..12000 {
        w.step();
    }
    let story = w.ending_story().unwrap().join(" ");
    assert!(story.contains("built no hearth"), "{story}");
}
