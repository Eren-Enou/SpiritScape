//! The storage message, and a reference player who knows nothing else.

use spirit_sim::ScenarioKind;
use spirit_sim::advice::StorageFix;
use spirit_sim::autoplay::play_guided;
use spirit_sim::defs::ResourceId;
use spirit_sim::world::World;

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
