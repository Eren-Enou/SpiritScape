//! The fourth trade, and raw materials kept in stock with nothing asking.

use spirit_sim::ScenarioKind;
use spirit_sim::save::{decode, encode};
use spirit_sim::world::{Command, World};

fn yard() -> World {
    World::from_scenario(ScenarioKind::KilnYard)
}

fn run_until(w: &mut World, max: u64, done: impl Fn(&World) -> bool) -> bool {
    for _ in 0..max {
        if done(w) {
            return true;
        }
        w.step();
        w.check_conservation().unwrap();
    }
    done(w)
}

fn stock(w: &mut World, id: &str, n: u32) {
    let r = w.defs.resource_id(id).unwrap();
    let main = w.main_storage;
    w.buildings[main].storage.capacity = 2000;
    let put = w.buildings[main].storage.deposit(r, n);
    *w.ledger.seeded.entry(r).or_default() += put as u64;
}

fn built(w: &World, id: &str) -> bool {
    let kind = w.defs.building_kind_id(id).unwrap();
    w.buildings
        .iter()
        .any(|b| b.kind == kind && b.is_complete())
}

fn plan(w: &mut World, id: &str) -> Result<String, String> {
    let kind = w.defs.building_kind_id(id).unwrap();
    w.apply(Command::QueueBuilding { kind, site: None })
}

#[test]
fn bricks_take_charcoal_first_and_the_books_balance() {
    let mut w = yard();
    let bricks = w.defs.resource_id("bricks").unwrap();
    let charcoal = w.defs.resource_id("charcoal").unwrap();
    w.apply(Command::SetOrderTarget {
        output: bricks,
        target: 10,
    })
    .unwrap();
    assert!(run_until(&mut w, 30_000, |w| w.main().storage.get(bricks) >= 10));
    assert!(w.ledger.crafted[&charcoal] >= 5, "it came from somewhere");
}

#[test]
fn bricks_need_a_spirit_of_level_five() {
    let w = yard();
    let bricks = w.defs.recipes.iter().find(|r| r.id == "bricks").unwrap();
    assert_eq!(bricks.level_required, 5);
    assert_eq!(w.spirits[0].level(), 5);
}

#[test]
fn the_kiln_waits_for_the_clay_pits() {
    let mut w = World::from_scenario(ScenarioKind::LateFinds);
    assert!(
        plan(&mut w, "clay_pits").is_err(),
        "that's the seventh find"
    );
    w.settlement.caravans = 7;
    assert!(plan(&mut w, "kiln").is_err());
    stock(&mut w, "planks", 30);
    plan(&mut w, "clay_pits").unwrap();
    let banks = |w: &World| {
        let clay = w.defs.resource_id("clay");
        let n = w
            .trees
            .iter()
            .filter(|t| Some(w.defs.tree(t.kind).resource) == clay);
        n.count()
    };
    assert_eq!(banks(&w), 0);
    assert!(run_until(&mut w, 8000, |w| built(w, "clay_pits")));
    assert_eq!(banks(&w), 6);
    plan(&mut w, "kiln").unwrap();
}

#[test]
fn a_standing_stock_gives_a_quarrying_spirit_something_to_do() {
    let mut w = World::from_scenario(ScenarioKind::QuarryStart);
    let stone = w.defs.resource_id("stone").unwrap();
    let before = w.main().storage.get(stone);
    w.run_ticks(600);
    assert_eq!(w.main().storage.get(stone), before, "nothing asks for it");
    w.apply(Command::SetStandingStock {
        resource: stone,
        target: before + 20,
    })
    .unwrap();
    assert!(run_until(&mut w, 12_000, |w| w.main().storage.get(stone)
        >= before + 20));
    let reached = w.main().storage.get(stone);
    w.run_ticks(1200);
    assert!(
        w.main().storage.get(stone) <= reached + 12,
        "and then it stops"
    );
}

#[test]
fn a_standing_stock_is_capped_by_the_store_and_survives_saving() {
    let mut w = yard();
    let stone = w.defs.resource_id("stone").unwrap();
    w.apply(Command::SetStandingStock {
        resource: stone,
        target: 100_000,
    })
    .unwrap();
    let room = w.main().storage.capacity;
    assert_eq!(w.settlement.standing[&stone], room);
    let (loaded, _) = decode(&encode(&w, "kiln_yard")).unwrap();
    assert_eq!(loaded.settlement.standing, w.settlement.standing);
    w.apply(Command::SetStandingStock {
        resource: stone,
        target: 0,
    })
    .unwrap();
    assert!(w.settlement.standing.is_empty());
}

/// How long a depleted tree takes to come back through one stretch of winter.
fn winter_regrowth(walls: bool) -> u64 {
    let mut w = World::new_journey("normal").unwrap();
    let tpm = w.defs.ticks_per_second as u64 * 60;
    let mut winter = None;
    for minute in 0..600 {
        w.tick = minute * tpm;
        if w.season().is_some_and(|s| s.growth == 0) {
            winter = Some(w.tick);
            break;
        }
    }
    w.tick = winter.unwrap();
    if walls {
        let kind = w.defs.building_kind_id("warm_walls").unwrap();
        let main = w.main_storage;
        w.buildings[main].kind = kind;
    }
    let at = w.tick + 10_000;
    let i = w
        .trees
        .iter()
        .position(|t| w.defs.tree(t.kind).seasonal)
        .unwrap();
    w.trees[i].state = spirit_sim::entities::TreeState::Depleted { regrow_at: at };
    w.run_ticks(100);
    match w.trees[i].state {
        spirit_sim::entities::TreeState::Depleted { regrow_at } => regrow_at - w.tick,
        _ => 0,
    }
}

#[test]
fn warm_walls_keep_things_recovering_through_winter() {
    let cold = winter_regrowth(false);
    let warm = winter_regrowth(true);
    assert_eq!(cold, 10_000, "frozen: no nearer than it was");
    assert!(warm < cold && warm >= 9_940, "half speed: {warm}");
}

#[test]
fn the_chimney_waits_for_the_hearth_and_changes_the_story() {
    let mut w = World::from_scenario(ScenarioKind::LastStage);
    assert!(plan(&mut w, "hearth_chimney").is_err());
    let chimney = w.defs.building_kind_id("hearth_chimney").unwrap();
    assert!(
        w.defs
            .building(chimney)
            .requires
            .iter()
            .any(|r| r == "hearth_of_spirits")
    );
    let line = w.defs.building(chimney).ending_line.clone().unwrap();
    assert!(line.contains("valley"));
}

/// It used to say "Clay Bank: in . 6 Clay each" at Quarrying level 3, hours
/// before there was any clay in the world.
#[test]
fn clay_is_not_mentioned_before_there_is_any() {
    let mut w = World::from_scenario(ScenarioKind::LateFinds);
    let quarrying = w.defs.skill_id("quarrying").unwrap();
    let is_clay = |w: &World, k: &spirit_sim::defs::TreeKindId| w.defs.tree(*k).id == "clay_bank";
    assert!(!w.tiers_in_world(quarrying).iter().any(|k| is_clay(&w, k)));
    assert!(w.defs.tree_tiers(quarrying).iter().any(|k| is_clay(&w, k)));
    w.settlement.caravans = 7;
    stock(&mut w, "planks", 30);
    plan(&mut w, "clay_pits").unwrap();
    assert!(run_until(&mut w, 8000, |w| built(w, "clay_pits")));
    assert!(w.tiers_in_world(quarrying).iter().any(|k| is_clay(&w, k)));
}
