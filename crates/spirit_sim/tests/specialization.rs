//! v0.6: advanced tiers, workstation places, several workshops, simultaneous
//! orders, spirit instructions, infrastructure-backed training and upgrading
//! saves from v0.5.

use spirit_sim::autoplay::{compare_chopping, compare_sawing};
use spirit_sim::defs::{BUILTIN_DEFS, BuildingKindId, GameDefs, JobKind, ResourceId, SkillId};
use spirit_sim::entities::{Focus, ProjectState, SpiritPolicy};
use spirit_sim::grid::TilePos;
use spirit_sim::metrics::WaitKind;
use spirit_sim::production::ProductionBottleneck;
use spirit_sim::save::{self, SAVE_VERSION, SaveStore, Slot};
use spirit_sim::tasks::Task;
use spirit_sim::{Command, ScenarioKind, World};

const V1_JOURNEY: &str = include_str!("fixtures/v1_journey.save");
const V1_WORKSHOP: &str = include_str!("fixtures/v1_workshop.save");

fn res(w: &World, id: &str) -> ResourceId {
    w.defs.resource_id(id).unwrap()
}

fn wc(w: &World) -> SkillId {
    w.defs.skill_id("woodcutting").unwrap()
}

fn ww(w: &World) -> SkillId {
    w.defs.skill_id("woodworking").unwrap()
}

fn workshop(w: &World) -> BuildingKindId {
    w.defs.building_kind_id("workshop").unwrap()
}

fn made(w: &World, id: &str) -> u64 {
    w.ledger.crafted.get(&res(w, id)).copied().unwrap_or(0)
}

fn run_until(w: &mut World, max: u64, mut done: impl FnMut(&World) -> bool) -> bool {
    for _ in 0..max {
        if done(w) {
            return true;
        }
        w.step();
    }
    done(w)
}

fn run_checked(w: &mut World, ticks: u64) {
    for _ in 0..ticks {
        w.step();
        w.check_conservation().unwrap();
    }
}

/// Adds stock to Main Storage, accounted for as if it had been seeded.
fn stock(w: &mut World, id: &str, n: u32) {
    let r = res(w, id);
    let main = w.main_storage;
    w.buildings[main].storage.capacity += n;
    w.buildings[main].storage.deposit(r, n);
    *w.ledger.seeded.entry(r).or_default() += n as u64;
}

fn ironwood(w: &World) -> TilePos {
    w.trees
        .iter()
        .find(|t| w.defs.tree(t.kind).id == "ironwood")
        .unwrap()
        .pos
}

fn policy(w: &mut World, spirit: usize, change: impl FnOnce(&mut SpiritPolicy)) {
    let mut p = w.spirits[spirit].policy;
    change(&mut p);
    w.apply(Command::SetSpiritPolicy { spirit, policy: p })
        .unwrap();
}

/// The Expansion scenario's line, already running two Workshops.
fn two_workshops() -> World {
    let mut w = World::from_scenario(ScenarioKind::ExpandedProduction);
    let kind = workshop(&w);
    assert!(run_until(&mut w, 3000, |w| {
        w.stations().filter(|b| b.kind == kind).count() == 2
    }));
    w
}

// --- Advanced tiers ---------------------------------------------------------

#[test]
fn ironwood_needs_level_ten_for_the_player() {
    let mut w = World::from_scenario(ScenarioKind::AdvancedSpecialization);
    let tree = ironwood(&w);
    let refused = w.apply(Command::PlayerInteract(tree)).unwrap_err();
    assert!(refused.contains("Woodcutting level 10"), "{refused}");
}

#[test]
fn hardwood_is_a_distinct_resource_from_a_renewable_site() {
    let w = World::from_scenario(ScenarioKind::NewGame);
    let site = w.defs.site_id("old_growth").unwrap();
    let trees: Vec<_> = w.trees.iter().filter(|t| t.site == Some(site)).collect();
    assert!(trees.len() >= 6);
    for t in trees {
        let def = w.defs.tree(t.kind);
        assert_eq!(def.resource, res(&w, "hardwood"));
        assert!(def.regrow_ticks > 0);
    }
    // Reachable on foot from Main Storage, over the bridge.
    let field = w.grid.distance_field(&[w.main().pos]);
    assert!(w.grid.approach_distance(&field, ironwood(&w)).is_some());
}

#[test]
fn only_a_level_ten_gatherer_brings_hardwood() {
    let mut w = World::from_scenario(ScenarioKind::ExpandedProduction);
    let r = res(&w, "hardwood");
    let low = w.gatherer_ids()[0];
    let high = w.gatherer_ids()[1];
    assert!(w.spirits[low].level() < 10 && w.spirits[high].level() >= 10);
    w.apply(Command::SetOrderTarget {
        output: res(&w, "beams"),
        target: 200,
    })
    .unwrap();
    run_checked(&mut w, 2500);
    assert!(w.ledger.harvested.get(&r).copied().unwrap_or(0) > 0);
    assert_eq!(w.spirits[low].actor.inventory.get(r), 0);
    assert!(w.spirits[low].units_gathered == 0 || w.spirits[high].units_gathered > 0);
}

#[test]
fn nobody_gathers_hardwood_without_demand() {
    let mut w = World::from_scenario(ScenarioKind::Demo);
    assert!(w.spirits.iter().any(|s| s.level() >= 10));
    w.run_ticks(3000);
    let r = res(&w, "hardwood");
    assert_eq!(w.ledger.harvested.get(&r).copied().unwrap_or(0), 0);
}

#[test]
fn beams_need_a_level_ten_worker() {
    let mut w = World::from_scenario(ScenarioKind::WorkshopAutomated);
    stock(&mut w, "hardwood", 40);
    let beams = res(&w, "beams");
    w.apply(Command::SetOrderTarget {
        output: beams,
        target: 5,
    })
    .unwrap();
    w.run_ticks(600);
    assert_eq!(made(&w, "beams"), 0);
    let line = w.order_reports();
    let beam = line.iter().find(|o| o.output == beams).unwrap();
    assert!(beam.stalled);
    assert!(beam.status.contains("level 10"), "{}", beam.status);
}

#[test]
fn the_player_makes_beams_only_at_level_ten() {
    let mut w = World::from_scenario(ScenarioKind::AdvancedSpecialization);
    let hardwood = res(&w, "hardwood");
    let main = w.main().pos;
    let bench = w.station().unwrap().pos;
    // Level 9: empty hands take logs, and the bench makes planks.
    w.apply(Command::PlayerInteract(main)).unwrap();
    assert!(run_until(&mut w, 300, |w| w.player.actor.queue.is_empty()));
    assert_eq!(w.player.actor.inventory.get(hardwood), 0);
    w.apply(Command::PlayerInteract(bench)).unwrap();
    let xp_target = 1154;
    assert!(run_until(&mut w, 3000, |w| w
        .player
        .xp(w.defs.skill_id("woodworking").unwrap())
        >= xp_target));
    w.player.actor.clear_tasks();
    // Level 10: unload, and the same empty hands now pick up what the best
    // recipe needs.
    w.apply(Command::PlayerInteract(main)).unwrap();
    assert!(run_until(&mut w, 300, |w| w.player.actor.queue.is_empty()));
    assert_eq!(w.player.actor.inventory.total(), 0);
    w.apply(Command::PlayerInteract(main)).unwrap();
    assert!(run_until(&mut w, 300, |w| w.player.actor.queue.is_empty()));
    assert!(w.player.actor.inventory.get(hardwood) > 0);
    w.apply(Command::PlayerInteract(bench)).unwrap();
    assert!(run_until(&mut w, 800, |w| made(w, "beams") > 0));
    w.check_conservation().unwrap();
    assert!(w.notices.contains("Structural Beams are now available"));
}

#[test]
fn a_sacrifice_loses_the_top_tier_and_the_spirit_keeps_it() {
    let mut w = World::from_scenario(ScenarioKind::AdvancedSpecialization);
    let skill = wc(&w);
    // Reach level 10, and a bit more.
    let mut bot = spirit_sim::autoplay::Bot::new(&w);
    assert!(bot.run(&mut w, spirit_sim::autoplay::Goal::ChopTo(1160), 5000));
    let have = w.player.xp(skill);
    let iron = w.trees.iter().find(|t| t.pos == ironwood(&w)).unwrap().kind;
    assert!(w.player_can_cut(iron));
    let p = w.soul_preview(skill, 1154);
    assert_eq!(p.lost.last(), Some(&iron));
    assert!(p.spirit_can_cut.contains(&iron));
    let option = w.soul_options(skill);
    assert!(option.iter().any(|o| o.label == "Ironwood spirit"));
    let id = w.soul_separate(skill, 1154).unwrap();
    assert_eq!(w.player.xp(skill), have - 1154, "exact transfer");
    assert_eq!(w.spirits[id].xp(), 1154, "exact transfer");
    assert_eq!(w.spirits[id].level(), 10);
    assert!(!w.player_can_cut(iron));

    // The spirit uses what the player gave up, and never gains XP.
    w.apply(Command::SetOrderTarget {
        output: res(&w, "beams"),
        target: 40,
    })
    .ok();
    let beams_hard = res(&w, "hardwood");
    w.main().storage.get(beams_hard);
    assert_eq!(w.spirits[id].xp(), 1154);
}

// --- Workstation places -----------------------------------------------------

#[test]
fn a_workshop_never_holds_more_workers_than_places() {
    let mut w = World::from_scenario(ScenarioKind::ProductionBottleneck);
    let id = w.station().unwrap().id;
    let mut most = 0;
    for _ in 0..1500 {
        w.step();
        most = most.max(w.slots_used(id));
        assert!(w.slots_used(id) <= 2);
    }
    assert_eq!(most, 2);
}

#[test]
fn extra_workers_wait_and_say_why() {
    let mut w = World::from_scenario(ScenarioKind::ProductionBottleneck);
    w.run_ticks(300);
    let waiting: Vec<_> = w
        .crafters()
        .filter(|s| s.wait == Some(WaitKind::Station))
        .collect();
    assert_eq!(waiting.len(), 2);
    assert!(
        waiting[0]
            .activity
            .text()
            .contains("Woodworking Spirit waiting: Workshop is occupied"),
        "{}",
        waiting[0].activity.text()
    );
    let r = w.production_report();
    assert_eq!(r.bottleneck, ProductionBottleneck::WorkshopCapacity);
    assert!(
        r.summary.contains(
            "Two Woodworking Spirits are idle. Additional workshop capacity would increase output."
        ),
        "{}",
        r.summary
    );
}

#[test]
fn adding_workers_stops_helping_when_the_places_are_full() {
    let rate = |workers: usize| {
        let mut w = World::from_scenario(ScenarioKind::ProductionBottleneck);
        let ids: Vec<usize> = w.crafters().map(|s| s.id).collect();
        for &spirit in ids.iter().skip(workers) {
            w.apply(Command::SetSpiritAssigned {
                spirit,
                assigned: false,
            })
            .unwrap();
        }
        w.run_ticks(300);
        let before = made(&w, "planks");
        w.run_ticks(600);
        made(&w, "planks") - before
    };
    let (one, two, four) = (rate(1), rate(2), rate(4));
    assert!(two > one * 3 / 2, "{one} {two}");
    assert_eq!(two, four, "the third and fourth workers add nothing");
}

#[test]
fn a_waiting_worker_takes_a_place_when_one_frees() {
    let mut w = World::from_scenario(ScenarioKind::ProductionBottleneck);
    w.run_ticks(300);
    let waiting = w
        .crafters()
        .find(|s| s.wait == Some(WaitKind::Station))
        .unwrap()
        .id;
    let working = w
        .crafters()
        .find(|s| matches!(s.actor.queue.front(), Some(Task::Craft { .. })))
        .unwrap()
        .id;
    w.apply(Command::SetSpiritAssigned {
        spirit: working,
        assigned: false,
    })
    .unwrap();
    policy(&mut w, working, |p| p.focus = Focus::Rest);
    w.spirits[working].actor.clear_tasks();
    assert!(run_until(&mut w, 200, |w| {
        w.spirits[waiting].wait != Some(WaitKind::Station)
    }));
}

#[test]
fn a_full_bench_tells_the_player_and_a_kept_place_makes_room() {
    let mut w = World::from_scenario(ScenarioKind::ProductionBottleneck);
    w.run_ticks(300);
    let id = w.station().unwrap().id;
    let bench = w.buildings[id].pos;
    let logs = res(&w, "logs");
    w.player.actor.inventory.add(logs, 10);
    *w.ledger.seeded.entry(logs).or_default() += 10;
    let refused = w.apply(Command::PlayerInteract(bench)).unwrap_err();
    assert!(refused.contains("places"), "{refused}");

    w.apply(Command::SetKeepBenchPlace(true)).unwrap();
    w.apply(Command::PlayerInteract(bench)).unwrap();
    assert!(w.slots_used(id) <= 2);
    w.run_ticks(100);
    assert!(w.slots_used(id) <= 2, "nobody crowds in behind the player");
    w.check_conservation().unwrap();
}

#[test]
fn a_kept_place_leaves_one_free_for_spirits_to_respect() {
    let mut w = World::from_scenario(ScenarioKind::ProductionBottleneck);
    w.apply(Command::SetKeepBenchPlace(true)).unwrap();
    let id = w.station().unwrap().id;
    for _ in 0..900 {
        w.step();
        assert!(w.slots_used(id) <= 1);
    }
}

// --- More workshops ---------------------------------------------------------

#[test]
fn a_workshop_costs_real_materials_and_goes_where_the_player_says() {
    let mut w = World::from_scenario(ScenarioKind::WorkshopAutomated);
    stock(&mut w, "beams", 6);
    stock(&mut w, "planks", 20);
    stock(&mut w, "logs", 20);
    let kind = workshop(&w);
    let refused = w.apply(Command::RequestBuildingAt {
        kind,
        site: TilePos::new(21, 1),
    });
    assert!(refused.is_err(), "water isn't buildable");
    let tree = w.trees[0].pos;
    assert!(
        w.apply(Command::RequestBuildingAt { kind, site: tree })
            .is_err()
    );
    let site = TilePos::new(8, 19);
    w.apply(Command::RequestBuildingAt { kind, site }).unwrap();
    assert!(run_until(&mut w, 3000, |w| {
        w.projects.last().unwrap().state == ProjectState::Complete
    }));
    let b = w.buildings.iter().find(|b| b.pos == site).unwrap();
    assert!(b.is_complete());
    assert_eq!(w.ledger.consumed.get(&res(&w, "beams")).copied(), Some(6));
    assert_eq!(w.stations().count(), 2);
    assert!(w.notices.contains("Workshop complete"));
    w.check_conservation().unwrap();
}

#[test]
fn the_settlement_makes_the_beams_a_workshop_needs() {
    let mut w = World::from_scenario(ScenarioKind::ExpandedProduction);
    // Take the ready beams away: they have to be made now.
    let main = w.main_storage;
    let beams = res(&w, "beams");
    assert!(w.projects[0].reserved, "ordered with materials ready");
    assert!(w.buildings[main].storage.reserved(beams) > 0);
    assert!(run_until(&mut w, 3000, |w| {
        w.projects[0].state == ProjectState::Complete
    }));
    assert!(run_until(&mut w, 3000, |w| made(w, "beams") > 0));
    w.check_conservation().unwrap();
}

#[test]
fn a_second_workshop_raises_output_when_materials_allow() {
    let rate = |mut w: World| {
        stock(&mut w, "logs", 800);
        w.apply(Command::SetProductionTarget(1500)).unwrap();
        let main = w.main_storage;
        w.buildings[main].storage.capacity += 1500;
        w.run_ticks(300);
        let before = made(&w, "planks");
        w.run_ticks(600);
        made(&w, "planks") - before
    };
    let one = rate(World::from_scenario(ScenarioKind::ProductionBottleneck));
    let two = rate(two_workshops());
    assert!(two > one * 3 / 2, "{one} -> {two}");
}

#[test]
fn both_workshops_are_real_production_sites() {
    let mut w = two_workshops();
    stock(&mut w, "logs", 800);
    w.apply(Command::SetProductionTarget(600)).unwrap();
    let main = w.main_storage;
    w.buildings[main].storage.capacity += 800;
    run_checked(&mut w, 900);
    let reports = w.station_reports();
    assert_eq!(reports.len(), 2);
    assert!(reports.iter().all(|s| s.crafted > 0), "{reports:?}");
}

#[test]
fn haulers_serve_every_workshop_and_collect_from_each() {
    let mut w = two_workshops();
    stock(&mut w, "logs", 800);
    w.apply(Command::SetProductionTarget(100)).unwrap();
    let main = w.main_storage;
    w.buildings[main].storage.capacity += 800;
    let planks = res(&w, "planks");
    let before = w.main().storage.get(planks);
    w.run_ticks(1200);
    assert!(w.main().storage.get(planks) > before);
    let ids: Vec<_> = w.stations().map(|b| b.id).collect();
    for id in ids {
        assert!(w.buildings[id].crafted > 0);
    }
}

// --- Orders -----------------------------------------------------------------

#[test]
fn several_orders_run_at_once_and_stop_at_their_targets() {
    let mut w = two_workshops();
    stock(&mut w, "hardwood", 40);
    stock(&mut w, "logs", 200);
    let beams = res(&w, "beams");
    let planks = res(&w, "planks");
    w.apply(Command::SetProductionTarget(40)).unwrap();
    w.apply(Command::SetOrderTarget {
        output: beams,
        target: 10,
    })
    .unwrap();
    assert!(run_until(&mut w, 6000, |w| {
        w.deficit_of(beams) == 0 && w.deficit_of(planks) == 0
    }));
    w.run_ticks(400);
    let (p, b) = (made(&w, "planks"), made(&w, "beams"));
    assert!(p > 0 && b > 0);
    w.run_ticks(1500);
    assert_eq!(
        (made(&w, "planks"), made(&w, "beams")),
        (p, b),
        "it settles"
    );
    assert!(w.main().storage.get(planks) >= 40);
    assert!(w.main().storage.get(beams) >= 10);
    w.check_conservation().unwrap();
}

#[test]
fn a_shortfall_asks_gatherers_for_the_right_material_within_a_cap() {
    let mut w = World::from_scenario(ScenarioKind::ExpandedProduction);
    let beams = res(&w, "beams");
    let hardwood = res(&w, "hardwood");
    w.apply(Command::SetOrderTarget {
        output: beams,
        target: 500,
    })
    .unwrap();
    assert_eq!(w.upstream_demand(hardwood), w.defs.production.upstream_cap);
    w.apply(Command::SetOrderTarget {
        output: beams,
        target: 0,
    })
    .unwrap();
    assert_eq!(w.gather_goal_of(hardwood), 0);
}

#[test]
fn a_reserve_on_one_input_leaves_the_others_alone() {
    let mut w = two_workshops();
    stock(&mut w, "hardwood", 40);
    stock(&mut w, "logs", 100);
    let beams = res(&w, "beams");
    let hardwood = res(&w, "hardwood");
    let logs = res(&w, "logs");
    w.apply(Command::SetOrderTarget {
        output: beams,
        target: 100,
    })
    .unwrap();
    w.apply(Command::SetOrderReserve {
        output: beams,
        reserve: 20,
    })
    .unwrap();
    w.apply(Command::SetInputReserve(30)).unwrap();
    let mut low_hard = u32::MAX;
    for _ in 0..600 {
        w.step();
        low_hard = low_hard.min(w.main().storage.get(hardwood));
    }
    assert!(low_hard >= 18, "{low_hard}");
    assert!(w.main().storage.get(logs) >= 25);
}

#[test]
fn priority_decides_who_gets_the_scarce_worker() {
    let run = |beams_first: bool| {
        let mut w = World::from_scenario(ScenarioKind::WorkshopAutomated);
        // One level 10 worker, plenty of both inputs, both orders short.
        let id = w.crafters().next().unwrap().id;
        let role = w.spirits[id].role;
        let mut one = World::from_scenario(ScenarioKind::ExpandedProduction);
        let high = one.crafters().find(|s| s.level() >= 10).unwrap().id;
        for s in one.crafters().map(|s| s.id).collect::<Vec<_>>() {
            if s != high {
                one.apply(Command::SetSpiritAssigned {
                    spirit: s,
                    assigned: false,
                })
                .unwrap();
            }
        }
        assert_eq!(one.spirits[high].role, role);
        w = one;
        stock(&mut w, "hardwood", 60);
        stock(&mut w, "logs", 200);
        let beams = res(&w, "beams");
        let planks = res(&w, "planks");
        w.apply(Command::SetOrderTarget {
            output: beams,
            target: 200,
        })
        .unwrap();
        w.apply(Command::SetProductionTarget(500)).unwrap();
        let (a, b) = if beams_first {
            (beams, planks)
        } else {
            (planks, beams)
        };
        w.apply(Command::SetOrderPriority {
            output: a,
            priority: 5,
        })
        .unwrap();
        w.apply(Command::SetOrderPriority {
            output: b,
            priority: 1,
        })
        .unwrap();
        w.run_ticks(1200);
        (made(&w, "planks"), made(&w, "beams"))
    };
    let planks_first = run(false);
    let beams_first = run(true);
    assert!(
        beams_first.1 > planks_first.1,
        "{planks_first:?} {beams_first:?}"
    );
    assert!(
        planks_first.0 > beams_first.0,
        "{planks_first:?} {beams_first:?}"
    );
}

#[test]
fn a_skilled_worker_moves_to_the_hard_job_when_materials_arrive() {
    let mut w = World::from_scenario(ScenarioKind::ExpandedProduction);
    let high = w.crafters().find(|s| s.level() >= 10).unwrap().id;
    let beams = res(&w, "beams");
    w.apply(Command::SetOrderTarget {
        output: beams,
        target: 30,
    })
    .unwrap();
    assert!(run_until(&mut w, 4000, |w| {
        matches!(
            w.spirits[high].actor.queue.front(),
            Some(Task::Craft { output: Some(o), .. }) if *o == beams
        )
    }));
}

#[test]
fn recipes_that_form_a_loop_are_refused_when_data_loads() {
    let text = BUILTIN_DEFS.replacen(
        "inputs = [{ resource = \"logs\", amount = 2 }]",
        "inputs = [{ resource = \"planks\", amount = 2 }]",
        1,
    );
    let e = GameDefs::from_toml(&text).err().unwrap();
    assert!(e.contains("loop"), "{e}");
}

#[test]
fn explanations_name_what_is_missing() {
    let mut w = World::from_scenario(ScenarioKind::ExpandedProduction);
    let beams = res(&w, "beams");
    assert!(run_until(&mut w, 3000, |w| {
        w.projects[0].state == ProjectState::Complete
    }));
    // Nothing to make beams from, and nobody allowed to fetch hardwood.
    let main = w.main_storage;
    let hardwood = res(&w, "hardwood");
    let held = w.buildings[main].storage.available(hardwood);
    w.buildings[main].storage.withdraw_available(hardwood, held);
    *w.ledger.consumed.entry(hardwood).or_default() += held as u64;
    let gatherers: Vec<_> = w.gatherer_ids();
    for g in gatherers {
        policy(&mut w, g, |p| p.focus = Focus::Rest);
    }
    let stations: Vec<_> = w.stations().map(|b| b.id).collect();
    for id in stations {
        let n = w.buildings[id].storage.available(hardwood);
        w.buildings[id].storage.withdraw_available(hardwood, n);
        *w.ledger.consumed.entry(hardwood).or_default() += n as u64;
    }
    w.apply(Command::SetOrderTarget {
        output: beams,
        target: 50,
    })
    .unwrap();
    w.apply(Command::SetProductionTarget(0)).unwrap();
    w.run_ticks(100);
    let reports = w.order_reports();
    let beam = reports.iter().find(|o| o.output == beams).unwrap();
    assert!(
        beam.status
            .contains("Structural Beams production paused: insufficient Hardwood Logs"),
        "{}",
        beam.status
    );
}

// --- Instructions -----------------------------------------------------------

#[test]
fn an_assigned_site_keeps_a_gatherer_there_or_idle() {
    let mut w = World::from_scenario(ScenarioKind::GroveSustainable);
    let forest = w.defs.site_id("distant_forest").unwrap();
    let grove = w.defs.site_id("starting_grove").unwrap();
    let g = w.gatherer_ids()[0];
    // Told to use a closed site, it has nowhere to work.
    policy(&mut w, g, |p| p.site = Some(forest));
    w.run_ticks(300);
    assert_eq!(w.spirits[g].units_gathered, 0);
    policy(&mut w, g, |p| p.site = Some(grove));
    assert!(run_until(&mut w, 800, |w| w.spirits[g].units_gathered > 0));
    assert_eq!(w.spirits[g].home_site, Some(grove));
}

#[test]
fn a_station_assignment_is_respected() {
    let mut w = two_workshops();
    let first = w.station().unwrap().id;
    let ids: Vec<_> = w.crafters().map(|s| s.id).collect();
    for id in &ids {
        w.spirits[*id].actor.clear_tasks();
        policy(&mut w, *id, |p| p.station = Some(first));
    }
    stock(&mut w, "logs", 400);
    w.apply(Command::SetProductionTarget(400)).unwrap();
    let main = w.main_storage;
    w.buildings[main].storage.capacity += 400;
    let other: Vec<_> = w.stations().map(|b| b.id).filter(|b| *b != first).collect();
    let crafted = w.buildings[other[0]].crafted;
    w.run_ticks(900);
    assert_eq!(w.buildings[other[0]].crafted, crafted, "nobody works there");
    assert!(w.buildings[first].crafted > 0);
}

#[test]
fn a_resting_spirit_finishes_its_load_and_stops() {
    let mut w = World::from_scenario(ScenarioKind::WorkshopAutomated);
    w.run_ticks(200);
    let g = w.gatherer_ids()[0];
    policy(&mut w, g, |p| p.focus = Focus::Rest);
    w.run_ticks(800);
    let s = &w.spirits[g];
    assert!(s.actor.queue.is_empty());
    assert_eq!(s.actor.inventory.total(), 0, "it put down what it carried");
    assert!(s.activity.text().contains("resting"));
}

#[test]
fn focus_separates_settlement_work_from_training_support() {
    let mut w = World::from_scenario(ScenarioKind::SupportedRetraining);
    let hauler = w
        .spirits
        .iter()
        .find(|s| w.defs.role(s.role).has_job(JobKind::Supply))
        .unwrap()
        .id;
    w.apply(Command::SetTrainingSupply(true)).unwrap();
    policy(&mut w, hauler, |p| p.focus = Focus::Settlement);
    w.run_ticks(400);
    assert_eq!(w.bench_stock(), 0, "settlement work only");
    policy(&mut w, hauler, |p| p.focus = Focus::Training);
    assert!(run_until(&mut w, 400, |w| w.bench_stock() > 0));
}

#[test]
fn instructions_never_make_a_spirit_capable_of_more() {
    let mut w = World::from_scenario(ScenarioKind::WorkshopAutomated);
    let site = w.defs.site_id("old_growth").unwrap();
    let g = w.gatherer_ids()[0];
    assert!(w.spirits[g].level() < 10);
    policy(&mut w, g, |p| {
        p.site = Some(site);
        p.prefer_advanced = true;
    });
    w.apply(Command::SetObjectiveTarget(200)).unwrap();
    w.run_ticks(1200);
    assert_eq!(
        w.ledger
            .harvested
            .get(&res(&w, "hardwood"))
            .copied()
            .unwrap_or(0),
        0
    );
}

#[test]
fn prefer_advanced_puts_the_top_tier_first() {
    let harvested = |prefer: bool| {
        let mut w = World::from_scenario(ScenarioKind::ExpandedProduction);
        let g = w.gatherer_ids()[1];
        let r = res(&w, "hardwood");
        policy(&mut w, g, |p| p.prefer_advanced = prefer);
        w.apply(Command::SetObjectiveTarget(250)).unwrap();
        w.apply(Command::SetOrderTarget {
            output: res(&w, "beams"),
            target: 100,
        })
        .unwrap();
        w.run_ticks(1500);
        w.ledger.harvested.get(&r).copied().unwrap_or(0)
    };
    assert!(harvested(true) > harvested(false));
}

// --- Training ---------------------------------------------------------------

#[test]
fn infrastructure_improves_training_and_nobody_else_earns_xp() {
    let rows = compare_sawing(3000);
    let alone = &rows[0].stretch;
    let hauled = &rows[1].stretch;
    let built = &rows[3].stretch;
    assert!(
        hauled.actions < alone.actions / 2,
        "low attention is an option"
    );
    assert!(
        built.seconds < alone.seconds,
        "{} {}",
        built.seconds,
        alone.seconds
    );
    assert!(built.actions < alone.actions / 2);
    assert_eq!(built.storage_trips, 0);
}

#[test]
fn woodcutting_support_reduces_walking() {
    let rows = compare_chopping(2000);
    let (alone, built) = (&rows[0].stretch, &rows[2].stretch);
    assert!(built.seconds < 3000, "the run finishes");
    assert!(built.storage_trips > 0);
    assert!(built.player.traveling < alone.player.traveling);
}

#[test]
fn a_log_rack_makes_room_at_the_bench_for_real_logs() {
    let mut w = World::from_scenario(ScenarioKind::SupportedRetraining);
    let rack = w.defs.building_kind_id("log_rack").unwrap();
    w.apply(Command::SetTrainingSupply(true)).unwrap();
    w.apply(Command::RequestBuilding(rack)).unwrap();
    assert!(run_until(&mut w, 2000, |w| w.settlement.bench_bonus == 40));
    assert_eq!(w.training_report().bench.1, 60);
    w.apply(Command::SetInputReserve(0)).unwrap();
    assert!(run_until(&mut w, 2500, |w| w.bench_stock() > 20));
    w.check_conservation().unwrap();
}

#[test]
fn a_dedicated_hauler_also_carries_the_players_planks_away() {
    let mut w = World::from_scenario(ScenarioKind::SupportedRetraining);
    w.apply(Command::SetTrainingSupply(true)).unwrap();
    let hauler = w
        .spirits
        .iter()
        .find(|s| w.defs.role(s.role).has_job(JobKind::Supply))
        .unwrap()
        .id;
    policy(&mut w, hauler, |p| p.focus = Focus::Training);
    assert!(run_until(&mut w, 800, |w| w.bench_stock() >= 10));
    let bench = w.station().unwrap().pos;
    let planks = res(&w, "planks");
    let before = w.main().storage.get(planks);
    w.apply(Command::PlayerInteract(bench)).unwrap();
    assert!(run_until(&mut w, 1500, |w| w.main().storage.get(planks) > before));
}

// --- Saves ------------------------------------------------------------------

#[test]
fn saves_from_v0_5_load_with_their_progress_intact() {
    let (w, meta) = save::decode(V1_JOURNEY).expect("a v0.5 save loads");
    assert_eq!(meta.version, 1);
    assert_eq!(meta.profile, "normal");
    assert_eq!(w.tick, 5254);
    let skill = wc(&w);
    assert_eq!(w.player.history(skill).xp_sacrificed, 1800);
    assert_eq!(w.spirits.len(), 3);
    assert_eq!(w.spirits[2].xp(), 1800);
    assert!(w.journey.is_some());
    assert!(w.settlement.training.supply);
    w.check_conservation().unwrap();
}

#[test]
fn an_upgraded_save_gains_the_new_content() {
    let (w, _) = save::decode(V1_JOURNEY).unwrap();
    let site = w.defs.site_id("old_growth").unwrap();
    assert!(w.settlement.site_open[site.index()]);
    assert_eq!(w.site_stats.len(), w.defs.sites.len());
    assert!(w.trees.iter().filter(|t| t.site == Some(site)).count() >= 6);
    assert_eq!(w.settlement.extra_orders.len(), 4);
    assert_eq!(w.settlement.extra_orders[0].target, 0);
    let b = w.station().unwrap();
    assert_eq!(
        b.storage.capacity,
        w.defs.building(b.kind).storage_capacity()
    );
}

#[test]
fn an_upgraded_world_keeps_running_and_conserving() {
    for fixture in [V1_JOURNEY, V1_WORKSHOP] {
        let (mut w, _) = save::decode(fixture).unwrap();
        run_checked(&mut w, 2000);
    }
    let (mut w, _) = save::decode(V1_WORKSHOP).unwrap();
    assert!(run_until(&mut w, 3000, |w| {
        w.projects[0].state == ProjectState::Complete
    }));
    assert_eq!(w.main().storage.capacity, 450);
}

#[test]
fn an_upgraded_save_saves_in_the_new_format_and_stays_deterministic() {
    let (w, _) = save::decode(V1_JOURNEY).unwrap();
    let text = save::encode(&w, "new_game");
    assert!(text.starts_with(&format!("SPIRITSCAPE-SAVE {SAVE_VERSION} ")));
    let (mut again, meta) = save::decode(&text).unwrap();
    assert_eq!(meta.version, SAVE_VERSION);
    assert_eq!(again.state_text(), w.state_text());
    let mut w = w;
    w.run_ticks(1000);
    again.run_ticks(1000);
    assert_eq!(again.state_text(), w.state_text());
}

#[test]
fn upgrading_keeps_the_original_file() {
    let dir = std::env::temp_dir().join(format!("spiritscape_v1_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("world.save"), V1_JOURNEY).unwrap();
    let store = SaveStore::new(&dir);
    let loaded = store.load(Slot::Manual).unwrap();
    assert!(loaded.warning.unwrap().contains("world.save.v1"));
    store
        .write(Slot::Manual, &loaded.world, "new_game")
        .unwrap();
    let kept = std::fs::read_to_string(dir.join("world.save.v1")).unwrap();
    assert_eq!(kept, V1_JOURNEY);
    assert!(store.load(Slot::Manual).unwrap().warning.is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn new_state_survives_saving() {
    let mut w = two_workshops();
    let g = w.gatherer_ids()[1];
    let forest = w.defs.site_id("old_growth").unwrap();
    policy(&mut w, g, |p| {
        p.site = Some(forest);
        p.prefer_advanced = true;
        p.focus = Focus::Settlement;
    });
    let beams = res(&w, "beams");
    w.apply(Command::SetOrderTarget {
        output: beams,
        target: 33,
    })
    .unwrap();
    w.apply(Command::SetOrderPriority {
        output: beams,
        priority: 4,
    })
    .unwrap();
    w.apply(Command::SetKeepBenchPlace(true)).unwrap();
    w.run_ticks(217);
    let mut l = save::decode(&save::encode(&w, "x")).unwrap().0;
    assert_eq!(l.spirits[g].policy, w.spirits[g].policy);
    assert_eq!(l.order_for(beams), w.order_for(beams));
    assert_eq!(l.state_text(), w.state_text());
    l.run_ticks(800);
    w.run_ticks(800);
    assert_eq!(l.state_text(), w.state_text(), "deterministic continuation");
}

#[test]
fn every_new_scenario_replays_identically() {
    for kind in [
        ScenarioKind::AdvancedSpecialization,
        ScenarioKind::ProductionBottleneck,
        ScenarioKind::ExpandedProduction,
        ScenarioKind::SupportedRetraining,
    ] {
        let mut a = World::from_scenario(kind);
        let mut b = World::from_scenario(kind);
        run_checked(&mut a, 1200);
        b.run_ticks(1200);
        assert_eq!(a.state_hash(), b.state_hash(), "{}", kind.key());
    }
}

#[test]
fn woodworking_gain_is_personal_at_the_top_tier_too() {
    let mut w = World::from_scenario(ScenarioKind::ExpandedProduction);
    w.run_ticks(2500);
    assert!(made(&w, "beams") > 0 || made(&w, "planks") > 0);
    assert_eq!(w.ledger.xp_earned, 0);
    assert_eq!(w.player.xp(ww(&w)), 0);
}
