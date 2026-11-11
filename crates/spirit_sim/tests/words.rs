//! How the game puts things into words.

use spirit_sim::ScenarioKind;
use spirit_sim::world::World;

/// "1 Planks" is wrong wherever it turns up.
fn one_with_a_plural(w: &World, text: &str) -> Option<String> {
    for r in &w.defs.resources {
        if r.one.is_empty() {
            continue;
        }
        let wrong = format!("1 {}", r.name);
        for (i, _) in text.match_indices(&wrong) {
            // "11 Planks" and "21 Planks" are fine.
            let before = text[..i].chars().last();
            if !before.is_some_and(|c| c.is_ascii_digit()) {
                return Some(wrong);
            }
        }
    }
    None
}

#[test]
fn every_resource_with_a_plural_name_has_a_singular() {
    let w = World::from_scenario(ScenarioKind::FullMap);
    for r in &w.defs.resources {
        if r.name.ends_with('s') {
            assert!(!r.one.is_empty(), "{} has no singular", r.name);
            assert!(!r.one.ends_with('s'), "{} -> {}", r.name, r.one);
        }
    }
}

#[test]
fn one_of_anything_is_said_in_the_singular() {
    let w = World::from_scenario(ScenarioKind::FullMap);
    let planks = w.defs.resource_id("planks").unwrap();
    assert_eq!(w.defs.amount(planks, 1), "1 Plank");
    assert_eq!(w.defs.amount(planks, 2), "2 Planks");
    assert_eq!(w.defs.amount(planks, 0), "0 Planks");
    let stone = w.defs.resource_id("stone").unwrap();
    assert_eq!(w.defs.amount(stone, 1), "1 Stone");
}

#[test]
fn nothing_the_busy_scenarios_say_has_one_with_a_plural() {
    for kind in [
        ScenarioKind::FullMap,
        ScenarioKind::KilnYard,
        ScenarioKind::MasonYard,
        ScenarioKind::PanelWorkshop,
    ] {
        let mut w = World::from_scenario(kind);
        w.run_ticks(4000);
        for e in w.events.iter() {
            assert_eq!(one_with_a_plural(&w, &e.text), None, "{kind:?}: {}", e.text);
        }
        for n in w.notices.iter() {
            assert_eq!(one_with_a_plural(&w, &n.text), None, "{kind:?}: {}", n.text);
        }
        for option in w.recipe_options() {
            let line = format!("{} -> {}", option.inputs, option.output);
            assert_eq!(one_with_a_plural(&w, &line), None, "{kind:?}: {line}");
        }
    }
}

#[test]
fn one_spirit_made_is_said_in_the_singular_at_the_end() {
    // The cold camp scenario has a lit Hearth and one spirit made from your XP.
    let mut w = World::from_scenario(ScenarioKind::ColdCamp);
    let made = w.spirits.iter().filter(|s| s.gift_xp > 0).count();
    assert_eq!(made, 1);
    let ending = w.ending_story().unwrap().join(" ");
    assert!(
        ending.contains("to make 1 spirit, and it's still here, in it."),
        "{ending}"
    );
    let stone = w.stone_words(0).unwrap().join(" ");
    assert!(stone.contains("to make 1 helper."), "{stone}");
    w.apply(spirit_sim::world::Command::CarveStone(0)).unwrap();
    // And more than one stays plural.
    let w = World::from_scenario(ScenarioKind::Midwinter);
    let ending = w.ending_story().unwrap().join(" ");
    assert!(
        ending.contains("spirits, and it's still here, in them."),
        "{ending}"
    );
}
