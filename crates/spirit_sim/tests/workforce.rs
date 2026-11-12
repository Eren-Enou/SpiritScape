//! Workforce regression tests and reproducible comparisons (run with --nocapture).
use spirit_sim::defs::{GameDefs, ResourceId, SkillId};
use spirit_sim::entities::{BuildingId, ProjectId, ProjectState};
use spirit_sim::grid::{BUILTIN_MAP, MapLayout, TilePos};
use spirit_sim::save::{decode, encode};
use spirit_sim::scenario::{BUILTIN_SCENARIOS, ScenarioKind, load_scenarios};
use spirit_sim::tasks::Task;
use spirit_sim::world::{Command, World};

const LIMIT: u64 = 15_000;

fn pioneer() -> World {
    World::from_scenario(ScenarioKind::PioneerWorkforce)
}

fn scenario(key: &str, profile: &str, specialist_xp: u64) -> World {
    let mut scenarios = load_scenarios(BUILTIN_SCENARIOS).unwrap();
    let start = scenarios.get_mut(key).unwrap();
    if specialist_xp > 0 {
        for discipline in ["hauling", "construction"] {
            start.player_xp.insert(discipline.into(), specialist_xp);
        }
    }
    World::build(
        GameDefs::builtin_profile(profile).unwrap(),
        &MapLayout::parse(BUILTIN_MAP).unwrap(),
        start,
    )
    .unwrap()
}

fn simulation_state(w: &World) -> serde_json::Value {
    let text = encode(w, "pioneer_workforce");
    let (_, json) = text.split_once('\n').unwrap();
    let mut value: serde_json::Value = serde_json::from_str(json).unwrap();
    value["state"].take()
}

fn skill(w: &World, id: &str) -> SkillId {
    w.defs.skill_id(id).unwrap()
}

fn logs(w: &World) -> ResourceId {
    w.defs.resource_id("logs").unwrap()
}

fn wait(w: &mut World, ready: impl Fn(&World) -> bool) {
    for _ in 0..LIMIT {
        if ready(w) {
            w.check_conservation().unwrap();
            return;
        }
        w.step();
    }
    panic!("timed out at {}: {}", w.tick, w.player.activity);
}

fn idle(w: &mut World) {
    wait(w, |w| w.player.actor.queue.is_empty());
}

fn interact(w: &mut World, at: TilePos) {
    w.apply(Command::PlayerInteract(at)).unwrap();
    idle(w);
}

fn gather(w: &mut World, target: u32) -> u32 {
    let resource = logs(w);
    let main = w.main().pos;
    let mut actions = 0;
    while w.main().storage.available(resource) < target {
        let tree = w
            .trees
            .iter()
            .filter(|t| t.is_grown() && w.defs.tree(t.kind).resource == resource)
            .filter(|t| w.player_can_cut(t.kind))
            .min_by_key(|t| t.pos.manhattan(main))
            .expect("reachable trees near the settlement")
            .pos;
        interact(w, tree);
        assert!(w.player.actor.inventory.get(resource) > 0);
        interact(w, main);
        actions += 2;
        assert!(actions < 50);
    }
    actions
}

fn plan(w: &mut World, kind: &str) -> ProjectId {
    let kind = w.defs.building_kind_id(kind).unwrap();
    if w.defs.building(kind).training_cache {
        let area = w.defs.site_id("starting_grove").unwrap();
        w.apply(Command::SetTrainingArea(Some(area))).unwrap();
    }
    w.apply(Command::RequestBuilding(kind)).unwrap();
    w.step();
    w.active_project().unwrap().id
}

fn supply(w: &mut World, id: ProjectId) -> u32 {
    let mut actions = 0;
    while !w.project_materials_ready(id) {
        w.apply(Command::SupplyProject(id)).unwrap();
        idle(w);
        actions += 1;
        assert!(actions < 30);
    }
    actions
}

fn seed(w: &mut World, name: &str, amount: u32) {
    let resource = w.defs.resource_id(name).unwrap();
    let main = w.main().id;
    assert_eq!(w.buildings[main].storage.deposit(resource, amount), amount);
    *w.ledger.seeded.entry(resource).or_default() += u64::from(amount);
}

fn gift(w: &mut World, name: &str) -> usize {
    let skill = skill(w, name);
    let xp = w.player.xp(skill);
    assert!(w.soul_preview(skill, xp).error.is_none());
    let sid = w.soul_separate(skill, xp).unwrap();
    assert_eq!(w.player.xp(skill), 0);
    assert_eq!(w.spirits[sid].gift_xp, xp);
    assert_eq!(w.spirits[sid].skill(), skill);
    sid
}

#[test]
fn a_station_pays_only_for_accepted_raw_materials() {
    let mut w = pioneer();
    let hauling = skill(&w, "hauling");
    let bench = w.station().unwrap().id;
    seed(&mut w, "logs", 28);
    let main = w.main().pos;
    interact(&mut w, main);
    interact(&mut w, main);
    assert_eq!(w.player.xp(hauling), 0);
    let resource = logs(&w);
    while w.station_delivery(bench).is_ok() {
        w.apply(Command::SupplyStation(bench)).unwrap();
        idle(&mut w);
    }
    let given = w.buildings[bench].storage.get(resource);
    assert_eq!(given, 20);
    assert_eq!(
        w.player.xp(hauling),
        u64::from(given) * w.defs.workforce.haul_xp_per_unit
    );
    assert!(w.apply(Command::SupplyStation(bench)).is_err());
    w.run_ticks(100);
    assert_eq!(w.player.xp(hauling), 160);
    w.check_conservation().unwrap();
}

#[test]
fn delivery_and_partial_build_replay_across_saves() {
    let mut w = pioneer();
    seed(&mut w, "logs", 15);
    let id = plan(&mut w, "training_cache");
    let hauling = skill(&w, "hauling");
    let construction = skill(&w, "construction");
    assert!(w.apply(Command::HelpBuild(id)).is_err());
    assert_eq!(w.player.xp(construction), 0);
    w.apply(Command::SupplyProject(id)).unwrap();
    wait(&mut w, |w| w.player.actor.inventory.total() > 0);
    let text = encode(&w, "pioneer_workforce");
    let (mut loaded, meta) = decode(&text).unwrap();
    assert_eq!(meta.version, 9);
    idle(&mut w);
    idle(&mut loaded);
    assert_eq!(simulation_state(&w), simulation_state(&loaded));
    assert_eq!(w.player.xp(hauling), 48);
    supply(&mut w, id);
    w.apply(Command::HelpBuild(id)).unwrap();
    wait(&mut w, |w| w.projects[id].build_progress >= 12);
    w.apply(Command::PlayerStop).unwrap();
    let xp = w.player.xp(construction);
    let progress = w.projects[id].build_progress;
    w.run_ticks(50);
    assert_eq!(w.player.xp(construction), xp);
    assert_eq!(w.projects[id].build_progress, progress);
    assert!(w.apply(Command::CancelProject(id)).is_err());
    w.apply(Command::HelpBuild(id)).unwrap();
    wait(&mut w, |w| w.projects[id].build_progress > progress);
    let (mut loaded, _) = decode(&encode(&w, "pioneer_workforce")).unwrap();
    idle(&mut w);
    idle(&mut loaded);
    assert_eq!(simulation_state(&w), simulation_state(&loaded));
    assert_eq!(w.projects[id].state, ProjectState::Complete);
    assert_eq!(w.projects[id].consumed[&logs(&w)], 15);
    assert!(w.apply(Command::HelpBuild(id)).is_err());
}

#[test]
fn cancelled_pickup_releases_only_the_project_hold_and_keeps_the_players_cargo() {
    let mut w = pioneer();
    seed(&mut w, "logs", 20);
    let id = plan(&mut w, "training_cache");
    let resource = logs(&w);
    let hauling = skill(&w, "hauling");
    w.apply(Command::SupplyProject(id)).unwrap();
    wait(&mut w, |w| w.player.actor.inventory.total() > 0);
    let cargo = w.player.actor.inventory.get(resource);
    assert_eq!(w.main().storage.reserved(resource), 15 - cargo);
    w.apply(Command::CancelProject(id)).unwrap();
    idle(&mut w);
    assert_eq!(w.main().storage.reserved(resource), 0);
    assert_eq!(w.player.actor.inventory.get(resource), cargo);
    assert_eq!(w.player.xp(hauling), 0);
    assert!(w.projects[id].delivered.is_empty());
    w.check_conservation().unwrap();
}

#[test]
fn interrupted_pickup_can_be_returned_and_reclaimed_without_duplicating_xp() {
    let mut w = pioneer();
    seed(&mut w, "logs", 15);
    let id = plan(&mut w, "training_cache");
    w.apply(Command::SupplyProject(id)).unwrap();
    wait(&mut w, |w| w.player.actor.inventory.total() > 0);
    w.apply(Command::PlayerStop).unwrap();
    let main = w.main().pos;
    interact(&mut w, main);
    assert_eq!(w.player.xp(skill(&w, "hauling")), 0);
    supply(&mut w, id);
    assert_eq!(w.player.xp(skill(&w, "hauling")), 120);
    assert_eq!(w.projects[id].delivered[&logs(&w)], 15);
}

#[test]
fn founding_workforce_control_and_player_assistance() {
    for manual in [false, true] {
        let mut w = World::new_journey("normal").unwrap();
        let hauling = skill(&w, "hauling");
        let construction = skill(&w, "construction");
        assert_eq!(w.spirits.len(), 2);
        assert!(!w.is_open("hauling") && !w.is_open("construction"));
        let gathering_inputs = gather(&mut w, 15);
        let id = plan(&mut w, "training_cache");
        let mut help_inputs = 0;
        if manual {
            w.apply(Command::SupplyProject(id)).unwrap();
            idle(&mut w);
            help_inputs += 1;
        }
        wait(&mut w, |w| w.project_materials_ready(id));
        if manual {
            w.apply(Command::HelpBuild(id)).unwrap();
            help_inputs += 1;
            wait(&mut w, |w| {
                matches!(w.player.actor.queue.front(), Some(Task::Build { .. }))
                    && w.spirits
                        .iter()
                        .any(|s| matches!(s.actor.queue.front(), Some(Task::Build { .. })))
            });
        }
        wait(&mut w, |w| !w.projects[id].is_active());
        let p = &w.projects[id];
        assert_eq!(p.state, ProjectState::Complete);
        assert_eq!(p.consumed[&logs(&w)], 15);
        let (haul, build) = (w.player.xp(hauling), w.player.xp(construction));
        assert_eq!(haul > 0, manual);
        assert_eq!(build > 0, manual);
        println!(
            "founding manual={manual} finish_tick={} project_ticks={} haul_xp={haul} build_xp={build} gather_actions={gathering_inputs} help_actions={help_inputs}",
            w.tick,
            p.completed_tick.unwrap() - p.created_tick
        );
    }
}

#[test]
fn pioneer_earns_a_workforce_and_uses_it_to_extend_the_settlement() {
    let mut w = pioneer();
    assert!(w.spirits.is_empty());
    assert_eq!(w.ledger.xp_earned, 0);
    assert_eq!(w.main().storage.total(), 0);
    let bench = w.station().unwrap().id;
    let mut inputs = gather(&mut w, 18);
    for _ in 0..3 {
        w.apply(Command::SupplyStation(bench)).unwrap();
        idle(&mut w);
        inputs += 1;
    }
    let hauler = gift(&mut w, "hauling");
    let haul_tick = w.tick;
    let first_haul_xp = w.spirits[hauler].gift_xp;
    inputs += gather(&mut w, 15);
    let cache = plan(&mut w, "training_cache");
    inputs += supply(&mut w, cache);
    w.apply(Command::HelpBuild(cache)).unwrap();
    idle(&mut w);
    inputs += 1;
    let builder = gift(&mut w, "construction");
    let build_tick = w.tick;
    let first_build_xp = w.spirits[builder].gift_xp;
    assert_eq!(w.projects[cache].state, ProjectState::Complete);
    let mut project_times = Vec::new();
    for (kind, plank_goal) in [("log_rack", 10), ("storehouse", 20)] {
        inputs += craft_planks(&mut w, bench, plank_goal);
        inputs += gather(&mut w, 20);
        let id = plan(&mut w, kind);
        wait(&mut w, |w| !w.projects[id].is_active());
        let p = &w.projects[id];
        assert_eq!(p.assigned, Some(builder));
        assert_eq!(p.state, ProjectState::Complete);
        project_times.push(p.completed_tick.unwrap() - p.created_tick);
    }
    assert_eq!(w.player.xp(skill(&w, "construction")), 0);
    assert_eq!(w.spirits[hauler].gift_xp, first_haul_xp);
    assert_eq!(w.spirits[builder].gift_xp, first_build_xp);
    let project_finish = w.tick;
    let time = w.spirits[hauler].time.total;
    assert!(time.traveling > 0 && time.handling > 0);
    println!(
        "pioneer haul_tick={haul_tick} build_tick={build_tick} first_auto_finish={} last_finish={project_finish} project_ticks={project_times:?} earned={} sacrificed={} manual_actions={inputs} hauler_time={time:?} consumed={:?}",
        w.projects[1].completed_tick.unwrap(),
        w.ledger.xp_earned,
        w.ledger.xp_separated,
        w.ledger.consumed
    );

    gift(&mut w, "woodcutting");
    gift(&mut w, "woodworking");
    w.apply(Command::SetTrainingSupply(false)).unwrap();
    w.apply(Command::SetProductionEnabled(true)).unwrap();
    w.apply(Command::SetProductionTarget(40)).unwrap();
    let (mut loaded, _) = decode(&encode(&w, "pioneer_workforce")).unwrap();
    let planks = loaded.defs.resource_id("planks").unwrap();
    let before = loaded.ledger.crafted[&planks];
    wait(&mut loaded, |w| w.main().storage.available(planks) >= 40);
    let produced = loaded.ledger.crafted[&planks] - before;
    println!(
        "pioneer autonomous stock reached in {} ticks with {produced} planks crafted",
        loaded.tick - project_finish,
    );
    assert!(produced > 0);
    loaded.run_ticks(18_000);
    loaded.check_conservation().unwrap();
    assert!(loaded.main().storage.available(planks) >= 40);
    assert_eq!(loaded.spirits[builder].level(), w.spirits[builder].level());
}

fn craft_planks(w: &mut World, bench: BuildingId, target: u32) -> u32 {
    let planks = w.defs.resource_id("planks").unwrap();
    let mut inputs = 0;
    w.apply(Command::SetTrainingSupply(true)).unwrap();
    while w.main().storage.available(planks) < target {
        inputs += gather(w, 24);
        let at = w.buildings[bench].pos;
        let before = w.ledger.crafted.get(&planks).copied().unwrap_or(0);
        w.apply(Command::PlayerInteract(at)).unwrap();
        wait(w, |w| {
            w.ledger.crafted.get(&planks).copied().unwrap_or(0) >= before + 4
                || w.player.actor.queue.is_empty()
        });
        w.apply(Command::PlayerStop).unwrap();
        idle(w);
        if w.player.actor.inventory.total() > 0 {
            interact(w, w.main().pos);
            inputs += 1;
        }
        w.run_ticks(140);
        inputs += 1;
        assert!(inputs < 60, "failed to make planks");
    }
    inputs
}

#[test]
fn the_normal_pace_can_earn_both_separations() {
    let mut w = scenario("pioneer_workforce", "normal", 0);
    let bench = w.station().unwrap().id;
    for _ in 0..2 {
        gather(&mut w, 20);
        while w.station_delivery(bench).is_ok() {
            w.apply(Command::SupplyStation(bench)).unwrap();
            idle(&mut w);
        }
        if w.player.xp(skill(&w, "hauling")) < w.defs.soul_min_xp {
            let at = w.buildings[bench].pos;
            let main = w.main().pos;
            interact(&mut w, at);
            if w.player.actor.inventory.total() > 0 {
                interact(&mut w, main);
            }
        }
    }
    gift(&mut w, "hauling");
    gather(&mut w, 15);
    let id = plan(&mut w, "training_cache");
    supply(&mut w, id);
    w.apply(Command::HelpBuild(id)).unwrap();
    idle(&mut w);
    gift(&mut w, "construction");
}

#[test]
fn higher_fixed_competency_improves_carrying_and_construction() {
    let mut observations = Vec::new();
    for level in [4, 10] {
        let xp = GameDefs::builtin().xp_table.xp_for_level(level);
        let mut w = scenario("pioneer_workforce", "development", xp);
        for discipline in ["hauling", "construction"] {
            gift(&mut w, discipline);
        }
        let capacity = w.spirits[0].actor.inventory.capacity;
        seed(&mut w, "logs", 15);
        let id = plan(&mut w, "training_cache");
        wait(&mut w, |w| w.projects[id].state == ProjectState::Building);
        let start = w.tick;
        wait(&mut w, |w| w.projects[id].state == ProjectState::Complete);
        assert_eq!(w.spirits[1].level(), level);
        let build_ticks = w.tick - start;
        let cache = w.training_cache().unwrap().id;
        let resource = logs(&w);
        let cargo = 24;
        assert_eq!(w.buildings[cache].storage.deposit(resource, cargo), cargo);
        *w.ledger.seeded.entry(resource).or_default() += u64::from(cargo);
        w.apply(Command::SetTrainingCollect(true)).unwrap();
        let start = w.tick;
        wait(&mut w, |w| w.main().storage.get(resource) == cargo);
        observations.push((capacity, build_ticks, w.tick - start));
    }
    assert!(observations[1].0 > observations[0].0);
    assert!(observations[1].1 < observations[0].1);
    assert!(observations[1].2 < observations[0].2);
    println!(
        "specialization (hauler load, builder work ticks, ticks to haul 24) level 4, 10: {observations:?}"
    );
}

#[test]
fn a_version_nine_project_with_a_queued_legacy_pickup_still_finishes() {
    let mut w = World::new_journey("development").unwrap();
    seed(&mut w, "logs", 15);
    let id = plan(&mut w, "training_cache");
    let builder = w.projects[id].assigned.unwrap();
    wait(&mut w, |w| {
        w.spirits[builder]
            .actor
            .queue
            .iter()
            .any(|t| matches!(t, Task::WithdrawForProject { project, .. } if *project == id))
    });
    let main = w.main().id;
    for task in &mut w.spirits[builder].actor.queue {
        if let Task::WithdrawForProject {
            resource, amount, ..
        } = *task
        {
            *task = Task::Withdraw {
                building: main,
                resource,
                amount,
                reserved: true,
            };
        }
    }
    let text = encode(&w, "new_game");
    let (_, body) = text.split_once('\n').unwrap();
    let mut json: serde_json::Value = serde_json::from_str(body).unwrap();
    let fields = json["state"]["projects"][id].as_object_mut().unwrap();
    for added in ["held_at_main", "build_fraction", "min_worker_level"] {
        fields.remove(added);
    }
    let save = spirit_sim::save::seal(9, &json.to_string());
    let (mut loaded, _) = decode(&save).unwrap();
    wait(&mut loaded, |w| {
        w.projects[id].state == ProjectState::Complete
    });
    assert_eq!(loaded.spirits.len(), 2);
    assert_eq!(loaded.projects[id].consumed[&logs(&loaded)], 15);
    assert_eq!(loaded.main().storage.reserved(logs(&loaded)), 0);
}

#[test]
fn advanced_building_checks_skill_before_starting_work() {
    let mut scenarios = load_scenarios(BUILTIN_SCENARIOS).unwrap();
    let start = scenarios.get_mut("midwinter").unwrap();
    start
        .prebuilt
        .retain(|seed| seed.building != "hearth_of_spirits");
    start.spirits.clear();
    start.hearth_lit = false;
    start.player_xp.clear();
    let mut w = World::build(
        GameDefs::builtin(),
        &MapLayout::parse(BUILTIN_MAP).unwrap(),
        start,
    )
    .unwrap();
    for (material, count) in [("keystones", 12), ("panels", 20), ("dressed_stone", 20)] {
        seed(&mut w, material, count);
    }
    let id = plan(&mut w, "hearth_of_spirits");
    supply(&mut w, id);
    assert!(
        w.apply(Command::HelpBuild(id))
            .unwrap_err()
            .contains("level 3")
    );
    assert_eq!(w.player.xp(skill(&w, "construction")), 0);
}

#[test]
fn the_still_boat_returns_the_new_disciplines_without_earning_xp_again() {
    let mut w = scenario("last_boat", "development", 388);
    for discipline in ["hauling", "construction"] {
        gift(&mut w, discipline);
    }
    let earned = w.ledger.xp_earned;
    w.apply(Command::CallHome).unwrap();
    wait(&mut w, World::homecoming_finished);
    for discipline in ["hauling", "construction"] {
        assert_eq!(w.player.xp(skill(&w, discipline)), 388);
    }
    assert_eq!(w.ledger.xp_earned, earned);
}
