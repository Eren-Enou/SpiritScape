//! Visitors: offers that need an answer.

use spirit_sim::ScenarioKind;
use spirit_sim::defs::ResourceId;
use spirit_sim::save::{decode, encode};
use spirit_sim::world::{Command, World};

fn game() -> World {
    World::new_journey("normal").unwrap()
}

fn res(w: &World, id: &str) -> ResourceId {
    w.defs.resource_id(id).unwrap()
}

fn stock(w: &mut World, id: &str, n: u32) {
    let r = res(w, id);
    let main = w.main_storage;
    w.buildings[main].storage.capacity = 2000;
    let put = w.buildings[main].storage.deposit(r, n);
    *w.ledger.seeded.entry(r).or_default() += put as u64;
}

/// Brings the first visitor without playing to them.
fn with_visitor() -> World {
    let mut w = game();
    let tpm = w.defs.ticks_per_second as u64 * 60;
    w.tick = w.defs.visitors[0].after_minutes * tpm;
    w.run_ticks(101);
    assert!(w.visitor().is_some());
    w
}

#[test]
fn every_offer_names_real_goods_and_has_a_cost() {
    let w = game();
    assert!(w.defs.visitors.len() >= 5);
    for v in &w.defs.visitors {
        for a in v.wants.iter().chain(&v.gives) {
            assert!(w.defs.resource_id(&a.resource).is_some(), "{}", v.id);
        }
        assert!(!v.wants.is_empty(), "{} asks for nothing", v.id);
    }
}

#[test]
fn scenarios_have_no_visitors() {
    let mut w = World::from_scenario(ScenarioKind::ExpandedRemote);
    w.tick = 3_000_000;
    w.run_ticks(200);
    assert!(w.visitor().is_none());
}

#[test]
fn a_visitor_arrives_with_an_offer_and_says_so() {
    let w = with_visitor();
    let v = w.visitor().unwrap();
    assert!(v.offer.starts_with("Give "), "{}", v.offer);
    assert!(w.notices.contains("has arrived"));
}

#[test]
fn an_offer_cannot_be_taken_without_the_goods() {
    let mut w = with_visitor();
    assert!(!w.visitor().unwrap().can_accept);
    assert!(w.apply(Command::AnswerVisitor(true)).is_err());
    assert!(w.visitor().is_some(), "they wait");
}

#[test]
fn taking_an_offer_moves_real_goods_and_the_books_balance() {
    let mut w = with_visitor();
    let def = w.defs.visitors[0].clone();
    stock(&mut w, &def.wants[0].resource, def.wants[0].amount);
    let got = res(&w, &def.gives[0].resource);
    let before = w.main().storage.get(got);
    w.apply(Command::AnswerVisitor(true)).unwrap();
    assert!(w.visitor().is_none());
    assert_eq!(w.main().storage.get(got), before + def.gives[0].amount);
    assert_eq!(w.settlement.offers_taken, 1);
    w.check_conservation().unwrap();
}

#[test]
fn turning_one_down_costs_nothing() {
    let mut w = with_visitor();
    let total = w.main().storage.total();
    w.apply(Command::AnswerVisitor(false)).unwrap();
    assert!(w.visitor().is_none());
    assert_eq!(w.main().storage.total(), total);
    assert_eq!(w.settlement.offers_taken, 0);
}

#[test]
fn nobody_stays_for_ever() {
    let mut w = with_visitor();
    let tpm = w.defs.ticks_per_second as u64 * 60;
    w.tick += w.defs.construction.visitor_patience_minutes * tpm;
    w.run_ticks(101);
    assert!(w.notices.contains("has moved on"));
}

#[test]
fn each_comes_once_and_the_next_waits_their_turn() {
    let mut w = with_visitor();
    w.apply(Command::AnswerVisitor(false)).unwrap();
    assert_eq!(w.settlement.visitors_met, 1);
    w.run_ticks(200);
    assert!(w.visitor().is_none(), "the next one isn't due");
}

#[test]
fn a_visitor_who_needs_a_building_waits_for_it() {
    let mut w = game();
    let at = w.defs.visitors.iter().position(|v| v.needs_built.is_some());
    w.settlement.visitors_met = at.unwrap() as u32;
    w.tick = 40_000_000;
    w.run_ticks(200);
    assert!(w.visitor().is_none());
}

#[test]
fn a_waiting_visitor_survives_saving() {
    let w = with_visitor();
    let (loaded, _) = decode(&encode(&w, "new_game")).unwrap();
    assert_eq!(loaded.visitor(), w.visitor());
}

#[test]
fn nothing_is_lost_when_storage_cannot_hold_what_they_give() {
    let mut w = with_visitor();
    let def = w.defs.visitors[0].clone();
    stock(&mut w, &def.wants[0].resource, def.wants[0].amount);
    let main = w.main_storage;
    let keep = w.main().storage.total();
    w.buildings[main].storage.capacity = keep;
    w.apply(Command::AnswerVisitor(true)).unwrap();
    w.check_conservation().unwrap();
}
