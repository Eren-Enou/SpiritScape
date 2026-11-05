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
