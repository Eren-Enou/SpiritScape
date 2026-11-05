//! A reference player: plays a real game through commands alone, the way a
//! steady person would, so the whole progression loop can be measured and
//! tested without a window.
//!
//! It has no special access. It reads the world, waits a reaction delay after
//! each action finishes, and issues the same commands a click would. It plays
//! efficiently and never gets distracted, so its times are a lower bound on a
//! person's, not a prediction of them.

use crate::defs::{BuildingKindId, JobKind, ResourceId, SkillId};
use crate::entities::{Focus, TreeId};
use crate::journey::{Milestone, Phase};
use crate::metrics::TimeUse;
use crate::tasks::Task;
use crate::training::TrainingMode;
use crate::world::{Command, World};

/// Seconds between finishing one action and starting the next.
const REACTION_SECONDS: u64 = 1;

/// What the player is working on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Goal {
    /// Chop and deposit until this much XP.
    ChopTo(u64),
    /// Saw logs until this much XP.
    SawTo(u64),
    /// Chop and deposit forever.
    Chop,
}

#[derive(Clone, Debug)]
pub struct Bot {
    wc: SkillId,
    ww: SkillId,
    logs: ResourceId,
    next_action: u64,
    /// Commands issued that a person would have clicked.
    pub actions: u32,
    /// Trips ended at a storage to unload or fetch.
    pub storage_trips: u32,
    /// Acts on the game's advice when storage runs short.
    pub heeds_storage_advice: bool,
    /// Tick of the last time it acted on that advice.
    last_relief: u64,
}

impl Bot {
    pub fn new(w: &World) -> Self {
        Self {
            wc: w.defs.skill_id("woodcutting").expect("woodcutting"),
            ww: w.defs.skill_id("woodworking").expect("woodworking"),
            logs: w.defs.objective.resource,
            next_action: 0,
            actions: 0,
            storage_trips: 0,
            heeds_storage_advice: false,
            last_relief: 0,
        }
    }

    fn click(&mut self, w: &mut World, cmd: Command) -> bool {
        self.actions += 1;
        self.next_action = w.tick + REACTION_SECONDS * w.defs.ticks_per_second as u64;
        w.apply(cmd).is_ok()
    }

    fn nearest_tree(&self, w: &World) -> Option<TreeId> {
        let field = w.grid.distance_field(&[w.player.actor.pos()]);
        // Most XP first, then the shortest walk.
        w.trees
            .iter()
            .filter(|t| t.is_grown() && w.player_can_cut(t.kind))
            .filter_map(|t| {
                let walk = w.grid.approach_distance(&field, t.pos)?;
                let xp = w.defs.tree(t.kind).xp_per_log;
                // Crossing the river for a better tree isn't worth it.
                (walk <= 16).then_some((walk as i64 * 2 - xp as i64 / 15 * 6, t.id))
            })
            .min()
            .map(|(_, id)| id)
    }

    /// Where the player drops off logs: the cache when one is available.
    fn drop_off(&self, w: &World) -> crate::grid::TilePos {
        match w.training_cache() {
            Some(c) if c.storage.free() > 0 && w.settlement.training.collect => c.pos,
            _ => w.main().pos,
        }
    }

    /// Does what the storage advice says: orders more room when it can, and
    /// gives spirits more reason to use up logs. A person would do this from
    /// the Production tab.
    fn relieve_storage(&mut self, w: &mut World) {
        let tps = w.defs.ticks_per_second as u64;
        if w.storage_advice().is_none() || w.tick < self.last_relief + 60 * tps {
            return;
        }
        self.last_relief = w.tick;
        let target = w.settlement.order.target + 40;
        self.click(w, Command::SetProductionTarget(target));
        let room = w.defs.buildings.iter().position(|b| b.capacity_bonus > 0);
        if let Some(i) = room {
            let kind = BuildingKindId(i as u16);
            let def = w.defs.building(kind);
            let main = &w.main().storage;
            if def.cost.iter().all(|(r, n)| main.available(*r) >= *n) {
                self.click(w, Command::RequestBuilding(kind));
            }
        }
    }

    fn chop_step(&mut self, w: &mut World) {
        if self.heeds_storage_advice {
            self.relieve_storage(w);
        }
        let bag = &w.player.actor.inventory;
        let spot = self.nearest_tree(w);
        let empty_handed = bag.total() == 0;
        let tps = w.defs.ticks_per_second as u64;
        if bag.is_full() && w.main().storage.free() == 0 {
            // Nothing to do but wait for room; a person wouldn't keep clicking.
            self.next_action = w.tick + 10 * tps;
        } else if bag.is_full() || spot.is_none() && !empty_handed {
            let target = self.drop_off(w);
            self.storage_trips += 1;
            self.click(w, Command::PlayerInteract(target));
        } else if let Some(t) = spot {
            let pos = w.trees[t].pos;
            self.click(w, Command::PlayerInteract(pos));
        }
    }

    fn saw_step(&mut self, w: &mut World) {
        let Some(station) = w.station().map(|b| b.pos) else {
            return;
        };
        let bag = &w.player.actor.inventory;
        let logs = bag.get(self.logs);
        let bench = w.bench_stock();
        if bag.is_full() && logs < 2 {
            let main = w.main().pos;
            self.storage_trips += 1;
            self.click(w, Command::PlayerInteract(main));
        } else if logs >= 2 || bench >= 2 || w.has_training_help() && w.settlement.training.supply {
            self.click(w, Command::PlayerInteract(station));
        } else if bag.total() > 0
            || (w.main().storage.available(self.logs) >= 2 && !w.settlement.training.supply)
        {
            let main = w.main().pos;
            self.storage_trips += 1;
            self.click(w, Command::PlayerInteract(main));
        } else if !w.settlement.training.supply {
            // Nothing in storage: go and cut some.
            self.chop_step(w);
        }
    }

    /// Issues the next action when the player is free. Call once per tick.
    pub fn play(&mut self, w: &mut World, goal: Goal) {
        let busy = !w.player.actor.queue.is_empty() || w.player.session.is_some();
        if busy || w.tick < self.next_action {
            return;
        }
        match goal {
            Goal::Chop | Goal::ChopTo(_) => self.chop_step(w),
            Goal::SawTo(_) => self.saw_step(w),
        }
    }

    fn goal_met(&self, w: &World, goal: Goal) -> bool {
        match goal {
            Goal::ChopTo(xp) => w.player.xp(self.wc) >= xp,
            Goal::SawTo(xp) => w.player.xp(self.ww) >= xp,
            Goal::Chop => false,
        }
    }

    /// Plays toward `goal` until it's met or the time runs out. Returns
    /// whether it was met.
    pub fn run(&mut self, w: &mut World, goal: Goal, max_ticks: u64) -> bool {
        for _ in 0..max_ticks {
            if self.goal_met(w, goal) {
                return true;
            }
            self.play(w, goal);
            w.step();
        }
        self.goal_met(w, goal)
    }

    /// Keeps chopping while `done` is false.
    pub fn chop_until(
        &mut self,
        w: &mut World,
        max_ticks: u64,
        mut done: impl FnMut(&World) -> bool,
    ) -> bool {
        for _ in 0..max_ticks {
            if done(w) {
                return true;
            }
            self.play(w, Goal::Chop);
            w.step();
        }
        done(w)
    }

    /// Finishes whatever's in the bag and stands still.
    pub fn stop(&mut self, w: &mut World) {
        while !w.player.actor.queue.is_empty()
            && !matches!(w.player.actor.queue.front(), Some(Task::Chop { .. }))
        {
            w.step();
        }
        let _ = w.apply(Command::PlayerStop);
    }
}

/// How a stretch of play went.
#[derive(Clone, Debug, Default)]
pub struct Stretch {
    pub seconds: u64,
    pub actions: u32,
    pub storage_trips: u32,
    pub player: TimeUse,
    /// Combined spirit time use.
    pub spirits: TimeUse,
    /// Stock gained in Main Storage.
    pub logs_gained: i64,
    pub planks_gained: i64,
}

/// A first playthrough, from an empty start to a retrained skill.
#[derive(Clone, Debug, Default)]
pub struct PacingReport {
    pub profile: String,
    /// Each milestone and when it was reached, in seconds.
    pub milestones: Vec<(Milestone, u64)>,
    /// Which phase the guidance reached.
    pub final_phase: Phase,
    pub woodcutting_sacrificed: u64,
    pub woodworking_sacrificed: u64,
    /// Regaining the sacrificed Woodcutting XP, starting from the same moment.
    pub retrain_alone: Stretch,
    pub retrain_supported: Stretch,
    /// Regaining the sacrificed Woodworking XP, starting from the same moment.
    pub resaw_alone: Stretch,
    pub resaw_supported: Stretch,
    pub total_seconds: u64,
    pub player_time: TimeUse,
}

fn spirit_time(w: &World) -> TimeUse {
    let mut t = TimeUse::default();
    for s in &w.spirits {
        t.add(&s.time.total);
    }
    t
}

fn measure(mut w: World, bot: &Bot, goal: Goal, limit: u64) -> Stretch {
    let mut bot = bot.clone();
    bot.actions = 0;
    bot.storage_trips = 0;
    let (start_tick, start_player, start_spirits) = (w.tick, w.player.time, spirit_time(&w));
    let logs = w.defs.objective.resource;
    let planks = w.settlement.order.output;
    let stock = |w: &World, r| w.main().storage.get(r) as i64;
    let (l0, p0) = (stock(&w, logs), stock(&w, planks));
    bot.run(&mut w, goal, limit);
    Stretch {
        seconds: (w.tick - start_tick) / w.defs.ticks_per_second as u64,
        actions: bot.actions,
        storage_trips: bot.storage_trips,
        player: w.player.time.since(&start_player),
        spirits: spirit_time(&w).since(&start_spirits),
        logs_gained: stock(&w, logs) - l0,
        planks_gained: stock(&w, planks) - p0,
    }
}

/// Sets up the Starting Grove as a training area with a cache and a hauler.
fn enable_area(w: &mut World, bot: &mut Bot, limit: u64) {
    let grove = w.defs.site_id("starting_grove").expect("grove");
    let _ = w.apply(Command::SetTrainingArea(Some(grove)));
    let _ = w.apply(Command::SetTrainingCollect(true));
    if let Some(kind) = w.training_cache_kind() {
        let _ = w.apply(Command::RequestBuilding(kind));
    }
    bot.chop_until(w, limit, |w| w.training_cache().is_some());
}

/// Plays the intended first cycle under a profile and reports how it went.
pub fn play_first_cycle(profile: &str) -> Result<(World, PacingReport), String> {
    play_cycle(profile, true)
}

/// The first cycle, with automatic targeting on or off.
fn play_cycle(profile: &str, auto_gather: bool) -> Result<(World, PacingReport), String> {
    let mut w = World::new_journey(profile)?;
    w.apply(Command::SetAutoGather(auto_gather))?;
    let mut bot = Bot::new(&w);
    let table = w.defs.xp_table.clone();
    let tps = w.defs.ticks_per_second as u64;
    let hour = 3600 * tps;
    let oak = 5;
    let (wc, ww) = (bot.wc, bot.ww);

    // A and B: train until a level 5 spirit leaves something behind, then
    // sacrifice exactly what a level 5 spirit needs.
    let wc_sacrifice = table.xp_for_level(oak);
    if !bot.run(&mut w, Goal::ChopTo(table.xp_for_level(oak + 1)), hour) {
        return Err("never reached the first sacrifice".into());
    }
    w.apply(Command::SoulSeparate {
        skill: wc,
        xp: wc_sacrifice,
    })?;

    // C and D: the same for Woodworking.
    let ww_sacrifice = table.xp_for_level(oak);
    if !bot.run(&mut w, Goal::SawTo(table.xp_for_level(oak + 1)), hour) {
        return Err("never reached the Woodworking sacrifice".into());
    }
    bot.stop(&mut w);
    w.apply(Command::SoulSeparate {
        skill: ww,
        xp: ww_sacrifice,
    })?;

    // E: order the improvement and keep training while the chain pays for it.
    let storehouse = w
        .defs
        .buildings
        .iter()
        .position(|b| b.capacity_bonus > 0)
        .map(|i| crate::defs::BuildingKindId(i as u16))
        .ok_or("no improvement defined")?;
    // The settlement may be building something of its own; one project at a time.
    bot.chop_until(&mut w, hour, |w| w.active_project().is_none());
    w.apply(Command::RequestBuilding(storehouse))?;
    let built = bot.chop_until(&mut w, hour, |w| {
        w.journey
            .as_ref()
            .is_some_and(|j| j.tick_of(Milestone::FirstImprovement).is_some())
    });
    if !built {
        return Err("the improvement was never finished".into());
    }

    // F: retrain from the same moment, with and without the settlement's help.
    bot.stop(&mut w);
    let wc_goal = Goal::ChopTo(w.player.xp(wc) + wc_sacrifice);
    let ww_goal = Goal::SawTo(w.player.xp(ww) + ww_sacrifice);
    let retrain_alone = measure(w.clone(), &bot, wc_goal, hour);
    let resaw_alone = measure(w.clone(), &bot, ww_goal, hour);
    let mut bench = w.clone();
    let _ = bench.apply(Command::SetTrainingSupply(true));
    let resaw_supported = measure(bench, &bot, ww_goal, hour);
    let mut supported = w.clone();
    let mut setup = bot.clone();
    enable_area(&mut supported, &mut setup, hour);
    let retrain_supported = measure(
        supported.clone(),
        &setup,
        Goal::ChopTo(supported.player.xp(wc) + wc_sacrifice),
        hour,
    );

    // Finish the cycle in the world that's returned.
    bot = setup;
    w = supported;
    let finish = Goal::ChopTo(table.xp_for_level(w.player.history(wc).highest_level));
    bot.run(&mut w, finish, hour);

    let journey = w.journey.clone().unwrap_or_default();
    let report = PacingReport {
        profile: w.defs.profile.title.clone(),
        milestones: journey
            .reached
            .iter()
            .map(|(m, t)| (*m, *t / tps))
            .collect(),
        final_phase: journey.phase,
        woodcutting_sacrificed: wc_sacrifice,
        woodworking_sacrificed: ww_sacrifice,
        retrain_alone,
        retrain_supported,
        resaw_alone,
        resaw_supported,
        total_seconds: w.tick / tps,
        player_time: w.player.time,
    };
    Ok((w, report))
}

/// One way of setting up the settlement for retraining.
#[derive(Clone, Debug)]
pub struct RetrainResult {
    pub setup: &'static str,
    pub stretch: Stretch,
}

impl RetrainResult {
    /// XP earned per minute of the stretch, for `xp` earned.
    pub fn xp_per_min(&self, xp: u64) -> u64 {
        xp * 60 / self.stretch.seconds.max(1)
    }
}

fn dedicate(w: &mut World, job: JobKind) {
    let id = w
        .spirits
        .iter()
        .find(|s| w.defs.role(s.role).has_job(job))
        .map(|s| s.id);
    if let Some(spirit) = id {
        let mut policy = w.spirits[spirit].policy;
        policy.focus = Focus::Training;
        let _ = w.apply(Command::SetSpiritPolicy { spirit, policy });
    }
}

fn build_all(w: &mut World, bot: &mut Bot, kind: BuildingKindId, count: u32, limit: u64) {
    for _ in 0..count {
        let built = w.buildings.iter().filter(|b| b.kind == kind).count();
        let _ = w.apply(Command::RequestBuilding(kind));
        bot.chop_until(w, limit, |w| {
            w.active_project().is_none()
                && w.buildings.iter().filter(|b| b.kind == kind).count() > built
        });
    }
}

/// Sawing `xp` of Woodworking back from the same start under four settlements.
pub fn compare_sawing(xp: u64) -> Vec<RetrainResult> {
    let start = World::from_scenario(crate::ScenarioKind::SupportedRetraining);
    let bot = Bot::new(&start);
    let goal = Goal::SawTo(start.player.xp(bot.ww) + xp);
    let limit = 3600 * start.defs.ticks_per_second as u64;
    let mut out = vec![RetrainResult {
        setup: "No assistance",
        stretch: measure(start.clone(), &bot, goal, limit),
    }];

    let mut basic = start.clone();
    let _ = basic.apply(Command::SetTrainingSupply(true));
    out.push(RetrainResult {
        setup: "Hauling support",
        stretch: measure(basic.clone(), &bot, goal, limit),
    });

    let mut dedicated = basic.clone();
    dedicate(&mut dedicated, JobKind::Supply);
    dedicate(&mut dedicated, JobKind::Gather);
    out.push(RetrainResult {
        setup: "Dedicated gatherer and hauler",
        stretch: measure(dedicated.clone(), &bot, goal, limit),
    });

    // Local stock: racks at the bench and a larger standing supply, built up
    // before the session. That's the investment the earlier setups skip.
    let mut stocked = dedicated;
    let rack = stocked.defs.building_kind_id("log_rack").expect("log rack");
    let mut setup = bot.clone();
    build_all(&mut stocked, &mut setup, rack, 2, limit);
    let _ = stocked.apply(Command::SetObjectiveTarget(180));
    let full = stocked.station().map_or(0, |b| {
        stocked.defs.building(b.kind).training_buffer + stocked.settlement.bench_bonus
    });
    setup.chop_until(&mut stocked, limit, |w| {
        w.bench_stock() >= full && w.main().storage.get(w.defs.objective.resource) >= 120
    });
    setup.stop(&mut stocked);
    out.push(RetrainResult {
        setup: "Plus Log Racks and a standing stock",
        stretch: measure(
            stocked.clone(),
            &setup,
            Goal::SawTo(stocked.player.xp(setup.ww) + xp),
            limit,
        ),
    });
    out
}

/// Chopping `xp` of Woodcutting from the same start under three settlements.
pub fn compare_chopping(xp: u64) -> Vec<RetrainResult> {
    // A settlement with work of its own, so its gatherers want the same trees.
    let mut start = World::from_scenario(crate::ScenarioKind::SupportedRetraining);
    let _ = start.apply(Command::SetObjectiveTarget(150));
    let bot = Bot::new(&start);
    let goal = Goal::ChopTo(start.player.xp(bot.wc) + xp);
    let limit = 3600 * start.defs.ticks_per_second as u64;
    let mut out = vec![RetrainResult {
        setup: "No assistance",
        stretch: measure(start.clone(), &bot, goal, limit),
    }];

    let grove = start.defs.site_id("starting_grove").expect("grove");
    let mut reserved = start.clone();
    let _ = reserved.apply(Command::SetTrainingArea(Some(grove)));
    out.push(RetrainResult {
        setup: "Grove reserved",
        stretch: measure(reserved.clone(), &bot, goal, limit),
    });

    let mut cached = reserved;
    let mut setup = bot.clone();
    enable_area(&mut cached, &mut setup, limit);
    setup.stop(&mut cached);
    dedicate(&mut cached, JobKind::Haul);
    out.push(RetrainResult {
        setup: "Plus Training Cache and a dedicated hauler",
        stretch: measure(
            cached.clone(),
            &setup,
            Goal::ChopTo(cached.player.xp(setup.wc) + xp),
            limit,
        ),
    });
    out
}

/// A whole sitting, played through commands alone under normal rules.
#[derive(Clone, Debug)]
pub struct FullSession {
    pub auto_gather: bool,
    pub first_cycle: PacingReport,
    /// Sawing the same XP back each way, from the same moment.
    pub modes: Vec<(TrainingMode, Stretch)>,
    /// Optimized again, after building what its checklist asks for.
    pub invested: Stretch,
    /// Minutes of play it took to build that up.
    pub invest_seconds: u64,
    /// How far Woodcutting got toward the Ironwood level, and that level's XP.
    pub advanced_xp: (u64, u64),
    /// A save and load mid-session changed nothing.
    pub resumed_exactly: bool,
    /// Everything the play log recorded.
    pub report: String,
    pub seconds: u64,
}

/// Runs the structured playtest path: the first cycle, a comparison of the
/// three ways of training, training on toward advanced resources until
/// `minutes` have passed, then a save and resume in the middle of a session.
pub fn play_full_session(
    profile: &str,
    auto_gather: bool,
    minutes: u64,
) -> Result<(World, FullSession), String> {
    let (mut w, first_cycle) = play_cycle(profile, auto_gather)?;
    let mut bot = Bot::new(&w);
    let tps = w.defs.ticks_per_second as u64;
    let hour = 3600 * tps;
    bot.stop(&mut w);

    let xp = first_cycle.woodworking_sacrificed;
    let goal = Goal::SawTo(w.player.xp(bot.ww) + xp);
    let mut modes = Vec::new();
    for mode in TrainingMode::ALL {
        let mut trial = w.clone();
        trial.apply(Command::SetTrainingMode(mode))?;
        modes.push((mode, measure(trial, &bot, goal, hour)));
    }

    // The same again with the investment Optimized asks for: a Log Rack and a
    // standing stock, built while the player keeps chopping.
    let mut built = w.clone();
    let mut setup = bot.clone();
    built.apply(Command::SetTrainingMode(TrainingMode::Optimized))?;
    let rack = built
        .defs
        .building_kind_id("log_rack")
        .ok_or("no log rack")?;
    let started = built.tick;
    setup.chop_until(&mut built, hour, |w| w.active_project().is_none());
    build_all(&mut built, &mut setup, rack, 1, hour);
    built.apply(Command::SetObjectiveTarget(240))?;
    setup.chop_until(&mut built, hour, |w| {
        w.training_plan().checks.iter().all(|(_, done)| *done)
    });
    setup.stop(&mut built);
    let invest_seconds = (built.tick - started) / tps;
    let goal = Goal::SawTo(built.player.xp(setup.ww) + xp);
    let invested = measure(built, &setup, goal, hour);

    // Training on, while the settlement looks after itself.
    let _ = w.apply(Command::SetTrainingMode(TrainingMode::Relaxed));
    bot.heeds_storage_advice = true;
    let remaining = (minutes * 60 * tps).saturating_sub(w.tick);
    bot.chop_until(&mut w, remaining, |_| false);
    let ironwood = w.defs.tree_tiers(bot.wc).last().copied();
    let level = ironwood.map_or(1, |k| w.defs.tree(k).level_required);
    let advanced_xp = (w.player.xp(bot.wc), w.defs.xp_table.xp_for_level(level));

    // Save in the middle of a session and carry on in both worlds.
    bot.chop_until(&mut w, 600 * tps, |w| {
        w.player.session.is_some() && !w.player.actor.queue.is_empty()
    });
    let text = crate::save::encode(&w, "new_game");
    let (mut loaded, _) = crate::save::decode(&text).map_err(|e| e.to_string())?;
    let same_at_load = loaded.state_text() == w.state_text();
    w.run_ticks(600);
    loaded.run_ticks(600);
    let resumed_exactly = same_at_load && loaded.state_text() == w.state_text();

    let session = FullSession {
        auto_gather,
        first_cycle,
        modes,
        invested,
        invest_seconds,
        advanced_xp,
        resumed_exactly,
        report: w.play_report(),
        seconds: w.tick / tps,
    };
    Ok((w, session))
}

/// What happens if a player only ever chops, under a profile.
#[derive(Clone, Copy, Debug)]
pub struct StorageWall {
    /// When the game first warned about room, in seconds.
    pub warned_at: Option<u64>,
    /// When the bag first couldn't be emptied at all.
    pub stuck_at: Option<u64>,
    pub xp_at_stuck: u64,
    /// The XP Ironwood needs.
    pub goal_xp: u64,
    pub reached_goal: bool,
    pub seconds: u64,
}

/// Chops toward the top tree tier spending nothing, to find where storage
/// stops a player who has no reason to use what they gather.
pub fn storage_wall(profile: &str, limit_minutes: u64) -> Result<StorageWall, String> {
    let mut w = World::new_journey(profile)?;
    let mut bot = Bot::new(&w);
    let tps = w.defs.ticks_per_second as u64;
    let top = w.defs.tree_tiers(bot.wc).last().copied();
    let level = top.map_or(1, |k| w.defs.tree(k).level_required);
    let goal_xp = w.defs.xp_table.xp_for_level(level);
    let mut wall = StorageWall {
        warned_at: None,
        stuck_at: None,
        xp_at_stuck: 0,
        goal_xp,
        reached_goal: false,
        seconds: 0,
    };
    for _ in 0..limit_minutes * 60 * tps {
        if w.player.xp(bot.wc) >= goal_xp {
            wall.reached_goal = true;
            break;
        }
        if wall.warned_at.is_none() && w.player.log.storage_warned {
            wall.warned_at = Some(w.tick / tps);
        }
        if w.main().storage.free() == 0 && w.player.actor.inventory.is_full() {
            wall.stuck_at = Some(w.tick / tps);
            wall.xp_at_stuck = w.player.xp(bot.wc);
            break;
        }
        bot.play(&mut w, Goal::Chop);
        w.step();
    }
    wall.seconds = w.tick / tps;
    Ok(wall)
}
