//! The end of the game: the Hearth of Spirits.

use spirit_sim::ScenarioKind;
use spirit_sim::defs::GameDefs;
use spirit_sim::save::{decode, encode};
use spirit_sim::world::{Command, World};

fn world() -> World {
    World::from_scenario(ScenarioKind::LastStage)
}

fn light(w: &mut World) -> bool {
    let kind = w.defs.building_kind_id("hearth_of_spirits").unwrap();
    w.apply(Command::QueueBuilding { kind, site: None })
        .unwrap();
    for _ in 0..12000 {
        if w.settlement.ended_at.is_some() {
            return true;
        }
        w.step();
        w.check_conservation().unwrap();
    }
    false
}

#[test]
fn the_stages_must_come_in_order() {
    let mut w = World::from_scenario(ScenarioKind::MasonYard);
    let kind = w.defs.building_kind_id("hearth_walls").unwrap();
    assert!(
        w.apply(Command::QueueBuilding { kind, site: None })
            .is_err()
    );
    let last = w.defs.building_kind_id("hearth_of_spirits").unwrap();
    assert!(!w.plannable().contains(&last));
}

#[test]
fn the_hearth_needs_all_three_trades() {
    let d = GameDefs::builtin_profile("normal").unwrap();
    let cost_of = |id: &str| {
        let k = d.building_kind_id(id).unwrap();
        d.building(k).cost.clone()
    };
    let all: Vec<_> = ["hearth_foundations", "hearth_walls", "hearth_of_spirits"]
        .iter()
        .flat_map(|id| cost_of(id))
        .collect();
    for id in [
        "stone",
        "dressed_stone",
        "logs",
        "panels",
        "beams",
        "keystones",
    ] {
        let r = d.resource_id(id).unwrap();
        assert!(all.iter().any(|c| c.0 == r), "{id}");
    }
}

#[test]
fn lighting_the_hearth_ends_the_story_and_not_the_game() {
    let mut w = world();
    assert!(w.ending_story().is_none());
    assert!(light(&mut w));
    let at = w.settlement.ended_at.unwrap();
    assert!(w.notices.contains("Hearth of Spirits is lit"));
    let story = w.ending_story().unwrap().join(" ");
    assert!(story.contains("was lit after"), "{story}");
    w.run_ticks(600);
    assert_eq!(w.settlement.ended_at, Some(at));
    w.check_conservation().unwrap();
}

#[test]
fn the_ending_remembers_what_you_gave() {
    let mut w = world();
    let skill = w.defs.skill_id("woodcutting").unwrap();
    let _ = skill;
    w.spirits[0].gift_xp = 500;
    w.spirits[0].trips = 100;
    w.run_ticks(100);
    assert!(light(&mut w));
    let story = w.ending_story().unwrap().join(" ");
    assert!(story.contains("500 XP"), "{story}");
    assert!(story.contains("Well-Travelled"), "{story}");
}

#[test]
fn the_ending_survives_saving() {
    let mut w = world();
    assert!(light(&mut w));
    let (loaded, _) = decode(&encode(&w, "last_stage")).unwrap();
    assert_eq!(loaded.ending_story(), w.ending_story());
}

#[test]
fn a_game_is_pointed_at_the_hearth_near_the_end() {
    let w = world();
    let goal = w
        .ambitions()
        .into_iter()
        .find(|a| a.title.contains("Hearth"));
    assert!(!goal.unwrap().done);
}

#[test]
fn everyone_comes_to_the_hearth_when_it_is_lit_and_then_goes_back() {
    let mut w = world();
    assert!(light(&mut w));
    let hearth = w.hearth().unwrap();
    assert!(w.hearth_gathering());
    w.run_ticks(900);
    for s in &w.spirits {
        assert!(s.actor.pos().is_adjacent(hearth), "{} isn't there", s.name);
    }
    let length = w.defs.construction.hearth_gathering_ticks;
    w.run_ticks(length);
    assert!(!w.hearth_gathering());
    w.check_conservation().unwrap();
}

#[test]
fn a_spirit_carrying_something_puts_it_down_first() {
    let mut w = world();
    let logs = w.defs.resource_id("logs").unwrap();
    assert!(light(&mut w));
    w.spirits[1].actor.inventory.add(logs, 3);
    *w.ledger.seeded.entry(logs).or_default() += 3;
    w.run_ticks(900);
    assert_eq!(w.spirits[1].actor.inventory.get(logs), 0);
    w.check_conservation().unwrap();
}
