//! Masonry: the third trade, and the first recipe with two kinds of input.

use spirit_sim::ScenarioKind;
use spirit_sim::defs::{RecipeId, ResourceId};
use spirit_sim::save::{decode, encode};
use spirit_sim::world::{Command, World};

fn world() -> World {
    World::from_scenario(ScenarioKind::MasonYard)
}

fn res(w: &World, id: &str) -> ResourceId {
    w.defs.resource_id(id).unwrap()
}

fn want(w: &mut World, id: &str, target: u32) {
    let output = res(w, id);
    w.apply(Command::SetOrderTarget { output, target }).unwrap();
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

#[test]
fn a_mason_dresses_stone_to_a_target() {
    let mut w = world();
    let dressed = res(&w, "dressed_stone");
    want(&mut w, "dressed_stone", 10);
    assert!(run_until(&mut w, 6000, |w| w.main().storage.get(dressed) >= 10));
}

#[test]
fn keystones_need_both_trades_and_the_chain_fills_itself() {
    let mut w = world();
    let (keys, beams) = (res(&w, "keystones"), res(&w, "beams"));
    let before = w.main().storage.get(beams);
    want(&mut w, "keystones", 5);
    assert!(run_until(&mut w, 20000, |w| w.main().storage.get(keys) >= 5));
    assert!(w.main().storage.get(beams) <= before - 5);
}

#[test]
fn a_keystone_order_does_nothing_without_a_skilled_mason() {
    let mut w = World::from_scenario(ScenarioKind::ExpandedCentral);
    let keys = res(&w, "keystones");
    want(&mut w, "keystones", 5);
    assert_eq!(w.deficit_of(keys), 0, "woodworkers aren't masons");
}

#[test]
fn woodworkers_stay_out_of_the_yard() {
    let w = world();
    let dressed = res(&w, "dressed_stone");
    assert!(w.stations_for(dressed).len() == 1);
    assert!(
        w.recipe_for(res(&w, "planks")).is_none(),
        "no woodworker here"
    );
}

#[test]
fn the_yard_stays_shut_until_a_third_caravan() {
    let mut w = World::from_scenario(ScenarioKind::CaravanYard);
    let yard = w.defs.building_kind_id("mason_yard").unwrap();
    w.settlement.caravans = 2;
    assert!(!w.plannable().contains(&yard));
    w.settlement.caravans = 3;
    assert!(w.plannable().contains(&yard));
}

#[test]
fn the_player_can_train_masonry_at_the_yard() {
    let mut w = world();
    let skill = w.defs.skill_id("masonry").unwrap();
    let stone = res(&w, "stone");
    let main = w.main_storage;
    w.buildings[main].storage.withdraw_available(stone, 20);
    w.player.actor.inventory.add(stone, 20);
    let yard = w.stations_for(res(&w, "dressed_stone"))[0].pos;
    let before = w.player.xp(skill);
    w.apply(Command::PlayerInteract(yard)).unwrap();
    assert!(run_until(&mut w, 1500, |w| w.player.xp(skill) >= before + 60));
}

#[test]
fn soul_separation_makes_a_masonry_spirit() {
    let mut w = world();
    let skill = w.defs.skill_id("masonry").unwrap();
    w.apply(Command::SoulSeparate { skill, xp: 174 }).unwrap();
    let s = w.spirits.last().unwrap();
    assert_eq!(w.defs.role(s.role).id, "mason");
}

#[test]
fn keystones_are_only_for_level_eight() {
    let w = world();
    let at = w
        .defs
        .recipes
        .iter()
        .position(|r| r.id == "keystones")
        .unwrap();
    assert_eq!(w.defs.recipe(RecipeId(at as u16)).level_required, 8);
    assert_eq!(w.defs.recipe(RecipeId(at as u16)).inputs.len(), 2);
}

#[test]
fn a_yard_survives_saving_and_continues_identically() {
    let mut w = world();
    want(&mut w, "dressed_stone", 10);
    w.run_ticks(600);
    let (mut loaded, _) = decode(&encode(&w, "mason_yard")).unwrap();
    w.run_ticks(900);
    loaded.run_ticks(900);
    assert_eq!(w.state_hash(), loaded.state_hash());
}

#[test]
fn older_saves_gain_orders_for_the_new_goods() {
    let path = format!(
        "{}/tests/fixtures/v7_panels.save",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = String::from_utf8(std::fs::read(path).unwrap()).unwrap();
    let (mut w, _) = decode(&text).unwrap();
    assert!(w.order_for(res(&w, "keystones")).is_some());
    w.run_ticks(300);
    w.check_conservation().unwrap();
}
