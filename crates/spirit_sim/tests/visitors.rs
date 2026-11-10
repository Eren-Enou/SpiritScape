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

/// Moves on to one visitor by id, as if the ones before had been and gone.
fn due(w: &mut World, id: &str) {
    let at = w.defs.visitors.iter().position(|v| v.id == id).unwrap();
    w.settlement.visitors_met = at as u32;
    let tpm = w.defs.ticks_per_second as u64 * 60;
    w.tick = w.defs.visitors[at].after_minutes * tpm;
}

#[test]
fn someone_you_dealt_with_comes_back() {
    let mut w = game();
    w.settlement.offers_accepted.push("pedlar".into());
    due(&mut w, "pedlar_returns");
    w.run_ticks(101);
    assert_eq!(w.visitor().unwrap().name, "The pedlar, again");
}

#[test]
fn someone_you_never_dealt_with_does_not() {
    let mut w = game();
    due(&mut w, "pedlar_returns");
    w.run_ticks(101);
    assert_ne!(
        w.visitor().map(|v| v.name),
        Some("The pedlar, again".to_string())
    );
}

#[test]
fn turning_someone_down_can_bring_someone_else() {
    let mut w = game();
    w.settlement.offers_refused.push("surveyor".into());
    due(&mut w, "surveyor_son");
    w.run_ticks(101);
    assert_eq!(w.visitor().unwrap().name, "A surveyor's son");
}

#[test]
fn a_refusal_is_remembered_by_name() {
    let mut w = with_visitor();
    w.apply(Command::AnswerVisitor(false)).unwrap();
    assert_eq!(w.settlement.offers_refused, vec!["pedlar".to_string()]);
    let (loaded, _) = decode(&encode(&w, "new_game")).unwrap();
    assert_eq!(
        loaded.settlement.offers_refused,
        w.settlement.offers_refused
    );
}

#[test]
fn someone_waiting_on_a_building_holds_nobody_up() {
    let mut w = game();
    w.settlement.offers_accepted.push("pedlar".into());
    w.settlement.visitors_seen.push(0);
    w.settlement.visitors_met = 1;
    let at = w
        .defs
        .visitors
        .iter()
        .position(|v| v.id == "pedlar_returns");
    let tpm = w.defs.ticks_per_second as u64 * 60;
    w.tick = w.defs.visitors[at.unwrap()].after_minutes * tpm;
    w.run_ticks(101);
    assert_eq!(w.visitor().unwrap().name, "The pedlar, again");
    assert_eq!(w.settlement.visitors_met, 1, "the surveyor can still come");
}

#[test]
fn nobody_comes_twice_after_coming_out_of_turn() {
    let mut w = game();
    w.settlement.offers_accepted.push("pedlar".into());
    due(&mut w, "pedlar_returns");
    w.settlement.visitors_met = 1;
    w.run_ticks(101);
    w.apply(Command::AnswerVisitor(false)).unwrap();
    let tpm = w.defs.ticks_per_second as u64 * 60;
    w.tick += w.defs.construction.visitor_gap_minutes * tpm;
    w.run_ticks(200);
    assert!(w.visitor().is_none());
}

#[test]
fn visitors_do_not_arrive_on_each_others_heels() {
    let mut w = game();
    w.settlement.offers_accepted.push("pedlar".into());
    due(&mut w, "pedlar_returns");
    w.settlement.visitor_last = Some(w.tick);
    w.run_ticks(101);
    assert!(w.visitor().is_none());
    let tpm = w.defs.ticks_per_second as u64 * 60;
    w.tick += w.defs.construction.visitor_gap_minutes * tpm;
    w.run_ticks(101);
    assert!(w.visitor().is_some());
}

#[test]
fn someone_can_have_heard_how_you_treated_someone_else() {
    let said = |took: bool, refused: bool| {
        let mut w = game();
        let at = w.defs.visitors.iter().position(|v| v.id == "apprentice");
        if took {
            w.settlement.offers_accepted.push("mason".into());
        }
        if refused {
            w.settlement.offers_refused.push("mason".into());
        }
        w.settlement.visitor = Some(spirit_sim::visitors::VisitorHere {
            index: at.unwrap() as u32,
            arrived: w.tick,
        });
        w.visitor().unwrap().text
    };
    let (took, refused, neither) = (said(true, false), said(false, true), said(false, false));
    assert!(took.contains("died"), "{took}");
    assert!(refused.contains("sent them away"), "{refused}");
    assert_ne!(neither, took);
    assert_ne!(neither, refused);
}

#[test]
fn every_visitor_who_remembers_names_someone_real() {
    let w = game();
    for v in &w.defs.visitors {
        for id in [&v.remembers, &v.refused, &v.knows].into_iter().flatten() {
            let at = w.defs.visitors.iter().position(|o| &o.id == id);
            let me = w.defs.visitors.iter().position(|o| o.id == v.id);
            assert!(at.is_some(), "{} remembers {id}", v.id);
            assert!(at < me, "{} comes before {id}", v.id);
        }
        if v.knows.is_some() {
            assert!(
                v.text_took.is_some() && v.text_refused.is_some(),
                "{}",
                v.id
            );
        }
    }
}

#[test]
fn saves_from_before_the_list_of_who_came_still_work() {
    // Written by v0.31: a count of who has come, and no list.
    let path = format!(
        "{}/tests/fixtures/v8_journey.save",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = String::from_utf8(std::fs::read(path).unwrap()).unwrap();
    let (mut w, _) = decode(&text).unwrap();
    assert!(w.settlement.visitors_seen.is_empty());
    assert!(w.settlement.offers_accepted.is_empty());
    let met = w.settlement.visitors_met;
    w.run_ticks(600);
    assert!(w.settlement.visitors_met >= met);
    w.check_conservation().unwrap();
}

#[test]
fn visitors_alone_never_light_the_hearth() {
    // Taking every offer must still leave Keystones to cut. Those from the far
    // shore come too late to count: the lantern needs a crossing that costs 4.
    let w = game();
    let given: u32 = w
        .defs
        .visitors
        .iter()
        .filter(|v| v.needs_built.as_deref() != Some("still_lantern"))
        .flat_map(|v| &v.gives)
        .filter(|a| a.resource == "keystones")
        .map(|a| a.amount)
        .sum();
    let keystones = res(&w, "keystones");
    let kind = w.defs.building_kind_id("hearth_of_spirits").unwrap();
    let needed = w.defs.building(kind).cost.iter().find(|c| c.0 == keystones);
    assert!(given < needed.unwrap().1, "{given}");
}

#[test]
fn a_lesson_gives_real_xp_and_says_so() {
    let mut w = with_visitor();
    assert!(
        w.visitor()
            .unwrap()
            .offer
            .contains("learn something of Woodcutting")
    );
    let def = w.defs.visitors[0].clone();
    stock(&mut w, &def.wants[0].resource, def.wants[0].amount);
    let skill = w.defs.skill_id("woodcutting").unwrap();
    let before = w.player.xp(skill);
    w.apply(Command::AnswerVisitor(true)).unwrap();
    assert_eq!(w.player.xp(skill), before + def.teaches.unwrap().xp);
    assert!(w.notices.contains("a thing or two"));
}

#[test]
fn turning_someone_down_teaches_you_nothing() {
    let mut w = with_visitor();
    let skill = w.defs.skill_id("woodcutting").unwrap();
    let before = w.player.xp(skill);
    w.apply(Command::AnswerVisitor(false)).unwrap();
    assert_eq!(w.player.xp(skill), before);
}

#[test]
fn every_lesson_is_in_a_real_skill() {
    let w = game();
    for v in &w.defs.visitors {
        if let Some(l) = &v.teaches {
            assert!(w.defs.skill_id(&l.skill).is_some(), "{}", v.id);
            assert!(l.xp > 0, "{}", v.id);
        }
    }
}

#[test]
fn what_you_said_ends_up_in_the_story() {
    let story = |took: bool| {
        let mut w = game();
        w.settlement.ended_at = Some(1);
        let list = if took {
            &mut w.settlement.offers_accepted
        } else {
            &mut w.settlement.offers_refused
        };
        list.push("pilgrim".into());
        w.ending_story().unwrap().join(" ")
    };
    assert!(story(true).contains("The pilgrim got home."));
    assert!(story(false).contains("never did find out"));
}

#[test]
fn someone_you_never_answered_is_not_in_the_story() {
    let mut w = game();
    w.settlement.ended_at = Some(1);
    let story = w.ending_story().unwrap().join(" ");
    assert!(!story.contains("pilgrim"));
}

#[test]
fn the_story_is_not_a_ledger() {
    let mut w = game();
    w.settlement.ended_at = Some(1);
    let ids: Vec<String> = w.defs.visitors.iter().map(|v| v.id.clone()).collect();
    w.settlement.offers_accepted = ids;
    let told = |w: &World, l: &String| {
        w.defs
            .visitors
            .iter()
            .any(|v| v.ending_took.as_ref() == Some(l))
    };
    let story = w.ending_story().unwrap();
    assert_eq!(story.iter().filter(|l| told(&w, l)).count(), 3);
}
