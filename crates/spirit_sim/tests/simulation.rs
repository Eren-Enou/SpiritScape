//! End-to-end simulation tests: XP transfer, resource conservation, job
//! transitions, construction requirements and the autonomous project lifecycle.

use spirit_sim::defs::{GameDefs, JobKind, BUILTIN_DEFS};
use spirit_sim::entities::{Activity, ProjectState, TreeState};
use spirit_sim::events::EventKind;
use spirit_sim::grid::{MapLayout, BUILTIN_MAP};
use spirit_sim::scenario::{load_scenarios, ScenarioDef, BUILTIN_SCENARIOS};
use spirit_sim::{Command, ScenarioKind, World};

/// Builds a world from the built-in data with optional text/struct patches.
fn custom_world(kind: ScenarioKind, patch_defs: impl Fn(String) -> String, patch: impl Fn(&mut ScenarioDef)) -> World {
    let defs = GameDefs::from_toml(&patch_defs(BUILTIN_DEFS.to_string())).unwrap();
    let mut scenarios = load_scenarios(BUILTIN_SCENARIOS).unwrap();
    let scenario = scenarios.get_mut(kind.key()).unwrap();
    patch(scenario);
    World::build(defs, &MapLayout::parse(BUILTIN_MAP).unwrap(), scenario).unwrap()
}

fn woodcutting(w: &World) -> spirit_sim::defs::SkillId {
    w.defs.skill_id("woodcutting").unwrap()
}

fn logs(w: &World) -> spirit_sim::defs::ResourceId {
    w.defs.resource_id("logs").unwrap()
}

fn run_until(w: &mut World, max_ticks: u64, mut cond: impl FnMut(&World) -> bool) -> bool {
    for _ in 0..max_ticks {
        if cond(w) {
            return true;
        }
        w.step();
    }
    cond(w)
}

fn spirit_by_role(w: &World, role: &str) -> usize {
    let role = w.defs.role_id(role).unwrap();
    w.spirits.iter().find(|s| s.role == role).unwrap().id
}

// ---------------------------------------------------------------------------
// Progression and Soul Separation
// ---------------------------------------------------------------------------

#[test]
fn xp_table_maps_xp_to_levels() {
    let defs = GameDefs::builtin();
    let t = &defs.xp_table;
    assert_eq!(t.level_for_xp(0), 1);
    assert_eq!(t.level_for_xp(82), 1);
    assert_eq!(t.level_for_xp(83), 2);
    assert_eq!(t.level_for_xp(1154), 10);
    assert_eq!(t.level_for_xp(u64::MAX), t.max_level());
    assert_eq!(t.xp_for_level(10), 1154);
    assert_eq!(t.next_level_xp(100), Some(174));
}

#[test]
fn player_chops_tree_gains_xp_and_tree_depletes_then_regrows() {
    let mut w = World::from_scenario(ScenarioKind::NewGame);
    let skill = woodcutting(&w);
    let tree = w.trees.iter().min_by_key(|t| t.pos.manhattan(w.player.actor.pos())).unwrap().clone();
    let per_tree = w.defs.tree(tree.kind).logs;
    w.apply(Command::PlayerInteract(tree.pos)).unwrap();

    assert!(run_until(&mut w, 2000, |w| !w.trees[tree.id].is_grown()), "tree should deplete");
    assert_eq!(w.player.actor.inventory.get(logs(&w)), per_tree);
    assert_eq!(w.player.xp(skill), per_tree as u64 * w.defs.tree(tree.kind).xp_per_log);
    assert!(w.player.level(skill, &w.defs.xp_table) >= 2);

    let TreeState::Depleted { regrow_at } = w.trees[tree.id].state else { panic!() };
    assert!(w.apply(Command::PlayerInteract(tree.pos)).is_err(), "depleted tree can't be chopped");
    w.run_ticks(regrow_at - w.tick);
    assert_eq!(w.trees[tree.id].logs_left(), per_tree, "tree regrows after the configured interval");
}

#[test]
fn player_deposits_logs_into_main_storage() {
    let mut w = World::from_scenario(ScenarioKind::NewGame);
    let tree = w.trees.iter().min_by_key(|t| t.pos.manhattan(w.player.actor.pos())).unwrap().pos;
    w.apply(Command::PlayerInteract(tree)).unwrap();
    run_until(&mut w, 2000, |w| w.player.actor.queue.is_empty());
    let carried = w.player.actor.inventory.total();
    let main = w.main().pos;
    w.apply(Command::PlayerInteract(main)).unwrap();
    run_until(&mut w, 2000, |w| w.player.actor.queue.is_empty());
    assert_eq!(w.main().storage.get(logs(&w)), carried);
    w.check_conservation().unwrap();
}

#[test]
fn soul_separation_transfers_and_conserves_xp() {
    let mut w = World::from_scenario(ScenarioKind::Demo);
    let skill = woodcutting(&w);
    let before = w.player.xp(skill);
    let spirits_before = w.spirits.len();

    let id = w.soul_separate(skill, 1000).unwrap();
    let s = &w.spirits[id];
    assert_eq!(w.spirits.len(), spirits_before + 1);
    assert_eq!(s.xp(), 1000);
    assert_eq!(s.level(), w.defs.xp_table.level_for_xp(1000));
    assert_eq!(w.player.xp(skill), before - 1000);
    assert_eq!(w.player.xp(skill) + s.xp(), before, "XP is conserved");
    assert_eq!(w.player.level(skill, &w.defs.xp_table), w.defs.xp_table.level_for_xp(before - 1000));
    assert_eq!(w.defs.role(s.role).id, "woodcutter");
    assert_eq!(s.actor.pos(), w.player.actor.pos(), "spirit appears at the player");
    assert_eq!(w.ledger.xp_separated, 1000);
    assert!(w.events.contains(EventKind::Player, "Soul Separation"));
}

#[test]
fn soul_separation_rejects_invalid_requests() {
    let mut w = World::from_scenario(ScenarioKind::Demo);
    let skill = woodcutting(&w);
    let before = w.player.xp(skill);
    let n = w.spirits.len();
    assert!(w.soul_separate(skill, w.defs.soul_min_xp - 1).is_err(), "below minimum");
    assert!(w.soul_separate(skill, before + 1).is_err(), "more than available");
    let hauling = w.defs.skill_id("hauling").unwrap();
    assert!(w.soul_separate(hauling, 100).is_err(), "skill without a soul role");
    assert_eq!(w.player.xp(skill), before);
    assert_eq!(w.spirits.len(), n);
}

#[test]
fn spirits_never_gain_xp_from_work() {
    let mut w = World::from_scenario(ScenarioKind::Demo);
    let skill = woodcutting(&w);
    let xp_before: Vec<u64> = w.spirits.iter().map(|s| s.xp()).collect();
    let player_before = w.player.xp(skill);
    w.run_ticks(4000);
    let wc = spirit_by_role(&w, "woodcutter");
    assert!(w.spirits[wc].units_gathered > 0, "the woodcutter actually worked");
    let xp_after: Vec<u64> = w.spirits.iter().map(|s| s.xp()).collect();
    assert_eq!(xp_before, xp_after);
    assert_eq!(w.player.xp(skill), player_before, "spirit work does not credit the player");
}

#[test]
fn created_spirit_gathers_without_player_control() {
    let mut w = World::from_scenario(ScenarioKind::NewGame);
    let skill = woodcutting(&w);
    w.player.actor.queue.clear();
    // Earn XP the normal way: chop two grove trees.
    for _ in 0..2 {
        let p = w.player.actor.pos();
        let Some(tree) = w.trees.iter().filter(|t| t.is_grown()).min_by_key(|t| t.pos.manhattan(p)).map(|t| t.pos) else {
            break;
        };
        w.apply(Command::PlayerInteract(tree)).unwrap();
        run_until(&mut w, 3000, |w| w.player.actor.queue.is_empty());
    }
    let xp = w.player.xp(skill);
    assert!(xp >= 200, "player earned {xp} XP");
    let id = w.soul_separate(skill, xp).unwrap();
    assert!(run_until(&mut w, 3000, |w| w.main().storage.get(logs(w)) > 0), "spirit delivered logs on its own");
    assert!(w.spirits[id].units_delivered > 0);
    w.check_conservation().unwrap();
}

// ---------------------------------------------------------------------------
// Automation
// ---------------------------------------------------------------------------

#[test]
fn woodcutter_switches_jobs_with_world_conditions() {
    let mut w = custom_world(ScenarioKind::Demo, |d| d, |s| s.objective_target = 40);
    let wc = spirit_by_role(&w, "woodcutter");
    let mut seen = Vec::new();
    let done = run_until(&mut w, 6000, |w| {
        let job = w.spirits[wc].job;
        if seen.last() != Some(&job) {
            seen.push(job);
        }
        w.tick > 0 && matches!(w.spirits[wc].activity, Activity::Idle(_)) && w.spirits[wc].actor.inventory.total() == 0
    });
    assert!(done, "woodcutter should go idle once the objective is met");
    assert!(seen.contains(&Some(JobKind::Gather)));
    assert!(seen.contains(&Some(JobKind::Deliver)));
    assert!(w.main().storage.get(logs(&w)) >= 40);
    assert!(w.spirits[wc].activity.text().contains("objective satisfied"));

    // Raising the objective makes the condition false again: work resumes.
    w.apply(Command::SetObjectiveTarget(60)).unwrap();
    assert!(run_until(&mut w, 50, |w| w.spirits[wc].job == Some(JobKind::Gather)));
}

#[test]
fn woodcutter_is_blocked_when_no_trees_are_grown() {
    let mut w = World::from_scenario(ScenarioKind::Demo);
    for t in &mut w.trees {
        t.state = TreeState::Depleted { regrow_at: 1_000_000 };
    }
    w.run_ticks(20);
    let wc = spirit_by_role(&w, "woodcutter");
    match &w.spirits[wc].activity {
        Activity::Blocked(reason) => assert!(reason.contains("regrowth"), "{reason}"),
        other => panic!("expected blocked, got {other:?}"),
    }
}

#[test]
fn inventory_capacity_limits_each_trip() {
    let mut w = World::from_scenario(ScenarioKind::Demo);
    let wc = spirit_by_role(&w, "woodcutter");
    let cap = w.spirits[wc].actor.inventory.capacity;
    for _ in 0..3000 {
        w.step();
        assert!(w.spirits[wc].actor.inventory.total() <= cap);
    }
}

// ---------------------------------------------------------------------------
// Resource conservation and determinism
// ---------------------------------------------------------------------------

#[test]
fn resources_are_conserved_through_the_full_cycle() {
    let mut w = World::from_scenario(ScenarioKind::Demo);
    for _ in 0..9000 {
        w.step();
        w.check_conservation().unwrap_or_else(|e| panic!("tick {}: {e}", w.tick));
    }
    let r = logs(&w);
    assert_eq!(w.projects.len(), 1);
    assert_eq!(w.ledger.consumed.get(&r).copied(), Some(15), "construction consumed exactly its cost");
}

#[test]
fn simulation_is_deterministic() {
    let run = || {
        let mut w = World::from_scenario(ScenarioKind::Demo);
        let skill = woodcutting(&w);
        let mut hashes = Vec::new();
        for i in 0..6000 {
            if i == 100 {
                w.apply(Command::SoulSeparate { skill, xp: 500 }).unwrap();
            }
            w.step();
            if i % 500 == 0 {
                hashes.push(w.state_hash());
            }
        }
        (hashes, w.events.iter().map(|e| e.text.clone()).collect::<Vec<_>>())
    };
    assert_eq!(run(), run());
}

// ---------------------------------------------------------------------------
// Settlement planning
// ---------------------------------------------------------------------------

#[test]
fn autonomous_field_storage_lifecycle() {
    let mut w = World::from_scenario(ScenarioKind::Demo);
    let r = logs(&w);
    let main_before_project;

    // 1-4: problem detected, project created by the planner (no player input).
    assert!(run_until(&mut w, 4000, |w| !w.projects.is_empty()), "planner should start a project");
    assert!(w.events.contains(EventKind::Planner, "Inefficiency detected"));
    assert!(w.events.contains(EventKind::Planner, "Decision: build a Field Storage"));
    let p = &w.projects[0];
    assert!(p.reason.contains("walking takes"));
    let site = p.site;
    let builder = spirit_by_role(&w, "builder");
    assert_eq!(p.assigned, Some(builder));
    main_before_project = w.main().storage.get(r);
    assert_eq!(w.main().storage.reserved(r), 15, "materials reserved");

    // 5-7: builder physically carries reserved logs and builds.
    let mut states = vec![w.projects[0].state];
    assert!(run_until(&mut w, 3000, |w| {
        let s = w.projects[0].state;
        if states.last() != Some(&s) {
            states.push(s);
        }
        s == ProjectState::Complete
    }));
    assert_eq!(states.last(), Some(&ProjectState::Complete));
    assert!(states.contains(&ProjectState::Building));
    assert_eq!(w.projects[0].consumed.get(&r), Some(&15));
    assert_eq!(w.main().storage.reserved(r), 0);
    let field = w.building_at(site).unwrap();
    assert!(field.is_complete());
    assert!(w.main().storage.get(r) + 15 >= main_before_project, "logs left main storage for the site");

    // 8: workers adapt - woodcutter delivers to the field storage, hauler moves it.
    let field_id = field.id;
    let wc = spirit_by_role(&w, "woodcutter");
    let hauler = spirit_by_role(&w, "hauler");
    assert!(run_until(&mut w, 3000, |w| w.spirits[wc].last_deliver_target == Some(field_id)));
    assert!(run_until(&mut w, 3000, |w| w.spirits[hauler].units_delivered > 0));
    assert!(w.events.contains(EventKind::Spirit, "now delivers to Field Storage"));

    // No duplicate or endless construction.
    w.run_ticks(20_000);
    assert_eq!(w.projects.len(), 1);
    assert_eq!(w.buildings.len(), 2);
    w.check_conservation().unwrap();
}

#[test]
fn no_construction_without_a_real_logistics_problem() {
    // Short routes only: a low objective is satisfied from the grove next to town.
    let mut w = custom_world(ScenarioKind::Demo, |d| d, |s| s.objective_target = 30);
    w.run_ticks(6000);
    assert!(w.projects.is_empty());

    // No woodcutter at all: nothing to measure, nothing to build, however long we wait.
    let mut w = World::from_scenario(ScenarioKind::NewGame);
    w.run_ticks(10_000);
    assert!(w.projects.is_empty());
    assert!(w.settlement.planner.status.contains("Monitoring"));
}

#[test]
fn construction_requires_a_builder() {
    let mut w = custom_world(ScenarioKind::Demo, |d| d, |s| s.spirits.retain(|sp| sp.role != "builder"));
    w.run_ticks(4000);
    assert!(w.projects.is_empty());
    assert!(w.events.contains(EventKind::Planner, "no Construction Spirit"));
}

#[test]
fn construction_requires_a_hauler() {
    let mut w = custom_world(ScenarioKind::Demo, |d| d, |s| s.spirits.retain(|sp| sp.role != "hauler"));
    w.run_ticks(4000);
    assert!(w.projects.is_empty());
    assert!(w.events.contains(EventKind::Planner, "no Hauling Spirit"));
}

#[test]
fn construction_requires_permission() {
    let mut w = World::from_scenario(ScenarioKind::Demo);
    w.apply(Command::SetAutonomousConstruction(false)).unwrap();
    assert!(run_until(&mut w, 4000, |w| w.settlement.planner.status.contains("not permitted")));
    w.run_ticks(300);
    assert!(w.projects.is_empty(), "no project while forbidden");
    assert!(w.events.contains(EventKind::Planner, "not permitted"));

    w.apply(Command::SetAutonomousConstruction(true)).unwrap();
    let interval = w.defs.planner.eval_interval_ticks;
    assert!(run_until(&mut w, interval + 1, |w| !w.projects.is_empty()));
}

#[test]
fn project_waits_for_materials_with_readable_reason() {
    let mut w = custom_world(
        ScenarioKind::Demo,
        |d| d.replace(r#"cost = [{ resource = "logs", amount = 15 }]"#, r#"cost = [{ resource = "logs", amount = 500 }]"#),
        |_| {},
    );
    assert!(run_until(&mut w, 4000, |w| !w.projects.is_empty()));
    w.run_ticks(10);
    let p = &w.projects[0];
    assert_eq!(p.state, ProjectState::AwaitingMaterials);
    let reason = p.blocked.clone().unwrap();
    assert!(reason.contains("Waiting for materials"), "{reason}");
    let builder = spirit_by_role(&w, "builder");
    assert!(matches!(w.spirits[builder].activity, Activity::Blocked(_)));
    assert_eq!(w.main().storage.reserved(logs(&w)), 0, "nothing reserved until the full cost is available");
}

#[test]
fn rejected_plans_are_explained_and_not_retried_immediately() {
    let mut w = custom_world(
        ScenarioKind::Demo,
        |d| d.replace("min_savings_pct = 50", "min_savings_pct = 101"),
        |_| {},
    );
    w.run_ticks(6000);
    assert!(w.projects.is_empty());
    assert!(w.events.contains(EventKind::Planner, "only shortens deliveries"));
    let rejections = w.events.iter().filter(|e| e.text.contains("rejected it")).count();
    assert_eq!(rejections, 1, "same rejection is logged once");
}
