//! v0.7: continuous gathering, recipe choice, supported training, feedback,
//! play measurements, and that all of it saves and replays exactly.

use spirit_sim::autoplay::{play_full_session, storage_wall};
use spirit_sim::defs::{RecipeId, SiteId, TreeKindId};
use spirit_sim::entities::{Focus, TreeState};
use spirit_sim::grid::TilePos;
use spirit_sim::save::{SAVE_VERSION, SaveStore, Slot, decode, encode};
use spirit_sim::session::{ActivityState, CraftBlock, SessionEnd, SessionKind};
use spirit_sim::tasks::Task;
use spirit_sim::training::TrainingMode;
use spirit_sim::{Command, ScenarioKind, World};

fn skilling() -> World {
    World::from_scenario(ScenarioKind::SkillingSession)
}

fn recipes() -> World {
    World::from_scenario(ScenarioKind::WorkshopRecipes)
}

fn kind(w: &World, id: &str) -> TreeKindId {
    TreeKindId(w.defs.trees.iter().position(|t| t.id == id).unwrap() as u16)
}

fn recipe(w: &World, id: &str) -> RecipeId {
    RecipeId(w.defs.recipes.iter().position(|r| r.id == id).unwrap() as u16)
}

fn grove(w: &World) -> SiteId {
    w.defs.site_id("starting_grove").unwrap()
}

/// A grown tree of this kind in the grove.
fn tree_pos(w: &World, id: &str) -> TilePos {
    let k = kind(w, id);
    w.trees
        .iter()
        .find(|t| t.kind == k && t.site == Some(grove(w)) && t.is_grown())
        .unwrap()
        .pos
}

fn click(w: &mut World, p: TilePos) {
    w.apply(Command::PlayerInteract(p)).unwrap();
}

fn click_tree(w: &mut World, id: &str) {
    let p = tree_pos(w, id);
    click(w, p);
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

fn waiting(w: &World) -> bool {
    let s = w.player.session.as_ref();
    s.is_some_and(|s| s.waiting_since.is_some())
}

fn chopping(w: &World) -> bool {
    matches!(w.player.actor.queue.front(), Some(Task::Chop { .. }))
}

/// Takes the materials the current choice needs, then stands at the bench.
fn fill_bag(w: &mut World) {
    let main = w.main().pos;
    click(w, main);
    run_until(w, 400, |w| w.player.actor.queue.is_empty());
}

fn bench(w: &World) -> TilePos {
    w.station().unwrap().pos
}

fn end_of(w: &World) -> SessionEnd {
    w.player.last_session.as_ref().expect("a session ended").end
}

// ---------------------------------------------------------------------------
// Continuous gathering
// ---------------------------------------------------------------------------

#[test]
fn one_click_chops_until_the_bag_is_full() {
    let mut w = skilling();
    let start = tree_pos(&w, "tree");
    click(&mut w, start);
    assert!(run_until(&mut w, 6000, |w| w.player.session.is_none()));
    let bag = &w.player.actor.inventory;
    assert!(bag.is_full(), "it stopped early");
    assert_eq!(end_of(&w), SessionEnd::BagFull);
    assert_eq!(
        w.player.log.gather_interactions, 1,
        "one click did all that"
    );
    let cut = w.trees.iter().filter(|t| !t.is_grown()).count();
    assert!(cut >= 2, "it moved between trees");
    assert!(w.player.activity.contains("bag is full"));
}

#[test]
fn a_session_earns_exactly_what_clicking_would() {
    let mut w = skilling();
    let wc = w.defs.skill_id("woodcutting").unwrap();
    let before = w.player.xp(wc);
    click_tree(&mut w, "tree");
    run_until(&mut w, 6000, |w| w.player.session.is_none());
    let last = w.player.last_session.clone().unwrap();
    assert_eq!(last.units, w.player.actor.inventory.total());
    assert_eq!(last.xp, w.player.xp(wc) - before);
    assert_eq!(w.ledger.xp_earned, last.xp);
    // Every log paid its own tree's rate, whichever trees the session chose.
    let oak = kind(&w, "oak");
    let oak_logs: u64 = w
        .trees
        .iter()
        .filter(|t| t.kind == oak)
        .map(|t| (w.defs.tree(oak).logs - t.logs_left()) as u64)
        .sum();
    let tree_logs = last.units as u64 - oak_logs;
    let rate = |id: &str| w.defs.tree(kind(&w, id)).xp_per_log;
    assert_eq!(last.xp, tree_logs * rate("tree") + oak_logs * rate("oak"));
}

#[test]
fn it_prefers_the_kind_clicked_and_falls_back_to_the_same_material() {
    let mut w = skilling();
    let oak = kind(&w, "oak");
    click_tree(&mut w, "oak");
    run_until(&mut w, 400, chopping);
    // Every oak gets cut before anything else does.
    let basic = kind(&w, "tree");
    run_until(&mut w, 3000, |w| {
        !w.trees
            .iter()
            .any(|t| t.kind == oak && t.site == Some(grove(w)) && t.is_grown())
            || w.trees.iter().any(|t| t.kind == basic && !t.is_grown())
    });
    assert!(
        w.trees
            .iter()
            .filter(|t| t.kind == basic)
            .all(|t| t.is_grown()),
        "it left Oak for a Tree while Oak was standing"
    );
}

#[test]
fn it_only_targets_trees_the_player_may_cut() {
    let mut w = World::from_scenario(ScenarioKind::SkillingSession);
    let wc = w.defs.skill_id("woodcutting").unwrap();
    // Below Oak: the session has to pass the grove's oaks by.
    w.soul_separate(wc, 300).unwrap();
    assert!(!w.player_can_cut(kind(&w, "oak")));
    let oak = kind(&w, "oak");
    click_tree(&mut w, "tree");
    run_until(&mut w, 6000, |w| w.player.session.is_none());
    let full = w.defs.tree(oak).logs;
    assert!(
        w.trees
            .iter()
            .filter(|t| t.kind == oak)
            .all(|t| t.logs_left() == full),
        "an oak was cut below its level"
    );
}

#[test]
fn it_stays_inside_the_site_and_never_crosses_the_map() {
    let mut w = skilling();
    let site = grove(&w);
    // Nothing regrows in time, and the forest over the river is full of trees.
    for t in &mut w.trees {
        if t.site == Some(site) {
            t.state = TreeState::Depleted {
                regrow_at: 1_000_000,
            };
        }
    }
    let first = w.trees.iter().position(|t| t.site == Some(site)).unwrap();
    w.trees[first].state = TreeState::Grown { logs: 1 };
    w.trees[first].regrow_ticks = 1_000_000;
    let pos = w.trees[first].pos;
    click(&mut w, pos);
    assert!(run_until(&mut w, 600, |w| w.player.session.is_none()));
    assert_eq!(end_of(&w), SessionEnd::NoTargets);
    assert!(
        w.player.activity.contains("another site"),
        "{}",
        w.player.activity
    );
    assert!(pos.is_adjacent(w.player.actor.pos()), "it wandered off");
}

#[test]
fn it_never_walks_further_than_its_reach() {
    let mut w = skilling();
    w.defs.player.gather_reach_tiles = 0;
    let start = tree_pos(&w, "tree");
    click(&mut w, start);
    run_until(&mut w, 400, chopping);
    let stood = w.player.actor.pos();
    // With nothing else in reach it stays put and waits for this tree.
    for _ in 0..800 {
        w.step();
        assert_eq!(w.player.actor.pos(), stood);
    }
}

#[test]
fn it_waits_for_regrowth_and_then_carries_on() {
    let mut w = skilling();
    let site = grove(&w);
    let wc = w.defs.skill_id("woodcutting").unwrap();
    let regrow = w.tick + 400;
    for t in &mut w.trees {
        if t.site == Some(site) {
            t.state = TreeState::Depleted { regrow_at: regrow };
        }
    }
    let first = w.trees.iter().position(|t| t.site == Some(site)).unwrap();
    w.trees[first].state = TreeState::Grown { logs: 1 };
    let pos = w.trees[first].pos;
    click(&mut w, pos);
    assert!(run_until(&mut w, 300, |w| w.player.actor.queue.is_empty()));
    let report = w.activity_report();
    assert_eq!(report.state, ActivityState::Waiting);
    assert!(report.title.contains("regrow"), "{}", report.title);
    let xp = w.player.xp(wc);
    let waited = w.player.time.waiting_recovery;
    w.run_ticks(50);
    assert_eq!(w.player.xp(wc), xp, "waiting earns nothing");
    assert!(w.player.time.waiting_recovery > waited);
    assert!(run_until(&mut w, 800, chopping), "it went back to work");
    assert!(w.player.session.is_some());
}

#[test]
fn a_tree_finished_by_a_spirit_does_not_end_the_session() {
    let mut w = skilling();
    let pos = tree_pos(&w, "tree");
    click(&mut w, pos);
    run_until(&mut w, 400, chopping);
    let id = w.tree_at(pos).unwrap().id;
    w.trees[id].state = TreeState::Depleted {
        regrow_at: w.tick + 600,
    };
    w.run_ticks(3);
    assert!(w.player.session.is_some());
    assert!(run_until(&mut w, 300, chopping));
    assert_ne!(w.session_target(), Some(pos));
}

#[test]
fn turning_targeting_off_stops_at_the_end_of_the_tree() {
    let mut w = skilling();
    w.apply(Command::SetAutoGather(false)).unwrap();
    let pos = tree_pos(&w, "tree");
    click(&mut w, pos);
    assert!(run_until(&mut w, 800, |w| w.player.session.is_none()));
    assert_eq!(end_of(&w), SessionEnd::TreeFinished);
    assert!(!w.tree_at(pos).unwrap().is_grown());
    assert_eq!(w.trees.iter().filter(|t| !t.is_grown()).count(), 1);
}

#[test]
fn moving_stops_the_session_at_once() {
    let mut w = skilling();
    click_tree(&mut w, "tree");
    run_until(&mut w, 400, chopping);
    w.run_ticks(5);
    let dx = if w.grid.is_walkable(TilePos::new(
        w.player.actor.pos().x + 1,
        w.player.actor.pos().y,
    )) {
        1
    } else {
        -1
    };
    w.apply(Command::PlayerStep { dx, dy: 0 }).unwrap();
    assert!(w.player.session.is_none());
    assert_eq!(end_of(&w), SessionEnd::Player);
    let logs = w.player.actor.inventory.total();
    w.run_ticks(100);
    assert_eq!(w.player.actor.inventory.total(), logs);
    assert!(w.player.actor.queue.is_empty());
}

#[test]
fn walking_or_a_storage_click_ends_it_too() {
    let mut w = skilling();
    click_tree(&mut w, "tree");
    run_until(&mut w, 400, chopping);
    let main = w.main().pos;
    click(&mut w, main);
    assert!(w.player.session.is_none());
    run_until(&mut w, 400, |w| w.player.actor.queue.is_empty());
    w.run_ticks(30);
    assert!(w.player.actor.queue.is_empty(), "no return trip on its own");
}

#[test]
fn stop_finishes_the_step_and_cancels_everything_else() {
    let mut w = skilling();
    click_tree(&mut w, "tree");
    w.run_ticks(1);
    assert!(w.player.actor.mover.progress > 0);
    w.apply(Command::PlayerStop).unwrap();
    assert!(w.player.session.is_none());
    assert!(run_until(&mut w, 10, |w| w.player.actor.mover.progress == 0));
    w.run_ticks(5);
    assert!(w.player.actor.queue.is_empty());
    assert!(w.player.actor.mover.path.is_empty());
    assert_eq!(w.activity_report().state, ActivityState::Idle);
}

#[test]
fn a_click_on_another_tree_keeps_the_session_and_its_count() {
    let mut w = skilling();
    click_tree(&mut w, "tree");
    run_until(&mut w, 600, |w| {
        w.player.session.as_ref().is_some_and(|s| s.units >= 2)
    });
    let target = w.session_target().unwrap();
    let other = w
        .trees
        .iter()
        .find(|t| t.is_grown() && t.pos != target && t.site == Some(grove(&w)))
        .unwrap()
        .pos;
    let sessions = w.player.log.sessions;
    click(&mut w, other);
    assert_eq!(w.player.log.sessions, sessions);
    assert!(w.player.session.as_ref().unwrap().units >= 2);
    assert_eq!(w.session_target(), Some(other));
}

#[test]
fn the_activity_report_says_what_is_going_on() {
    let mut w = skilling();
    assert_eq!(w.activity_report().state, ActivityState::Idle);
    click_tree(&mut w, "tree");
    run_until(&mut w, 400, chopping);
    let r = w.activity_report();
    assert_eq!(r.state, ActivityState::Working);
    assert!(r.title.contains("Chopping"));
    assert!(r.progress.is_some());
    run_until(&mut w, 6000, |w| w.player.session.is_none());
    let r = w.activity_report();
    assert_eq!(r.state, ActivityState::Blocked);
    assert!(r.detail.unwrap().contains("Main Storage"));
}

// ---------------------------------------------------------------------------
// Recipes at the bench
// ---------------------------------------------------------------------------

#[test]
fn the_player_can_make_a_lesser_recipe_on_purpose() {
    let mut w = recipes();
    let rough = recipe(&w, "rough_planks");
    let beams = recipe(&w, "structural_beams");
    w.apply(Command::SetPlayerRecipe(Some(rough))).unwrap();
    fill_bag(&mut w);
    let logs = w.defs.resource_id("logs").unwrap();
    assert_eq!(w.player.actor.inventory.get(logs), 28);
    // Beams could be made here too, from what's at the bench.
    let hardwood = w.defs.resource_id("hardwood").unwrap();
    let id = w.station().unwrap().id;
    w.buildings[id].storage.deposit(hardwood, 10);
    *w.ledger.seeded.entry(hardwood).or_default() += 10;
    let at = bench(&w);
    click(&mut w, at);
    assert!(run_until(&mut w, 2000, |w| w.player.session.is_none()));
    assert_eq!(w.ledger.batches.get(&rough), Some(&14));
    assert_eq!(w.ledger.batches.get(&beams), None, "no beams were forced");
}

#[test]
fn automatic_choice_still_makes_the_best_recipe() {
    let mut w = recipes();
    fill_bag(&mut w);
    let at = bench(&w);
    click(&mut w, at);
    run_until(&mut w, 2000, |w| w.player.session.is_none());
    let beams = recipe(&w, "structural_beams");
    assert_eq!(w.ledger.batches.get(&beams), Some(&14));
}

#[test]
fn a_recipe_above_the_players_level_is_refused() {
    let mut w = World::from_scenario(ScenarioKind::WorkshopManual);
    let precise = recipe(&w, "precise_planks");
    let err = w
        .apply(Command::SetPlayerRecipe(Some(precise)))
        .unwrap_err();
    assert!(err.contains("level 5"), "{err}");
    assert_eq!(w.player.recipe, None);
}

#[test]
fn repeated_work_ends_when_the_materials_run_out_and_says_why() {
    let mut w = recipes();
    let ww = w.defs.skill_id("woodworking").unwrap();
    let before = w.player.xp(ww);
    let precise = recipe(&w, "precise_planks");
    w.apply(Command::SetPlayerRecipe(Some(precise))).unwrap();
    fill_bag(&mut w);
    let at = bench(&w);
    click(&mut w, at);
    assert!(run_until(&mut w, 2000, |w| w.player.session.is_none()));
    assert_eq!(w.player.log.workshop_interactions, 1);
    assert_eq!(end_of(&w), SessionEnd::Blocked);
    assert!(
        w.player.activity.contains("You need 3 Logs"),
        "{}",
        w.player.activity
    );
    assert!(
        w.player.activity.contains("you have 1"),
        "{}",
        w.player.activity
    );
    // 28 logs is 9 batches, with 1 log left.
    assert_eq!(w.ledger.batches.get(&precise), Some(&9));
    assert_eq!(w.player.xp(ww) - before, 9 * w.defs.recipe(precise).xp);
}

#[test]
fn a_click_with_nothing_to_work_with_names_what_is_missing() {
    let mut w = recipes();
    let beams = recipe(&w, "structural_beams");
    w.apply(Command::SetPlayerRecipe(Some(beams))).unwrap();
    let at = bench(&w);
    let err = w.apply(Command::PlayerInteract(at)).unwrap_err();
    assert!(err.contains("You need 2 Hardwood Logs"), "{err}");
    assert_eq!(w.player.log.refused_interactions, 1);
}

#[test]
fn empty_hands_take_what_the_chosen_recipe_needs() {
    let mut w = recipes();
    let beams = recipe(&w, "structural_beams");
    w.apply(Command::SetPlayerRecipe(Some(beams))).unwrap();
    fill_bag(&mut w);
    let hardwood = w.defs.resource_id("hardwood").unwrap();
    assert_eq!(w.player.actor.inventory.get(hardwood), 28);
}

#[test]
fn changing_recipe_mid_batch_gives_no_free_progress() {
    let mut w = recipes();
    let precise = recipe(&w, "precise_planks");
    let rough = recipe(&w, "rough_planks");
    w.apply(Command::SetPlayerRecipe(Some(precise))).unwrap();
    fill_bag(&mut w);
    let at = bench(&w);
    click(&mut w, at);
    run_until(&mut w, 200, |w| w.player.actor.action_ticks > 5);
    w.apply(Command::SetPlayerRecipe(Some(rough))).unwrap();
    assert_eq!(w.player.actor.action_ticks, 0);
    assert!(w.player.session.is_some(), "the session carries on");
}

#[test]
fn a_full_bag_and_shelf_is_explained() {
    let mut w = recipes();
    let planks = w.defs.resource_id("planks").unwrap();
    let logs = w.defs.resource_id("logs").unwrap();
    let id = w.station().unwrap().id;
    let room = w.buildings[id].storage.free();
    w.buildings[id].storage.deposit(planks, room - 2);
    w.buildings[id].storage.deposit(logs, 2);
    *w.ledger.seeded.entry(planks).or_default() += room as u64 - 2;
    *w.ledger.seeded.entry(logs).or_default() += 2;
    let bag = w.player.actor.inventory.free();
    w.player.actor.inventory.add(planks, bag);
    *w.ledger.seeded.entry(planks).or_default() += bag as u64;
    let block = w.craft_block(id).unwrap();
    assert!(matches!(block, CraftBlock::NoRoom { .. }), "{block:?}");
    assert!(w.craft_block_text(&block).contains("both full"));
}

#[test]
fn a_taken_bench_says_so_and_keeps_the_players_place_once_they_have_it() {
    let mut w = World::from_scenario(ScenarioKind::ProductionBottleneck);
    let id = w.station().unwrap().id;
    let slots = w.defs.building(w.buildings[id].kind).slots;
    run_until(&mut w, 2000, |w| w.slots_used(id) >= slots);
    assert_eq!(w.slots_used(id), slots);
    let logs = w.defs.resource_id("logs").unwrap();
    w.player.actor.inventory.add(logs, 4);
    *w.ledger.seeded.entry(logs).or_default() += 4;
    let at = w.buildings[id].pos;
    let err = w.apply(Command::PlayerInteract(at)).unwrap_err();
    assert!(err.contains("places"), "{err}");
    assert!(err.contains("Training tab"), "{err}");
}

#[test]
fn moving_away_from_the_bench_stops_work_and_costs_nothing() {
    let mut w = recipes();
    fill_bag(&mut w);
    let logs = w.defs.resource_id("logs").unwrap();
    let at = bench(&w);
    click(&mut w, at);
    run_until(&mut w, 200, |w| w.player.actor.action_ticks > 3);
    let held = w.player.actor.inventory.get(logs);
    let p = w.player.actor.pos();
    let (dx, dy) = [(1, 0), (-1, 0), (0, 1), (0, -1)]
        .into_iter()
        .find(|(x, y)| w.grid.is_walkable(TilePos::new(p.x + x, p.y + y)))
        .unwrap();
    w.apply(Command::PlayerStep { dx, dy }).unwrap();
    assert!(w.player.session.is_none());
    w.run_ticks(40);
    assert_eq!(
        w.player.actor.inventory.get(logs),
        held,
        "no batch finished"
    );
}

// ---------------------------------------------------------------------------
// Soul Separation
// ---------------------------------------------------------------------------

#[test]
fn losing_a_chosen_recipe_stops_work_and_says_why() {
    let mut w = recipes();
    let ww = w.defs.skill_id("woodworking").unwrap();
    let precise = recipe(&w, "precise_planks");
    w.apply(Command::SetPlayerRecipe(Some(precise))).unwrap();
    fill_bag(&mut w);
    let at = bench(&w);
    click(&mut w, at);
    run_until(&mut w, 200, |w| {
        matches!(w.player.actor.queue.front(), Some(Task::Craft { .. }))
    });
    let xp = w.player.xp(ww);
    w.soul_separate(ww, xp).unwrap();
    assert!(run_until(&mut w, 60, |w| w.player.session.is_none()));
    assert_eq!(end_of(&w), SessionEnd::Blocked);
    assert!(
        w.player.activity.contains("level 5"),
        "{}",
        w.player.activity
    );
    assert_eq!(w.player.xp(ww), 0, "no XP after the sacrifice");
}

#[test]
fn losing_the_tree_being_cut_ends_the_session_clearly() {
    let mut w = skilling();
    let wc = w.defs.skill_id("woodcutting").unwrap();
    click_tree(&mut w, "oak");
    run_until(&mut w, 400, chopping);
    w.soul_separate(wc, 300).unwrap();
    assert!(w.player.session.is_none());
    assert!(w.player.activity.contains("no longer cut Oak"));
    w.run_ticks(100);
    assert!(w.player.actor.queue.is_empty());
}

#[test]
fn losing_a_tier_is_announced_with_what_the_spirit_keeps() {
    let mut w = skilling();
    let wc = w.defs.skill_id("woodcutting").unwrap();
    w.soul_separate(wc, 388).unwrap();
    let n = w.notices.recent(1).next().unwrap();
    assert!(
        n.text.contains("Your Woodcutting Spirit can cut"),
        "{}",
        n.text
    );
    assert!(
        n.text.contains("can't cut Oak until Woodcutting level 5"),
        "{}",
        n.text
    );
    assert!(n.text.contains("for good"), "{}", n.text);
}

#[test]
fn a_new_tier_says_where_what_and_why() {
    let mut w = World::from_scenario(ScenarioKind::Regrind);
    click_tree(&mut w, "tree");
    run_until(&mut w, 800, |w| {
        w.notices.contains("Oak trees are now available")
    });
    let n = w
        .notices
        .iter()
        .find(|n| n.text.contains("Oak trees"))
        .unwrap();
    assert!(n.text.contains("Starting Grove"), "{}", n.text);
    assert!(n.text.contains("30 XP"), "{}", n.text);
    assert!(n.text.contains("Twice the XP"), "{}", n.text);
}

// ---------------------------------------------------------------------------
// Supported training
// ---------------------------------------------------------------------------

#[test]
fn the_three_modes_set_and_read_back() {
    let mut w = World::from_scenario(ScenarioKind::SupportedRetraining);
    assert_eq!(w.training_mode(), TrainingMode::Manual);
    w.apply(Command::SetTrainingMode(TrainingMode::Relaxed))
        .unwrap();
    assert_eq!(w.training_mode(), TrainingMode::Relaxed);
    assert!(w.settlement.training.supply && w.settlement.training.collect);
    assert!(w.spirits.iter().all(|s| s.policy.focus != Focus::Training));
    w.apply(Command::SetTrainingMode(TrainingMode::Optimized))
        .unwrap();
    assert_eq!(w.training_mode(), TrainingMode::Optimized);
    let serving = w
        .spirits
        .iter()
        .filter(|s| s.policy.focus == Focus::Training);
    assert_eq!(serving.count(), 2, "one hauler and one gatherer");
    w.apply(Command::SetTrainingMode(TrainingMode::Manual))
        .unwrap();
    assert_eq!(w.training_mode(), TrainingMode::Manual);
    assert!(w.spirits.iter().all(|s| s.policy.focus == Focus::Anything));
}

#[test]
fn the_bench_session_waits_for_spirits_with_one_click() {
    let mut w = World::from_scenario(ScenarioKind::SupportedRetraining);
    w.apply(Command::SetTrainingMode(TrainingMode::Relaxed))
        .unwrap();
    let ww = w.defs.skill_id("woodworking").unwrap();
    w.apply(Command::SetProductionEnabled(false)).unwrap();
    w.apply(Command::SetInputReserve(10_000)).unwrap();
    let at = bench(&w);
    // Nothing at hand yet, and the click is still good.
    click(&mut w, at);
    assert!(w.player.session.is_some());
    assert!(run_until(&mut w, 200, waiting));
    assert_eq!(w.activity_report().state, ActivityState::Waiting);
    // Logs become available, and the player starts without another click.
    w.apply(Command::SetInputReserve(0)).unwrap();
    let start = w.player.xp(ww);
    assert!(run_until(&mut w, 1500, |w| w.player.xp(ww) >= start + 100));
    assert_eq!(w.player.log.workshop_interactions, 1);
    assert_eq!(w.player.log.storage_interactions, 0);
    assert!(w.player.time.waiting_input > 0);
}

#[test]
fn a_waiting_player_keeps_their_place_at_the_bench() {
    let mut w = World::from_scenario(ScenarioKind::SupportedRetraining);
    w.apply(Command::SetTrainingMode(TrainingMode::Relaxed))
        .unwrap();
    w.apply(Command::SetInputReserve(10_000)).unwrap();
    let at = bench(&w);
    click(&mut w, at);
    assert!(run_until(&mut w, 200, waiting));
    let id = w.station().unwrap().id;
    assert!(w.slots_used(id) >= 1);
    assert!(
        w.spirits
            .iter()
            .filter(|s| {
                s.actor
                    .queue
                    .iter()
                    .any(|t| matches!(t, Task::Craft { building, .. } if *building == id))
            })
            .count()
            < 2
    );
}

#[test]
fn waiting_ends_with_the_reason_when_nothing_comes() {
    let mut w = World::from_scenario(ScenarioKind::SupportedRetraining);
    w.apply(Command::SetTrainingMode(TrainingMode::Relaxed))
        .unwrap();
    // Nothing can be spared: the reserve covers every log in storage.
    w.apply(Command::SetInputReserve(10_000)).unwrap();
    w.defs.player.craft_wait_ticks = 50;
    let at = bench(&w);
    click(&mut w, at);
    assert!(run_until(&mut w, 400, |w| w.player.session.is_none()));
    assert_eq!(end_of(&w), SessionEnd::Blocked);
    assert!(
        w.player.activity.contains("Nothing arrived"),
        "{}",
        w.player.activity
    );
    assert!(
        w.player.activity.contains("reserve"),
        "{}",
        w.player.activity
    );
}

#[test]
fn the_plan_names_what_limits_training() {
    let mut w = World::from_scenario(ScenarioKind::SupportedRetraining);
    assert!(w.training_plan().limit.contains("carrying everything"));
    w.apply(Command::SetTrainingMode(TrainingMode::Relaxed))
        .unwrap();
    assert!(w.training_plan().limit.contains("supply is the limit"));
    w.run_ticks(1);
    w.apply(Command::SetObjectiveTarget(500)).unwrap();
    w.apply(Command::SetInputReserve(10_000)).unwrap();
    let plan = w.training_plan();
    assert_eq!(plan.spare, 0);
    assert!(plan.limit.contains("No spare Logs"), "{}", plan.limit);
    assert!(plan.use_per_min > 0);
    assert!(plan.checks.iter().any(|(_, done)| !done));
}

#[test]
fn support_never_gives_spirits_or_the_player_free_xp() {
    let mut w = World::from_scenario(ScenarioKind::SupportedRetraining);
    w.apply(Command::SetTrainingMode(TrainingMode::Optimized))
        .unwrap();
    let ww = w.defs.skill_id("woodworking").unwrap();
    let spirit_xp: Vec<u64> = w.spirits.iter().map(|s| s.xp()).collect();
    let before = w.player.xp(ww);
    // The player is doing nothing.
    w.run_ticks(3000);
    assert_eq!(w.player.xp(ww), before);
    assert_eq!(
        w.spirits.iter().map(|s| s.xp()).collect::<Vec<_>>(),
        spirit_xp
    );
}

// ---------------------------------------------------------------------------
// Storage and pacing
// ---------------------------------------------------------------------------

#[test]
fn a_nearly_full_storage_is_explained_once_with_real_options() {
    let mut w = World::from_scenario(ScenarioKind::FullStorage);
    w.run_ticks(2);
    let advice = w.storage_advice().unwrap();
    assert!(advice.contains("nearly full"), "{advice}");
    assert!(advice.contains("Storehouse"), "{advice}");
    assert!(advice.contains("saw logs"), "{advice}");
    let total = w.notices.total;
    assert!(w.notices.contains("nearly full"));
    w.run_ticks(200);
    assert_eq!(w.notices.total, total, "it says so once");
    assert_eq!(w.settlement_glance().attention, Some(advice));
}

#[test]
fn a_bag_that_cannot_be_emptied_counts_as_blocked_time() {
    let mut w = World::from_scenario(ScenarioKind::FullStorage);
    let main = w.main().pos;
    for _ in 0..2 {
        w.run_ticks(700);
        click_tree(&mut w, "tree");
        run_until(&mut w, 6000, |w| w.player.session.is_none());
        click(&mut w, main);
        run_until(&mut w, 400, |w| w.player.actor.queue.is_empty());
    }
    assert!(w.main().storage.free() == 0);
    let before = w.player.time.blocked_storage;
    w.run_ticks(50);
    assert!(w.player.actor.inventory.is_full());
    assert_eq!(w.player.time.blocked_storage, before + 50);
}

#[test]
fn only_chopping_reaches_the_storage_wall_and_the_game_warned_first() {
    let wall = storage_wall("normal", 120).unwrap();
    let stuck = wall
        .stuck_at
        .expect("storage stops a player who spends nothing");
    assert!(!wall.reached_goal);
    assert!(wall.warned_at.unwrap() < stuck);
    assert!(wall.xp_at_stuck < wall.goal_xp);
}

// ---------------------------------------------------------------------------
// Measurements
// ---------------------------------------------------------------------------

#[test]
fn the_play_log_counts_interactions_and_does_not_change_play() {
    let mut a = skilling();
    let p = tree_pos(&a, "tree");
    click(&mut a, p);
    let mut b = a.clone();
    a.run_ticks(300);
    let _ = a.play_report();
    a.run_ticks(300);
    b.run_ticks(600);
    assert_eq!(a.state_text(), b.state_text());
    assert_eq!(a.player.log.gather_interactions, 1);
    assert_eq!(a.player.log.sessions, 1);
    assert!(a.player.log.training_ticks > 0);
}

#[test]
fn regained_levels_are_timed() {
    let mut w = skilling();
    let wc = w.defs.skill_id("woodcutting").unwrap();
    w.soul_separate(wc, 100).unwrap();
    let r = w.player.log.regains[0].clone();
    assert_eq!((r.level, r.xp_given, r.done), (5, 100, None));
    click_tree(&mut w, "tree");
    assert!(run_until(&mut w, 2000, |w| w.player.log.regains[0]
        .done
        .is_some()));
    assert!(w.play_report().contains("back after"));
}

#[test]
fn the_report_covers_everything_a_session_is_compared_on() {
    let mut w = skilling();
    click_tree(&mut w, "tree");
    w.run_ticks(900);
    let report = w.play_report();
    for part in [
        "Gathering",
        "Processing",
        "Moving",
        "Waiting for trees",
        "Waiting for materials",
        "Stuck with a full bag",
        "Trees 1",
        "Woodcutting",
        "Spirits while you trained",
    ] {
        assert!(report.contains(part), "missing {part}:\n{report}");
    }
}

#[test]
fn the_glance_counts_every_spirit_once() {
    let mut w = World::from_scenario(ScenarioKind::WorkshopAutomated);
    w.run_ticks(300);
    let g = w.settlement_glance();
    assert_eq!(
        (g.working + g.waiting + g.blocked + g.idle) as usize,
        w.spirits.len()
    );
}

// ---------------------------------------------------------------------------
// Saves and replay
// ---------------------------------------------------------------------------

fn round_trip(w: &World) -> World {
    decode(&encode(w, "skilling_session")).unwrap().0
}

fn continues_exactly(mut w: World) {
    let mut l = round_trip(&w);
    assert_eq!(l.state_text(), w.state_text());
    for _ in 0..1500 {
        w.step();
        l.step();
    }
    assert_eq!(l.state_text(), w.state_text());
    assert_eq!(l.state_hash(), w.state_hash());
    l.check_conservation().unwrap();
}

#[test]
fn saving_while_chopping_continues_exactly() {
    let mut w = skilling();
    click_tree(&mut w, "tree");
    run_until(&mut w, 400, chopping);
    w.run_ticks(7);
    assert!(w.player.actor.action_ticks > 0);
    continues_exactly(w);
}

#[test]
fn saving_while_walking_to_the_next_tree_continues_exactly() {
    let mut w = skilling();
    click_tree(&mut w, "tree");
    run_until(&mut w, 800, |w| {
        w.player.session.as_ref().is_some_and(|s| s.units >= 3)
            && matches!(w.player.actor.queue.front(), Some(Task::Move { .. }))
            && w.player.actor.mover.progress > 0
    });
    continues_exactly(w);
}

#[test]
fn saving_while_waiting_for_regrowth_continues_exactly() {
    let mut w = skilling();
    let site = grove(&w);
    let regrow = w.tick + 300;
    for t in &mut w.trees {
        if t.site == Some(site) {
            t.state = TreeState::Depleted { regrow_at: regrow };
        }
    }
    let first = w.trees.iter().position(|t| t.site == Some(site)).unwrap();
    w.trees[first].state = TreeState::Grown { logs: 1 };
    let pos = w.trees[first].pos;
    click(&mut w, pos);
    run_until(&mut w, 300, |w| {
        w.player
            .session
            .as_ref()
            .is_some_and(|s| s.waiting_since.is_some())
    });
    continues_exactly(w);
}

#[test]
fn saving_at_the_bench_keeps_the_recipe_and_session() {
    let mut w = recipes();
    let rough = recipe(&w, "rough_planks");
    w.apply(Command::SetPlayerRecipe(Some(rough))).unwrap();
    fill_bag(&mut w);
    let at = bench(&w);
    click(&mut w, at);
    run_until(&mut w, 200, |w| w.player.actor.action_ticks > 4);
    let l = round_trip(&w);
    assert_eq!(l.player.recipe, Some(rough));
    assert!(matches!(
        l.player.session.as_ref().unwrap().kind,
        SessionKind::Crafting { .. }
    ));
    continues_exactly(w);
}

#[test]
fn saving_while_waiting_for_spirits_continues_exactly() {
    let mut w = World::from_scenario(ScenarioKind::SupportedRetraining);
    w.apply(Command::SetTrainingMode(TrainingMode::Relaxed))
        .unwrap();
    w.apply(Command::SetInputReserve(10_000)).unwrap();
    let at = bench(&w);
    click(&mut w, at);
    assert!(run_until(&mut w, 200, waiting));
    continues_exactly(w);
}

#[test]
fn a_session_with_an_impossible_target_is_refused() {
    let mut w = skilling();
    click_tree(&mut w, "tree");
    let text = encode(&w, "skilling_session");
    let json = text.split_once('\n').unwrap().1;
    let id = w.tree_at(w.session_target().unwrap()).unwrap().id;
    let damaged = json.replacen(&format!("\"target\":{id}"), "\"target\":9999", 1);
    assert_ne!(damaged, json);
    let sealed = spirit_sim::save::seal(SAVE_VERSION, &damaged);
    assert!(decode(&sealed).is_err());
}

#[test]
fn replays_with_sessions_are_identical() {
    let run = || {
        let mut w = skilling();
        click_tree(&mut w, "oak");
        w.run_ticks(2500);
        let main = w.main().pos;
        click(&mut w, main);
        w.run_ticks(400);
        click_tree(&mut w, "tree");
        w.run_ticks(600);
        (w.state_hash(), w.state_text())
    };
    assert_eq!(run(), run());
}

#[test]
fn no_time_passes_for_a_session_in_a_closed_game() {
    let mut w = skilling();
    let wc = w.defs.skill_id("woodcutting").unwrap();
    click_tree(&mut w, "tree");
    run_until(&mut w, 400, chopping);
    let xp = w.player.xp(wc);
    let tick = w.tick;
    let l = round_trip(&w);
    assert_eq!((l.tick, l.player.xp(wc)), (tick, xp));
}

fn fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    String::from_utf8(std::fs::read(path).unwrap()).unwrap()
}

#[test]
fn saves_from_v0_6_load_and_keep_working() {
    for name in ["v2_journey.save", "v2_workshop.save"] {
        let (mut w, meta) = decode(&fixture(name)).unwrap();
        assert_eq!(meta.version, 2, "{name}");
        assert!(
            w.player.session.is_none(),
            "{name}: nothing to continue yet"
        );
        assert!(w.player.auto_gather, "{name}: the new default applies");
        assert_eq!(w.player.recipe, None);
        let work = w.player.actor.queue.len();
        assert!(work > 0, "{name}: the saved job is still there");
        let xp = w.ledger.xp_earned;
        w.run_ticks(3000);
        w.check_conservation().unwrap();
        assert!(w.ledger.xp_earned > xp, "{name}: the job finished");
        assert!(
            w.player.actor.queue.is_empty(),
            "{name}: then the player stops"
        );
    }
}

#[test]
fn saves_from_v0_5_still_load() {
    for name in ["v1_journey.save", "v1_workshop.save"] {
        let (mut w, _) = decode(&fixture(name)).unwrap();
        w.run_ticks(600);
        w.check_conservation().unwrap();
    }
}

#[test]
fn an_upgraded_v0_6_save_keeps_its_original_and_can_start_sessions() {
    let dir = std::env::temp_dir().join(format!("spiritscape_v07_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("world.save"), fixture("v2_journey.save")).unwrap();
    let store = SaveStore::new(&dir);
    let loaded = store.load(Slot::Manual).unwrap();
    assert!(loaded.warning.unwrap().contains("world.save.v2"));
    assert!(dir.join("world.save.v2").exists());
    let mut w = loaded.world;
    let main = w.main().pos;
    click(&mut w, main);
    run_until(&mut w, 400, |w| w.player.actor.queue.is_empty());
    let pos = w
        .trees
        .iter()
        .find(|t| t.is_grown() && w.player_can_cut(t.kind) && t.site.is_some())
        .unwrap()
        .pos;
    click(&mut w, pos);
    assert!(w.player.session.is_some());
    store.write(Slot::Manual, &w, "new_game").unwrap();
    assert_eq!(store.meta(Slot::Manual).unwrap().version, SAVE_VERSION);
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------------
// The whole path
// ---------------------------------------------------------------------------

#[test]
fn the_full_session_works_under_normal_rules_with_no_help() {
    let (w, s) = play_full_session("normal", true, 45).unwrap();
    w.check_conservation().unwrap();
    assert!(s.resumed_exactly, "save and resume changed something");
    assert_eq!(s.modes.len(), 3);
    assert!(s.seconds >= 45 * 60);
    assert!(s.advanced_xp.0 > 0);
    assert!(s.report.contains("back after"), "{}", s.report);
    // Planks were spent on an improvement, by spirits, with no developer help.
    assert!(w.settlement_glance().working + w.settlement_glance().idle > 0);
    assert_eq!(
        w.ledger.xp_earned,
        w.player.log.xp_earned.values().sum::<u64>()
    );
}

#[test]
fn continuous_gathering_saves_clicks_without_changing_the_xp_rules() {
    let (auto, a) = play_full_session("development", true, 10).unwrap();
    let (manual, m) = play_full_session("development", false, 10).unwrap();
    let trees = |w: &World| w.player.log.gather_interactions;
    assert!(
        trees(&auto) * 2 < trees(&manual),
        "{} vs {}",
        trees(&auto),
        trees(&manual)
    );
    assert!(a.resumed_exactly && m.resumed_exactly);
    assert!(auto.player.time.total() > 0);
}

#[test]
fn relaxed_training_trades_pace_for_clicks() {
    let (_, s) = play_full_session("normal", true, 25).unwrap();
    let get = |m: TrainingMode| &s.modes.iter().find(|(x, _)| *x == m).unwrap().1;
    let manual = get(TrainingMode::Manual);
    let relaxed = get(TrainingMode::Relaxed);
    assert!(relaxed.actions < manual.actions, "fewer clicks");
    assert_eq!(relaxed.storage_trips, 0, "no carrying");
    assert!(relaxed.seconds > manual.seconds, "and it takes longer");
}

#[test]
fn optimized_training_pays_only_once_the_investment_is_made() {
    let (_, s) = play_full_session("normal", true, 25).unwrap();
    let get = |m: TrainingMode| &s.modes.iter().find(|(x, _)| *x == m).unwrap().1;
    let manual = get(TrainingMode::Manual);
    let early = get(TrainingMode::Optimized);
    assert!(
        early.seconds > s.invested.seconds,
        "no shortcut without the work"
    );
    assert!(
        s.invested.seconds < manual.seconds,
        "faster than doing it yourself"
    );
    assert!(s.invested.actions < manual.actions, "with far fewer clicks");
}

#[test]
fn following_the_storage_advice_keeps_a_long_session_moving() {
    let (w, s) = play_full_session("normal", true, 45).unwrap();
    assert!(s.seconds <= 47 * 60, "the session ran over: {}", s.seconds);
    let t = &w.player.time;
    assert!(
        t.blocked_storage * 100 / t.total() < 2,
        "stuck for {} of {} ticks",
        t.blocked_storage,
        t.total()
    );
}
