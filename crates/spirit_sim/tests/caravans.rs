//! Trade Caravans: the one thing that uses goods without end.

use spirit_sim::ScenarioKind;
use spirit_sim::advice::StorageFix;
use spirit_sim::defs::{BuildingKindId, GameDefs};
use spirit_sim::entities::BuildingState;
use spirit_sim::save::{decode, encode};
use spirit_sim::world::{Command, World};

fn world() -> World {
    World::from_scenario(ScenarioKind::CaravanYard)
}

fn kind(w: &World) -> BuildingKindId {
    w.defs.building_kind_id("trade_caravan").unwrap()
}

fn send(w: &mut World) {
    let kind = kind(w);
    w.apply(Command::QueueBuilding { kind, site: None })
        .unwrap();
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
fn a_caravan_takes_real_goods_away() {
    let mut w = world();
    let (logs, panels) = (
        w.defs.resource_id("logs").unwrap(),
        w.defs.resource_id("panels").unwrap(),
    );
    send(&mut w);
    assert!(run_until(&mut w, 6000, |w| w.settlement.caravans == 1));
    assert_eq!(w.main().storage.get(logs), 40);
    assert_eq!(w.main().storage.get(panels), 25);
    assert_eq!(w.ledger.consumed.get(&logs).copied(), Some(200));
}

#[test]
fn a_caravan_leaves_nothing_behind() {
    let mut w = world();
    send(&mut w);
    let site = w.active_project().unwrap().site;
    let b = w.active_project().unwrap().building;
    assert!(w.grid.is_occupied(site));
    assert!(run_until(&mut w, 6000, |w| w.settlement.caravans == 1));
    assert!(!w.grid.is_occupied(site));
    assert_eq!(w.buildings[b].state, BuildingState::Removed);
    assert!(w.building_at(site).is_none());
}

#[test]
fn caravans_can_be_sent_again_and_again() {
    let mut w = world();
    let logs = w.defs.resource_id("logs").unwrap();
    let panels = w.defs.resource_id("panels").unwrap();
    for n in 1..=3 {
        let main = w.main_storage;
        let put = w.buildings[main].storage.deposit(logs, 200);
        *w.ledger.seeded.entry(logs).or_default() += put as u64;
        let put = w.buildings[main].storage.deposit(panels, 20);
        *w.ledger.seeded.entry(panels).or_default() += put as u64;
        send(&mut w);
        assert!(run_until(&mut w, 6000, |w| w.settlement.caravans == n));
    }
    assert!(w.plannable().contains(&kind(&w)));
}

#[test]
fn normal_rules_charge_more_panels_but_the_same_logs() {
    let normal = GameDefs::builtin_profile("normal").unwrap();
    let def = normal.building(normal.building_kind_id("trade_caravan").unwrap());
    let cost = |id: &str| {
        let r = normal.resource_id(id).unwrap();
        def.cost.iter().find(|c| c.0 == r).unwrap().1
    };
    assert_eq!((cost("logs"), cost("panels")), (200, 80));
}

#[test]
fn the_message_offers_one_only_when_it_can_be_paid_for() {
    let mut w = world();
    let logs = w.defs.resource_id("logs").unwrap();
    let main = w.main_storage;
    let free = w.main().storage.free();
    let put = w.buildings[main].storage.deposit(logs, free);
    *w.ledger.seeded.entry(logs).or_default() += put as u64;
    let offered = |w: &World| {
        w.storage_fixes()
            .iter()
            .any(|f| matches!(f, StorageFix::SendCaravan(_)))
    };
    assert!(offered(&w));
    assert!(w.storage_advice().unwrap().contains("send a Trade Caravan"));
    let panels = w.defs.resource_id("panels").unwrap();
    let gone = w.buildings[main].storage.withdraw_available(panels, 45);
    *w.ledger.seeded.entry(panels).or_default() -= gone as u64;
    assert!(!offered(&w));
}

#[test]
fn caravans_count_toward_a_goal() {
    let mut w = world();
    w.settlement.caravans = 4;
    let goal = |w: &World| {
        let all = w.ambitions();
        all.into_iter()
            .find(|a| a.title.contains("Caravans"))
            .unwrap()
    };
    assert!(!goal(&w).done);
    assert_eq!(goal(&w).progress.unwrap().0, 4);
    w.settlement.caravans = 5;
    assert!(goal(&w).done);
}

#[test]
fn the_count_survives_saving_and_continues_identically() {
    let mut w = world();
    send(&mut w);
    assert!(run_until(&mut w, 6000, |w| w.settlement.caravans == 1));
    let (mut loaded, _) = decode(&encode(&w, "caravan_yard")).unwrap();
    assert_eq!(loaded.settlement.caravans, 1);
    w.run_ticks(300);
    loaded.run_ticks(300);
    assert_eq!(w.state_hash(), loaded.state_hash());
}

#[test]
fn a_caravan_can_be_called_off_before_it_is_loaded() {
    let mut w = world();
    send(&mut w);
    let pid = w.active_project().unwrap().id;
    w.apply(Command::CancelProject(pid)).unwrap();
    assert_eq!(w.settlement.caravans, 0);
    send(&mut w);
}

fn plan_named(w: &mut World, id: &str) -> Result<String, String> {
    let kind = w.defs.building_kind_id(id).unwrap();
    w.apply(Command::QueueBuilding { kind, site: None })
}

fn stock(w: &mut World, id: &str, n: u32) {
    let r = w.defs.resource_id(id).unwrap();
    let main = w.main_storage;
    w.buildings[main].storage.capacity = 750;
    let put = w.buildings[main].storage.deposit(r, n);
    *w.ledger.seeded.entry(r).or_default() += put as u64;
}

#[test]
fn a_find_stays_shut_until_a_caravan_brings_it() {
    let mut w = world();
    let e = plan_named(&mut w, "ironwood_nursery").unwrap_err();
    assert!(e.contains("Trade Caravan"), "{e}");
    let nursery = w.defs.building_kind_id("ironwood_nursery").unwrap();
    assert!(!w.plannable().contains(&nursery));
}

#[test]
fn the_first_caravan_brings_back_saplings() {
    let mut w = world();
    send(&mut w);
    assert!(run_until(&mut w, 6000, |w| w.settlement.caravans == 1));
    w.step();
    assert!(w.notices.contains("Ironwood saplings"));
    let nursery = w.defs.building_kind_id("ironwood_nursery").unwrap();
    assert!(w.plannable().contains(&nursery));
    let quarry = w.defs.building_kind_id("quarry_extension").unwrap();
    assert!(!w.plannable().contains(&quarry), "that takes two");
}

#[test]
fn a_nursery_plants_real_trees_that_can_be_cut() {
    let mut w = world();
    w.settlement.caravans = 1;
    stock(&mut w, "planks", 40);
    let count = w.trees.len();
    plan_named(&mut w, "ironwood_nursery").unwrap();
    let kind = w.defs.building_kind_id("ironwood_nursery").unwrap();
    assert!(run_until(&mut w, 6000, |w| w
        .buildings
        .iter()
        .any(|b| b.kind == kind && b.is_complete())));
    assert_eq!(w.trees.len(), count + 3);
    let t = w.trees.last().unwrap();
    assert_eq!(w.defs.tree(t.kind).id, "ironwood");
    assert_eq!(t.site, w.defs.site_id("starting_grove"));
    assert!(w.grid.is_occupied(t.pos));
}

#[test]
fn a_tile_built_on_since_is_left_alone() {
    let mut w = world();
    w.settlement.caravans = 1;
    stock(&mut w, "planks", 40);
    let blocked = spirit_sim::grid::TilePos::new(8, 8);
    w.grid.set_occupied(blocked, true);
    let count = w.trees.len();
    plan_named(&mut w, "ironwood_nursery").unwrap();
    let kind = w.defs.building_kind_id("ironwood_nursery").unwrap();
    assert!(run_until(&mut w, 6000, |w| w
        .buildings
        .iter()
        .any(|b| b.kind == kind && b.is_complete())));
    assert_eq!(w.trees.len(), count + 2);
}

#[test]
fn planted_trees_survive_saving_and_continue_identically() {
    let mut w = world();
    w.settlement.caravans = 1;
    stock(&mut w, "planks", 40);
    plan_named(&mut w, "ironwood_nursery").unwrap();
    w.run_ticks(2500);
    let (mut loaded, _) = decode(&encode(&w, "caravan_yard")).unwrap();
    assert_eq!(loaded.trees.len(), w.trees.len());
    w.run_ticks(600);
    loaded.run_ticks(600);
    assert_eq!(w.state_hash(), loaded.state_hash());
}

#[test]
fn every_planted_tile_is_open_ground_on_the_map() {
    let w = World::from_scenario(ScenarioKind::NewGame);
    for b in w.defs.buildings.iter() {
        for (_, [x, y]) in &b.plants {
            assert!(
                w.grid.is_buildable(spirit_sim::grid::TilePos::new(*x, *y)),
                "{} at {x},{y}",
                b.id
            );
        }
    }
}

#[test]
fn every_find_is_described() {
    let w = world();
    for b in w.defs.buildings.iter().filter(|b| b.needs_caravans > 0) {
        assert!(b.caravan_find.len() > 10, "{}", b.id);
    }
}

#[test]
fn each_of_the_first_six_caravans_opens_one_more_thing() {
    let w = world();
    let mut needs: Vec<u32> = w
        .defs
        .buildings
        .iter()
        .filter(|b| b.needs_caravans > 0)
        .map(|b| b.needs_caravans)
        .collect();
    needs.sort();
    assert_eq!(needs, vec![1, 2, 3, 4, 5, 6]);
}

#[test]
fn a_late_find_can_be_built() {
    let mut w = world();
    w.settlement.caravans = 4;
    stock(&mut w, "logs", 40);
    let count = w.trees.len();
    plan_named(&mut w, "oak_saplings").unwrap();
    let kind = w.defs.building_kind_id("oak_saplings").unwrap();
    assert!(run_until(&mut w, 6000, |w| w
        .buildings
        .iter()
        .any(|b| b.kind == kind && b.is_complete())));
    assert_eq!(w.trees.len(), count + 3);
}

#[test]
fn each_late_find_does_what_it_says() {
    let mut w = World::from_scenario(ScenarioKind::LateFinds);
    let main = w.main_storage;
    let room = w.buildings[main].storage.capacity;
    let bench = w.settlement.bench_bonus;
    for id in ["oak_saplings", "long_racks", "stone_cellar"] {
        plan_named(&mut w, id).unwrap();
    }
    let done = |w: &World, id: &str| {
        let k = w.defs.building_kind_id(id).unwrap();
        w.buildings.iter().any(|b| b.kind == k && b.is_complete())
    };
    let all = |w: &World| {
        ["oak_saplings", "long_racks", "stone_cellar"]
            .iter()
            .all(|id| done(w, id))
    };
    assert!(run_until(&mut w, 30_000, all));
    assert_eq!(w.buildings[main].storage.capacity, room + 800);
    assert_eq!(w.settlement.bench_bonus, bench + 60);
    assert!(
        w.notices.contains("Room for 60 more"),
        "not the Log Rack's words"
    );
}
