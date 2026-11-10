//! Spirits as characters: traits, deeds and the story of each one.

use spirit_sim::ScenarioKind;
use spirit_sim::save::{decode, encode};
use spirit_sim::world::{Command, World};

fn world() -> World {
    World::from_scenario(ScenarioKind::QuarryStart)
}

fn make_spirit(w: &mut World) -> usize {
    let skill = w.defs.skill_id("quarrying").unwrap();
    w.apply(Command::SoulSeparate { skill, xp: 174 }).unwrap();
    w.spirits.len() - 1
}

#[test]
fn scenario_spirits_have_no_trait_so_earlier_results_stand() {
    let w = world();
    assert!(w.spirits.iter().all(|s| s.character.is_none()));
}

#[test]
fn a_spirit_made_from_xp_has_a_trait_and_remembers_its_making() {
    let mut w = world();
    w.run_ticks(600);
    let id = make_spirit(&mut w);
    let s = &w.spirits[id];
    assert!(s.character.is_some());
    assert_eq!(s.born, 600);
    assert_eq!(s.gift_xp, 174);
    let story = w.spirit_story(s);
    assert!(story[0].contains("174 of your Quarrying XP"), "{story:?}");
    assert!(story[1].contains(':'), "{story:?}");
}

#[test]
fn a_trait_with_a_carry_bonus_changes_what_it_carries() {
    let mut w = world();
    while w.next_trait().unwrap().carry == 0 {
        let extra = w.spirits[0].clone();
        w.spirits.push(extra);
    }
    let bonus = w.next_trait().unwrap().carry;
    let id = make_spirit(&mut w);
    let s = &w.spirits[id];
    let role = w.defs.role(s.role);
    assert_eq!(
        s.actor.inventory.capacity,
        role.capacity_at(s.level()) + bonus
    );
}

#[test]
fn the_next_trait_can_be_seen_coming_and_they_come_round_in_turn() {
    let mut w = world();
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..w.defs.traits.len() {
        seen.insert(w.next_trait().unwrap().id.clone());
        let extra = w.spirits[0].clone();
        w.spirits.push(extra);
    }
    assert_eq!(seen.len(), w.defs.traits.len());
    let told = w.next_trait().unwrap().id.clone();
    let id = make_spirit(&mut w);
    let got = w.spirits[id].character.unwrap() as usize;
    assert_eq!(w.defs.traits[got].id, told);
}

#[test]
fn a_spirit_can_be_named_and_keeps_its_title() {
    let mut w = world();
    w.spirits[0].trips = 100;
    w.run_ticks(100);
    let name = "Old Pine".to_string();
    w.apply(Command::RenameSpirit { id: 0, name }).unwrap();
    assert!(w.spirit_title(&w.spirits[0]).starts_with("Old Pine the "));
}

#[test]
fn a_name_has_to_be_a_name() {
    let mut w = world();
    let bad = |w: &mut World, name: &str| {
        let name = name.to_string();
        w.apply(Command::RenameSpirit { id: 0, name }).is_err()
    };
    assert!(bad(&mut w, "   "));
    assert!(bad(&mut w, &"x".repeat(19)));
    let was = w.spirits[0].name.clone();
    assert_eq!(w.spirits[0].name, was);
}

#[test]
fn a_chosen_name_survives_saving() {
    let mut w = world();
    let name = "Fern".to_string();
    w.apply(Command::RenameSpirit { id: 0, name }).unwrap();
    let (loaded, _) = decode(&encode(&w, "quarry_start")).unwrap();
    assert_eq!(loaded.spirits[0].name, "Fern");
}

#[test]
fn the_same_game_makes_the_same_spirit() {
    let make = || {
        let mut w = world();
        w.run_ticks(600);
        let id = make_spirit(&mut w);
        w.spirits[id].character
    };
    assert_eq!(make(), make());
}

#[test]
fn work_earns_a_deed_and_a_title_once() {
    let mut w = world();
    let s = &mut w.spirits[0];
    let name = s.name.clone();
    s.trips = 100;
    w.run_ticks(300);
    let s = &w.spirits[0];
    assert_eq!(s.deeds.len(), 1);
    assert!(w.spirit_title(s).starts_with(&format!("{name} the ")));
    w.run_ticks(300);
    assert_eq!(w.spirits[0].deeds.len(), 1);
}

#[test]
fn the_latest_deed_gives_the_title() {
    let mut w = world();
    w.spirits[0].trips = 1000;
    w.run_ticks(100);
    let s = &w.spirits[0];
    assert!(s.deeds.len() >= 2);
    let last = &w.defs.deeds[s.deeds.last().unwrap().0 as usize];
    assert!(w.spirit_title(s).ends_with(&last.title));
}

#[test]
fn a_story_lists_what_a_spirit_has_done() {
    let mut w = world();
    w.spirits[0].units_gathered = 12;
    w.spirits[0].trips = 100;
    w.run_ticks(100);
    let story = w.spirit_story(&w.spirits[0]).join(" ");
    assert!(story.contains("gathered 12"), "{story}");
    assert!(story.contains("It was here before you"), "{story}");
}

#[test]
fn deeds_change_nothing_about_how_a_world_runs() {
    let run = |deeds: bool| {
        let mut w = World::from_scenario(ScenarioKind::ExpandedRemote);
        if !deeds {
            w.defs.deeds.clear();
        }
        w.run_ticks(6000);
        (w.ledger.harvested.clone(), w.main().storage.total())
    };
    assert_eq!(run(true), run(false));
}

#[test]
fn stories_survive_saving_and_continue_identically() {
    let mut w = world();
    let id = make_spirit(&mut w);
    w.spirits[id].trips = 100;
    w.run_ticks(200);
    let (mut loaded, _) = decode(&encode(&w, "quarry_start")).unwrap();
    assert_eq!(loaded.spirits[id].deeds, w.spirits[id].deeds);
    assert_eq!(loaded.spirits[id].character, w.spirits[id].character);
    w.run_ticks(300);
    loaded.run_ticks(300);
    assert_eq!(w.state_hash(), loaded.state_hash());
}

#[test]
fn older_saves_have_spirits_with_no_trait() {
    let path = format!(
        "{}/tests/fixtures/v7_panels.save",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = String::from_utf8(std::fs::read(path).unwrap()).unwrap();
    let (mut w, _) = decode(&text).unwrap();
    assert!(w.spirits.iter().all(|s| s.character.is_none()));
    w.run_ticks(300);
    assert!(!w.spirit_story(&w.spirits[0]).is_empty());
}

#[test]
fn there_are_enough_names_for_a_big_settlement() {
    let mut w = World::from_scenario(ScenarioKind::NewGame);
    let skill = w.defs.skill_id("woodcutting").unwrap();
    let _ = (&mut w, skill);
    assert!(spirit_sim::world::SPIRIT_NAMES.len() >= 24);
    let mut names = spirit_sim::world::SPIRIT_NAMES.to_vec();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), spirit_sim::world::SPIRIT_NAMES.len());
}
