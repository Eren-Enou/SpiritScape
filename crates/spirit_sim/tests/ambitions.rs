//! Long-term goals: they watch the world, and change nothing.

use spirit_sim::ScenarioKind;
use spirit_sim::autoplay::{lay_roads, play_guided};
use spirit_sim::defs::GameDefs;
use spirit_sim::journey::Phase;
use spirit_sim::world::{Command, World};

fn done(w: &World, title: &str) -> bool {
    w.ambitions().iter().any(|a| a.title == title && a.done)
}

#[test]
fn a_new_game_has_every_goal_ahead_of_it() {
    let w = World::new_journey("normal").unwrap();
    assert_eq!(w.ambitions().len(), 12);
    assert!(w.ambitions().iter().all(|a| !a.done));
    assert_eq!(w.next_ambition().unwrap().title, "Set up at Old Growth");
}

#[test]
fn every_hint_says_what_to_do() {
    let w = World::new_journey("normal").unwrap();
    assert!(w.ambitions().iter().all(|a| a.hint.len() > 20));
}

#[test]
fn a_goal_naming_something_unknown_is_refused() {
    let text = spirit_sim::defs::BUILTIN_DEFS.replace(
        "built = [\"outpost_storage\"]",
        "built = [\"no_such_building\"]",
    );
    let e = GameDefs::from_toml(&text).err().unwrap();
    assert!(e.contains("unknown"), "{e}");
}

#[test]
fn a_building_goal_is_met_when_it_is_finished_not_planned() {
    let mut w = World::from_scenario(ScenarioKind::PlankSurplus);
    assert!(done(&w, "Set up at Old Growth"), "the scenario has one");
    let kind = w.defs.building_kind_id("grove_lane").unwrap();
    w.apply(Command::QueueBuilding { kind, site: None })
        .unwrap();
    assert!(!done(&w, "Lay your first road"));
    for _ in 0..6000 {
        w.step();
    }
    assert!(done(&w, "Lay your first road"));
}

#[test]
fn a_goal_with_several_buildings_counts_them() {
    let mut w = World::from_scenario(ScenarioKind::PlankSurplus);
    let goal = |w: &World| {
        let all = w.ambitions();
        let a = all.iter().find(|a| a.title.contains("by road")).unwrap();
        a.progress.clone().unwrap()
    };
    assert_eq!(goal(&w).0, 0);
    let kind = w.defs.building_kind_id("grove_lane").unwrap();
    w.apply(Command::QueueBuilding { kind, site: None })
        .unwrap();
    for _ in 0..6000 {
        w.step();
    }
    assert_eq!((goal(&w).0, goal(&w).1), (1, 4));
    lay_roads(&mut w);
    assert!(
        !done(&w, "Join the settlement by road"),
        "only finished roads count"
    );
}

#[test]
fn a_skill_goal_stays_met_after_the_xp_is_given_away() {
    let mut w = World::from_scenario(ScenarioKind::PanelWorkshop);
    assert!(done(&w, "Make Structural Beams"));
    let skill = w.defs.skill_id("woodworking").unwrap();
    for _ in 0..3 {
        w.step();
    }
    let xp = w.player.xp(skill);
    w.apply(Command::SoulSeparate { skill, xp }).unwrap();
    assert!(done(&w, "Make Structural Beams"));
}

#[test]
fn a_made_goal_counts_what_was_crafted() {
    let mut w = World::from_scenario(ScenarioKind::PanelWorkshop);
    let panels = w.defs.resource_id("panels").unwrap();
    assert!(!done(&w, "Make 20 Fitted Panels"));
    w.ledger.crafted.insert(panels, 20);
    assert!(done(&w, "Make 20 Fitted Panels"));
}

#[test]
fn goals_change_nothing_in_the_world() {
    let mut a = World::from_scenario(ScenarioKind::ExpandedRemote);
    let mut b = World::from_scenario(ScenarioKind::ExpandedRemote);
    for _ in 0..600 {
        a.step();
        let _ = a.ambitions();
        b.step();
    }
    assert_eq!(a.state_hash(), b.state_hash());
}

#[test]
fn a_settled_game_is_pointed_at_the_next_goal() {
    let mut w = World::new_journey("development").unwrap();
    w.journey.as_mut().unwrap().phase = Phase::Settled;
    let g = w.guidance().unwrap();
    assert_eq!(g.title, "Set up at Old Growth");
    w.defs.ambitions.clear();
    assert_eq!(w.guidance().unwrap().title, "Your settlement runs itself");
}

#[test]
fn storage_trouble_still_comes_first() {
    let mut w = World::new_journey("development").unwrap();
    w.journey.as_mut().unwrap().phase = Phase::Settled;
    let logs = w.defs.resource_id("logs").unwrap();
    let main = w.main_storage;
    let free = w.main().storage.free();
    let put = w.buildings[main].storage.deposit(logs, free);
    *w.ledger.seeded.entry(logs).or_default() += put as u64;
    assert!(w.guidance().unwrap().hint.contains("nearly full"));
}

#[test]
fn a_resting_quarrying_spirit_does_not_talk_about_logs() {
    let mut w = World::from_scenario(ScenarioKind::QuarryStart);
    w.run_ticks(100);
    let s = w
        .spirits
        .iter()
        .find(|s| w.defs.role(s.role).id == "quarrier");
    let text = s.unwrap().activity.text().to_string();
    assert!(!text.contains("Logs"), "{text}");
}

/// A whole game, so it's release only.
#[test]
#[ignore]
fn the_guided_route_meets_the_goals_on_its_way() {
    let (w, _) = play_guided("normal").unwrap();
    for title in [
        "Set up at Old Growth",
        "Cut Ironwood",
        "Make Structural Beams",
        "Lay your first road",
        "Open the Hollow Grove",
        "Join the settlement by road",
    ] {
        assert!(done(&w, title), "{title}");
    }
}
