//! The storage message, and a reference player who knows nothing else.

use spirit_sim::ScenarioKind;
use spirit_sim::advice::StorageFix;
use spirit_sim::autoplay::{Offers, Style, play_all_goals_styled, play_guided};
use spirit_sim::defs::ResourceId;
use spirit_sim::save::{decode, encode};
use spirit_sim::world::{Command, World};

fn res(w: &World, id: &str) -> ResourceId {
    w.defs.resource_id(id).unwrap()
}

fn stock(w: &mut World, id: &str, n: u32) {
    let r = res(w, id);
    let main = w.main_storage;
    let put = w.buildings[main].storage.deposit(r, n);
    *w.ledger.seeded.entry(r).or_default() += put as u64;
}

/// A settlement with plenty of room in Main Storage.
fn roomy() -> World {
    let mut w = World::from_scenario(ScenarioKind::PanelWorkshop);
    let main = w.main_storage;
    w.buildings[main].storage.capacity = 750;
    w
}

/// Planks up to the room Main Storage keeps, with more than a bag free.
fn kept_room() -> World {
    let mut w = roomy();
    let room = w.defs.construction.raw_room;
    let free = w.main().storage.free();
    stock(&mut w, "planks", free - room + 40);
    w
}

#[test]
fn nothing_is_offered_while_there_is_room() {
    let w = roomy();
    assert!(w.storage_fixes().is_empty());
    assert_eq!(w.storage_advice(), None);
}

#[test]
fn the_message_speaks_up_when_the_kept_room_stops_the_player() {
    let w = kept_room();
    let bag = w.defs.player.carry_capacity;
    assert!(w.main().storage.free() >= bag, "more than a bag is free");
    assert!(!w.storage_fixes().is_empty());
    assert!(w.storage_advice().is_some());
}

#[test]
fn a_message_says_everything_on_the_list() {
    let w = kept_room();
    let advice = w.storage_advice().unwrap();
    assert!(advice.contains("lower the plank target"), "{advice}");
    assert!(advice.contains("make Fitted Panels"), "{advice}");
    assert!(advice.contains("plan the Grove Lane"), "{advice}");
    assert!(advice.contains("order a Storehouse"), "{advice}");
    assert_eq!(w.storage_fixes().len(), 4);
}

#[test]
fn logs_filling_the_store_get_different_advice() {
    let mut w = roomy();
    let free = w.main().storage.free();
    stock(&mut w, "logs", free);
    let fixes = w.storage_fixes();
    assert_eq!(fixes[0], StorageFix::RaisePlankTarget);
    assert!(!fixes.contains(&StorageFix::LowerPlankTarget));
}

#[test]
fn the_warning_is_given_once_and_not_every_tick() {
    let mut w = kept_room();
    let count = |w: &World| {
        let n = w.notices.iter().filter(|n| n.text.contains("Main Storage"));
        n.count()
    };
    let before = count(&w);
    w.run_ticks(300);
    assert_eq!(count(&w), before + 1);
}

/// A store that won't take anything, and a bag of logs.
fn stuck() -> World {
    let mut w = roomy();
    let free = w.main().storage.free();
    stock(&mut w, "panels", free);
    let logs = res(&w, "logs");
    w.player.actor.inventory.add(logs, 10);
    *w.ledger.seeded.entry(logs).or_default() += 10;
    w
}

#[test]
fn a_load_can_be_left_only_when_nowhere_will_take_it() {
    let mut w = roomy();
    let logs = res(&w, "logs");
    w.player.actor.inventory.add(logs, 10);
    *w.ledger.seeded.entry(logs).or_default() += 10;
    assert!(w.apply(Command::LeaveLoad).is_err());
    assert_eq!(w.player.actor.inventory.get(logs), 10);
}

#[test]
fn leaving_a_load_loses_it_and_the_books_still_balance() {
    let mut w = stuck();
    let logs = res(&w, "logs");
    assert!(w.load_is_stuck());
    assert!(w.storage_fixes().contains(&StorageFix::LeaveLoad));
    w.apply(Command::LeaveLoad).unwrap();
    assert_eq!(w.player.actor.inventory.get(logs), 0);
    assert_eq!(w.ledger.left_behind.get(&logs).copied(), Some(10));
    w.check_conservation().unwrap();
    assert!(!w.load_is_stuck());
}

#[test]
fn what_was_left_survives_saving() {
    let mut w = stuck();
    w.apply(Command::LeaveLoad).unwrap();
    let (loaded, _) = decode(&encode(&w, "panel_workshop")).unwrap();
    loaded.check_conservation().unwrap();
}

/// A whole game, so it's release only.
#[test]
#[ignore]
fn the_storage_message_alone_is_enough_to_reach_the_crossing() {
    let (w, r) = play_guided("normal").unwrap();
    w.check_conservation().unwrap();
    assert_eq!(r.roads.len(), 4);
    assert!(r.advice_taken >= 5, "{}", r.advice_taken);
    let at = |name: &str| r.steps.iter().find(|s| s.0 == name).unwrap().1;
    assert!(at("Forest Crossing finished") < 3 * 3600);
    let main = &w.main().storage;
    assert!(main.free() > 100, "{} free", main.free());
}

/// Many hours of play, so it's release only.
#[test]
#[ignore]
fn every_goal_can_be_met_in_one_game() {
    let (w, r) = spirit_sim::autoplay::play_all_goals(60).unwrap();
    w.check_conservation().unwrap();
    assert!(r.missed.is_empty(), "{:?}", r.missed);
}

/// A bench holding planks for the player used to look like a met target, so
/// a project a few planks short waited for ever. The winter planner found it.
#[test]
fn planks_set_aside_for_training_are_not_stock() {
    let mut w = World::from_scenario(ScenarioKind::PanelWorkshop);
    let planks = res(&w, "planks");
    let bench = w.stations().next().unwrap().id;
    let panels = w.defs.recipes.iter().position(|r| r.id == "fitted_panels");
    let recipe = spirit_sim::defs::RecipeId(panels.unwrap() as u16);
    w.apply(Command::SetTrainingBench(Some(bench))).unwrap();
    w.apply(Command::SetPlayerRecipe(Some(recipe))).unwrap();
    let before = w.projected_stock(planks);
    let storage = &mut w.buildings[bench].storage;
    let put = storage.deposit(planks, 14);
    assert!(storage.try_reserve(planks, put));
    *w.ledger.seeded.entry(planks).or_default() += put as u64;
    assert_eq!(w.bench_stock(), put);
    assert_eq!(w.projected_stock(planks), before);
}

fn came(w: &World, id: &str) -> bool {
    let at = w.defs.visitors.iter().position(|v| v.id == id).unwrap();
    w.settlement.visitors_seen.contains(&(at as u32))
}

/// Many hours of play, so it's release only.
#[test]
#[ignore]
fn someone_who_takes_offers_meets_people_twice() {
    let style = Style {
        offers: Offers::Take,
        ..Style::default()
    };
    let (w, r) = play_all_goals_styled(60, style).unwrap();
    w.check_conservation().unwrap();
    assert!(r.missed.is_empty(), "{:?}", r.missed);
    assert!(w.settlement.offers_taken >= 3);
    assert!(came(&w, "pedlar_returns"));
    assert!(!came(&w, "surveyor_son"), "nobody turned the surveyor down");
}

/// Many hours of play, so it's release only.
#[test]
#[ignore]
fn someone_who_turns_everyone_down_still_finishes() {
    let style = Style {
        offers: Offers::Refuse,
        ..Style::default()
    };
    let (w, r) = play_all_goals_styled(60, style).unwrap();
    w.check_conservation().unwrap();
    assert!(r.missed.is_empty(), "{:?}", r.missed);
    assert_eq!(w.settlement.offers_taken, 0);
    assert!(came(&w, "surveyor_son"));
    assert!(!came(&w, "pedlar_returns"));
}
