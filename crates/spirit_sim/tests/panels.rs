//! Fitted Panels: the first recipe that uses a finished good, and what they buy.

use spirit_sim::ScenarioKind;
use spirit_sim::defs::{BuildingKindId, RecipeId, ResourceId};
use spirit_sim::save::{SAVE_VERSION, SaveStore, Slot, decode, encode};
use spirit_sim::world::{Command, World};

fn world() -> World {
    World::from_scenario(ScenarioKind::PanelWorkshop)
}

fn res(w: &World, id: &str) -> ResourceId {
    w.defs.resource_id(id).unwrap()
}

fn kind(w: &World, id: &str) -> BuildingKindId {
    w.defs.building_kind_id(id).unwrap()
}

fn panels_recipe(w: &World) -> RecipeId {
    let at = w.defs.recipes.iter().position(|r| r.id == "fitted_panels");
    RecipeId(at.unwrap() as u16)
}

fn plan(w: &mut World, id: &str) -> Result<String, String> {
    let kind = kind(w, id);
    w.apply(Command::QueueBuilding { kind, site: None })
}

fn built(w: &World, id: &str) -> bool {
    let k = kind(w, id);
    w.buildings.iter().any(|b| b.kind == k && b.is_complete())
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
    let r = res(w, id);
    let main = w.main_storage;
    let put = w.buildings[main].storage.deposit(r, n);
    *w.ledger.seeded.entry(r).or_default() += put as u64;
}

fn fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    String::from_utf8(std::fs::read(path).unwrap()).unwrap()
}

fn woodworking(w: &World) -> u64 {
    w.player.xp(w.defs.skill_id("woodworking").unwrap())
}

/// Supported training with panels chosen, working at the Workshop.
fn supported() -> World {
    let mut w = world();
    let bench = w.training_bench().unwrap().pos;
    let r = panels_recipe(&w);
    w.apply(Command::SetPlayerRecipe(Some(r))).unwrap();
    w.apply(Command::SetTrainingSupply(true)).unwrap();
    w.apply(Command::PlayerInteract(bench)).unwrap();
    w
}

#[test]
fn panels_are_never_made_unless_someone_asks() {
    let mut w = world();
    let planks = res(&w, "planks");
    let station = w.training_bench().unwrap().id;
    let bag = &mut w.player.actor.inventory;
    bag.add(planks, 9);
    let main = w.main_storage;
    w.buildings[main].storage.withdraw_available(planks, 9);
    let made = w.player_recipe_at(station);
    assert_ne!(made, Some(panels_recipe(&w)));
    assert_eq!(
        w.player_recipe_at(station),
        None,
        "planks alone make nothing"
    );
}

#[test]
fn a_new_level_does_not_change_what_the_player_makes() {
    let w = world();
    let options = w.recipe_options();
    assert_eq!(options.len(), 4);
    let station = w.training_bench().unwrap();
    assert_eq!(
        w.defs
            .best_recipe(station.kind, 20, |_| 1000)
            .map(|r| w.defs.recipe(r).id.clone()),
        Some("structural_beams".to_string())
    );
}

#[test]
fn panels_need_level_seven() {
    let mut w = World::from_scenario(ScenarioKind::WorkshopManual);
    let r = panels_recipe(&w);
    let option = w.recipe_options().into_iter().find(|o| o.id == r).unwrap();
    assert!(!option.unlocked);
    assert_eq!(option.level_required, 7);
    assert!(w.apply(Command::SetPlayerRecipe(Some(r))).is_err());
}

#[test]
fn panels_are_the_best_xp_a_minute_short_of_beams() {
    let w = world();
    let options = w.recipe_options();
    let xp = |id: &str| {
        options
            .iter()
            .find(|o| w.defs.recipe(o.id).id == id)
            .unwrap()
            .xp_per_min
    };
    assert!(xp("fitted_panels") > xp("precise_planks"));
    assert!(xp("fitted_panels") < xp("structural_beams"));
}

#[test]
fn training_on_panels_uses_three_planks_for_each() {
    let mut w = supported();
    let (planks, panels) = (res(&w, "planks"), res(&w, "panels"));
    let before = woodworking(&w);
    assert!(run_until(&mut w, 6000, |w| {
        let bench = w.training_bench().unwrap();
        w.main().storage.get(panels) + bench.storage.get(panels) >= 5
    }));
    assert!(woodworking(&w) >= before + 450);
    let used = w.ledger.processed.get(&planks).copied().unwrap_or(0);
    assert!(used >= 15, "{used}");
}

#[test]
fn a_hauler_brings_planks_for_training_and_takes_panels_home() {
    let mut w = supported();
    let panels = res(&w, "panels");
    assert!(run_until(&mut w, 12000, |w| w.main().storage.get(panels) >= 5));
}

#[test]
fn the_bench_never_clogs_with_panels_while_training_is_supported() {
    let mut w = supported();
    let before = woodworking(&w);
    w.run_ticks(9000);
    w.check_conservation().unwrap();
    // 15 minutes at 90 XP a batch, with some waiting.
    assert!(woodworking(&w) > before + 3000, "{}", woodworking(&w));
}

#[test]
fn spirits_make_panels_for_a_plan_and_stop_when_it_is_built() {
    let mut w = world();
    let panels = res(&w, "panels");
    plan(&mut w, "panelled_warehouse").unwrap();
    assert!(run_until(&mut w, 30000, |w| built(w, "panelled_warehouse")));
    assert_eq!(w.main().storage.capacity, 600);
    w.run_ticks(1200);
    let bench = w.training_bench().unwrap();
    let left = w.main().storage.get(panels) + bench.storage.get(panels);
    assert!(left <= 1, "{left} left over");
}

#[test]
fn spirits_make_panels_to_a_target_without_carrying_planks_to_and_fro() {
    let mut w = world();
    let (planks, panels) = (res(&w, "planks"), res(&w, "panels"));
    let start = w.main().storage.get(planks);
    w.apply(Command::SetOrderTarget {
        output: panels,
        target: 10,
    })
    .unwrap();
    assert!(run_until(&mut w, 24000, |w| w.main().storage.get(panels) >= 10));
    let spent = w.ledger.processed.get(&planks).copied().unwrap_or(0) as u32;
    assert!(spent >= 30);
    let at_bench = w.training_bench().unwrap().storage.get(planks);
    assert!(w.main().storage.get(planks) + at_bench + 30 >= start - 5);
    let hauled: u32 = w.spirits.iter().map(|s| s.trips).sum();
    assert!(hauled < 40, "{hauled} trips for 10 panels");
}

/// The workshop with no planks at all, and logs to make them from.
fn no_planks() -> World {
    let mut w = world();
    let planks = res(&w, "planks");
    let main = w.main_storage;
    let had = w.buildings[main].storage.withdraw_available(planks, 200);
    *w.ledger.seeded.entry(planks).or_default() -= had as u64;
    stock(&mut w, "logs", 150);
    w
}

#[test]
fn a_panel_plan_makes_the_planks_it_needs() {
    let mut w = no_planks();
    stock(&mut w, "beams", 0);
    plan(&mut w, "carrying_frames").unwrap();
    assert!(run_until(&mut w, 30000, |w| built(w, "carrying_frames")));
}

#[test]
fn asking_for_panels_raises_the_plank_goal_by_a_capped_amount() {
    let mut w = no_planks();
    let (planks, panels) = (res(&w, "planks"), res(&w, "panels"));
    let before = w.goal_of(planks);
    w.apply(Command::SetOrderTarget {
        output: panels,
        target: 5,
    })
    .unwrap();
    assert_eq!(w.goal_of(planks), before + 15);
    w.apply(Command::SetOrderTarget {
        output: panels,
        target: 500,
    })
    .unwrap();
    assert_eq!(w.goal_of(planks), before + w.defs.production.upstream_cap);
}

#[test]
fn the_plank_goal_falls_back_when_panels_are_done() {
    let mut w = no_planks();
    let (planks, panels) = (res(&w, "planks"), res(&w, "panels"));
    let before = w.goal_of(planks);
    w.apply(Command::SetOrderTarget {
        output: panels,
        target: 2,
    })
    .unwrap();
    assert!(run_until(&mut w, 20000, |w| w.main().storage.get(panels) >= 2));
    assert_eq!(w.goal_of(planks), before);
}

#[test]
fn a_plan_says_who_is_making_panels() {
    let mut w = world();
    plan(&mut w, "carrying_frames").unwrap();
    let views = w.plan_views();
    let line = views[0].needs.iter().find(|n| n.name == "Fitted Panels");
    assert!(line.is_some());
}

#[test]
fn carrying_frames_add_to_every_spirit_now_and_later() {
    let mut w = world();
    let before: Vec<u32> = w
        .spirits
        .iter()
        .map(|s| s.actor.inventory.capacity)
        .collect();
    stock(&mut w, "panels", 10);
    plan(&mut w, "carrying_frames").unwrap();
    assert!(run_until(&mut w, 12000, |w| built(w, "carrying_frames")));
    for (s, was) in w.spirits.iter().zip(before) {
        assert_eq!(s.actor.inventory.capacity, was + 5, "{}", s.name);
    }
    let skill = w.defs.skill_id("woodcutting").unwrap();
    let count = w.spirits.len();
    w.apply(Command::SoulSeparate { skill, xp: 100 }).unwrap();
    assert_eq!(w.spirits.len(), count + 1);
    let new = w.spirits.last().unwrap();
    assert_eq!(new.actor.inventory.capacity, 15);
}

#[test]
fn normal_rules_charge_four_times_the_goods() {
    let normal = spirit_sim::defs::GameDefs::builtin_profile("normal").unwrap();
    let k = normal.building_kind_id("panelled_warehouse").unwrap();
    let panels = normal.resource_id("panels").unwrap();
    let cost = normal.building(k).cost.iter().find(|c| c.0 == panels);
    assert_eq!(cost.unwrap().1, 60);
}

#[test]
fn storage_advice_offers_panels_to_someone_who_can_make_them() {
    let mut w = world();
    let free = w.main().storage.free();
    stock(&mut w, "planks", free);
    let advice = w.storage_advice().expect("full");
    assert!(
        advice.contains("make Fitted Panels at your bench"),
        "{advice}"
    );
}

#[test]
fn storage_advice_does_not_offer_panels_too_early() {
    let mut w = World::from_scenario(ScenarioKind::FullStorage);
    let free = w.main().storage.free();
    stock(&mut w, "planks", free);
    if let Some(advice) = w.storage_advice() {
        assert!(!advice.contains("Fitted Panels"), "{advice}");
    }
}

#[test]
fn storage_advice_moves_on_to_warehouses_after_storehouses() {
    let mut w = world();
    let store = kind(&w, "storehouse");
    stock(&mut w, "logs", 120);
    for _ in 0..3 {
        w.apply(Command::QueueBuilding {
            kind: store,
            site: None,
        })
        .unwrap();
    }
    let done = move |w: &World| {
        w.buildings
            .iter()
            .filter(|b| b.kind == store && b.is_complete())
            .count()
            == 3
    };
    assert!(run_until(&mut w, 20000, done));
    let free = w.main().storage.free();
    stock(&mut w, "planks", free);
    let advice = w.storage_advice().expect("full");
    assert!(advice.contains("plan a Panelled Warehouse"), "{advice}");
}

#[test]
fn panels_survive_saving_and_continue_identically() {
    let mut w = supported();
    w.run_ticks(1500);
    let (mut loaded, _) = decode(&encode(&w, "panel_workshop")).unwrap();
    w.run_ticks(1500);
    loaded.run_ticks(1500);
    assert_eq!(w.state_hash(), loaded.state_hash());
}

#[test]
fn the_carry_bonus_survives_saving() {
    let mut w = world();
    stock(&mut w, "panels", 10);
    plan(&mut w, "carrying_frames").unwrap();
    assert!(run_until(&mut w, 12000, |w| built(w, "carrying_frames")));
    let (loaded, _) = decode(&encode(&w, "panel_workshop")).unwrap();
    assert_eq!(loaded.settlement.carry_bonus, 5);
}

#[test]
fn older_saves_gain_an_order_for_panels_and_nothing_else_changes() {
    for name in ["v6_road.save", "v5_crossing.save", "v4_outpost.save"] {
        let (mut w, _) = decode(&fixture(name)).unwrap();
        let panels = res(&w, "panels");
        let order = w.order_for(panels).expect(name);
        assert_eq!(order.target, 0, "{name}");
        w.run_ticks(600);
        w.check_conservation().unwrap();
    }
}

#[test]
fn a_v0_10_road_under_way_finishes_after_upgrading() {
    let (mut w, meta) = decode(&fixture("v6_road.save")).unwrap();
    assert_eq!(meta.version, 6);
    assert!(built(&w, "grove_lane"));
    assert!(w.active_project().is_some());
    assert!(run_until(&mut w, 6000, |w| built(w, "north_road")));
}

#[test]
fn an_upgraded_save_keeps_its_original() {
    let dir = std::env::temp_dir().join(format!("spiritscape_v11_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("world.save"), fixture("v6_road.save")).unwrap();
    let store = SaveStore::new(&dir);
    let loaded = store.load(Slot::Manual).unwrap();
    assert!(loaded.warning.unwrap().contains("world.save.v6"));
    store
        .write(Slot::Manual, &loaded.world, "plank_surplus")
        .unwrap();
    assert_eq!(store.meta(Slot::Manual).unwrap().version, SAVE_VERSION);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Hours of play, so it's release only.
#[test]
#[ignore]
fn long_training_keeps_going_with_panels_and_clogs_without_them() {
    use spirit_sim::autoplay::train_on;
    let (without, with) = (train_on(false, 120).unwrap(), train_on(true, 120).unwrap());
    assert_eq!(
        without.main_used, without.main_capacity,
        "planks fill the store"
    );
    assert!(with.main_used < with.main_capacity * 2 / 3);
    assert!(with.xp > without.xp * 3);
    assert_eq!(with.full_bag_seconds, 0);
}

/// Found by a long game: planks set aside for training filled the bench's
/// shelf, and the worker there stopped for good.
#[test]
fn planks_set_aside_for_training_do_not_stop_a_bench_making_planks() {
    let mut w = world();
    w.apply(Command::SetProductionTarget(400)).unwrap();
    stock(&mut w, "logs", 60);
    let planks = res(&w, "planks");
    let station = w.training_bench().unwrap().id;
    let b = &mut w.buildings[station];
    b.storage.deposit(planks, 20);
    assert!(b.storage.try_reserve(planks, 20));
    *w.ledger.seeded.entry(planks).or_default() += 20;
    let before = w.ledger.crafted.get(&planks).copied().unwrap_or(0);
    w.run_ticks(1500);
    assert!(w.ledger.crafted.get(&planks).copied().unwrap_or(0) > before);
}
