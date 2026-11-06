//! The other ending: calling the spirits home.

use spirit_sim::ScenarioKind;
use spirit_sim::save::{decode, encode};
use spirit_sim::world::{Command, World};

fn shore() -> World {
    World::from_scenario(ScenarioKind::LastBoat)
}

fn run_home(w: &mut World) {
    for _ in 0..20_000 {
        if w.homecoming_finished() {
            return;
        }
        w.step();
        w.check_conservation().unwrap();
    }
    panic!("they never all came");
}

fn call_and_wait(w: &mut World) {
    w.apply(Command::CallHome).unwrap();
    run_home(w);
}

fn story(w: &World) -> String {
    w.ending_story().unwrap().join(" ")
}

#[test]
fn the_boat_needs_the_lantern_and_the_kiln() {
    let mut w = World::from_scenario(ScenarioKind::LateFinds);
    let kind = w.defs.building_kind_id("still_boat").unwrap();
    assert!(!w.plannable().contains(&kind));
    let def = w.defs.building(kind);
    assert!(def.requires.iter().any(|r| r == "still_lantern"));
    assert!(def.requires.iter().any(|r| r == "kiln"));
    assert!(
        w.apply(Command::QueueBuilding { kind, site: None })
            .is_err()
    );
}

#[test]
fn there_is_nothing_to_call_them_to_without_a_boat() {
    let mut w = World::from_scenario(ScenarioKind::QuarryStart);
    assert!(!w.can_call_home());
    assert!(w.apply(Command::CallHome).is_err());
    assert!(w.settlement.homecoming.is_none());
}

#[test]
fn everything_you_gave_comes_back_and_they_are_gone() {
    let mut w = shore();
    let skill = w.defs.skill_id("woodcutting").unwrap();
    let before = w.player.xp(skill);
    let gave: u64 = w.spirits.iter().map(|s| s.gift_xp).sum();
    assert_eq!(gave, 1154 + 388);
    call_and_wait(&mut w);
    assert_eq!(w.player.xp(skill), before + gave);
    assert!(w.spirits.is_empty());
    assert!(w.settlement.ended_at.is_some());
}

#[test]
fn they_board_one_at_a_time() {
    let mut w = shore();
    w.apply(Command::CallHome).unwrap();
    let mut seen = 0;
    while !w.homecoming_finished() {
        w.step();
        let now = w.aboard().0;
        assert!(now <= seen + 1 || w.spirits.is_empty());
        seen = now;
    }
}

#[test]
fn what_they_carry_stays_behind_and_the_books_balance() {
    let mut w = shore();
    let logs = w.defs.resource_id("logs").unwrap();
    w.spirits[0].actor.inventory.add(logs, 5);
    *w.ledger.seeded.entry(logs).or_default() += 5;
    let left = w.ledger.left_behind.get(&logs).copied().unwrap_or(0);
    call_and_wait(&mut w);
    assert!(w.ledger.left_behind[&logs] >= left + 5);
    w.check_conservation().unwrap();
}

#[test]
fn the_story_is_a_different_one() {
    let mut w = shore();
    call_and_wait(&mut w);
    let s = story(&w);
    assert!(s.contains("4 spirits crossed the water"), "{s}");
    assert!(s.contains("no hearth"), "{s}");
    assert!(s.contains("back where it began"), "{s}");
    assert!(!s.contains("haven't used it"), "{s}");
}

#[test]
fn a_hearth_already_lit_changes_how_it_reads() {
    let mut w = shore();
    call_and_wait(&mut w);
    w.settlement.homecoming.as_mut().unwrap().hearth_stood = true;
    assert!(story(&w).contains("took them back anyway"));
}

#[test]
fn a_boat_left_at_the_shore_is_part_of_the_hearths_story() {
    let mut w = shore();
    w.settlement.ended_at = Some(w.tick);
    assert!(story(&w).contains("haven't used it"));
}

#[test]
fn a_homecoming_survives_saving_part_way() {
    let mut w = shore();
    w.apply(Command::CallHome).unwrap();
    while w.aboard().0 == 0 {
        w.step();
    }
    assert!(!w.homecoming_finished(), "too late to prove anything");
    let (mut loaded, _) = decode(&encode(&w, "last_boat")).unwrap();
    assert_eq!(loaded.state_hash(), w.state_hash());
    run_home(&mut w);
    run_home(&mut loaded);
    assert_eq!(loaded.state_hash(), w.state_hash());
}

#[test]
fn you_can_start_again_afterwards() {
    let mut w = shore();
    call_and_wait(&mut w);
    let skill = w.defs.skill_id("woodcutting").unwrap();
    let xp = w.defs.xp_table.xp_for_level(5);
    let id = w.soul_separate(skill, xp).unwrap();
    assert_eq!(id, 0);
    w.run_ticks(600);
    w.check_conservation().unwrap();
    let (loaded, _) = decode(&encode(&w, "last_boat")).unwrap();
    assert_eq!(loaded.spirits.len(), 1);
}

#[test]
fn nobody_can_hold_it_up_for_ever() {
    let mut w = shore();
    w.apply(Command::CallHome).unwrap();
    // Nobody can reach the boat, so it comes down to the time limit.
    for s in &mut w.spirits {
        s.next_think = u64::MAX;
    }
    w.run_ticks(100);
    assert_eq!(w.aboard().0, 0);
    w.tick += w.defs.construction.homecoming_limit_ticks;
    w.run_ticks(10);
    assert!(w.homecoming_finished());
}

#[test]
fn you_can_only_call_them_once() {
    let mut w = shore();
    w.apply(Command::CallHome).unwrap();
    assert!(w.apply(Command::CallHome).is_err());
}

#[test]
fn a_builder_can_be_called_off_a_project() {
    let mut w = shore();
    let kind = w.defs.building_kind_id("storehouse").unwrap();
    let main = w.main_storage;
    let planks = w.defs.resource_id("planks").unwrap();
    let put = w.buildings[main].storage.deposit(planks, 40);
    *w.ledger.seeded.entry(planks).or_default() += put as u64;
    w.apply(Command::QueueBuilding { kind, site: None })
        .unwrap();
    w.run_ticks(100);
    assert!(w.projects.iter().any(|p| p.assigned.is_some()));
    call_and_wait(&mut w);
    assert!(w.projects.iter().all(|p| p.assigned.is_none()));
    let (loaded, _) = decode(&encode(&w, "last_boat")).unwrap();
    assert!(loaded.spirits.is_empty());
}
