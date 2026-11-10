//! The player's notes: what they've been told, for when they've forgotten.

use spirit_sim::world::{Command, World};

fn game() -> World {
    World::new_journey("normal").unwrap()
}

fn section(w: &World, title: &str) -> Vec<String> {
    let found = w.notes().into_iter().find(|s| s.title.contains(title));
    found.map_or(Vec::new(), |s| s.lines)
}

#[test]
fn a_new_game_has_no_notes_at_all() {
    assert!(game().notes().is_empty());
}

#[test]
fn a_tip_stays_in_your_notes_after_it_has_faded() {
    let mut w = game();
    w.settlement.tips_seen = Some(vec!["first_winter".into()]);
    let told = section(&w, "been told");
    assert_eq!(told.len(), 1);
    assert!(told[0].contains("trees won't grow back"), "{}", told[0]);
    w.notices = Default::default();
    assert_eq!(section(&w, "been told"), told);
}

#[test]
fn notes_keep_the_order_things_were_said() {
    let mut w = game();
    w.settlement.tips_seen = Some(vec!["first_ice".into(), "first_winter".into()]);
    let told = section(&w, "been told");
    assert!(told[0].contains("frozen"));
}

#[test]
fn a_tip_that_no_longer_exists_is_left_out() {
    let mut w = game();
    w.settlement.tips_seen = Some(vec!["gone".into(), "first_winter".into()]);
    assert_eq!(section(&w, "been told").len(), 1);
}

#[test]
fn caravan_finds_are_listed_in_the_order_they_came() {
    let mut w = game();
    assert!(section(&w, "caravans").is_empty());
    w.settlement.caravans = 3;
    let found = section(&w, "caravans");
    assert_eq!(found.len(), 3);
    assert!(found[0].contains("Ironwood saplings"), "{}", found[0]);
    assert!(found[2].contains("mason's tools"), "{}", found[2]);
}

#[test]
fn every_visitor_is_remembered_with_what_you_said() {
    let mut w = game();
    w.settlement.visitors_met = 3;
    w.settlement.offers_accepted.push("pedlar".into());
    w.settlement.offers_refused.push("surveyor".into());
    let people = section(&w, "come through");
    assert_eq!(people.len(), 3);
    assert_eq!(people[0], "A pedlar: you took the offer.");
    assert_eq!(people[1], "A surveyor: you turned them down.");
    assert_eq!(people[2], "A shipwright: they left without an answer.");
}

#[test]
fn someone_who_came_out_of_turn_is_there_too() {
    let mut w = game();
    w.settlement.visitors_seen.push(5);
    assert_eq!(section(&w, "come through").len(), 1);
}

#[test]
fn someone_still_here_is_waiting() {
    let mut w = game();
    let tpm = w.defs.ticks_per_second as u64 * 60;
    w.tick = w.defs.visitors[0].after_minutes * tpm;
    w.run_ticks(101);
    assert!(section(&w, "come through")[0].contains("waiting for an answer"));
    w.apply(Command::AnswerVisitor(false)).unwrap();
    assert!(section(&w, "come through")[0].contains("turned them down"));
}

#[test]
fn the_autumn_tip_tells_you_what_winter_will_cost() {
    let w = game();
    let tip = w.defs.tips.iter().find(|t| t.id == "first_autumn").unwrap();
    assert_eq!(tip.when, "season:autumn");
    assert!(tip.text.contains("logs"));
    assert!(w.tip_condition_known(&tip.when));
}
