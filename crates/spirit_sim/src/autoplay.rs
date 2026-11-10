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

/// What a reference player says to visitors.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Offers {
    /// Lets them come and go without an answer.
    #[default]
    Ignore,
    /// Takes every offer it has the goods for.
    Take,
    /// Turns everyone down.
    Refuse,
}

/// How a reference player goes about things, on top of the route itself.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Style {
    pub offers: Offers,
    /// Puts logs by in autumn, and stops once winter's over.
    pub plans_for_winter: bool,
    /// Keeps the same logs by all year. It's the control for planning: what
    /// that gains might only be from holding more logs.
    pub stocks_all_year: bool,
    /// Plays a world where winter never stops anything growing, to measure
    /// what winter costs. It isn't a way a person can play.
    pub no_winter: bool,
}

/// Looks after what a style cares about. Once a game minute is often enough:
/// visitors wait for ten.
fn tend(w: &mut World, style: Style) {
    if let Some(v) = w.visitor() {
        let answer = match style.offers {
            Offers::Take if v.can_accept => Some(true),
            Offers::Refuse => Some(false),
            _ => None,
        };
        if let Some(yes) = answer {
            let _ = w.apply(Command::AnswerVisitor(yes));
        }
    }
    if style.plans_for_winter || style.stocks_all_year {
        keep_logs_by(w, style.stocks_all_year);
    }
}

/// Asks for a third of the store in logs through autumn and winter, or all
/// year round.
fn keep_logs_by(w: &mut World, all_year: bool) {
    let Some(season) = w.season().map(|s| s.id.clone()) else {
        return;
    };
    let Some(logs) = w.defs.resource_id("logs") else {
        return;
    };
    let wanted = match season.as_str() {
        "autumn" | "winter" => w.main().storage.capacity / 3,
        _ if all_year => w.main().storage.capacity / 3,
        _ => 0,
    };
    let now = w.settlement.standing.get(&logs).copied().unwrap_or(0);
    if now != wanted {
        let _ = w.apply(Command::SetStandingStock {
            resource: logs,
            target: wanted,
        });
    }
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
    /// Knows nothing about storage beyond what the game's message offers.
    pub guided: bool,
    /// What it did because the message said so.
    pub advice_taken: Vec<crate::advice::StorageFix>,
    /// The skill it gathers with, when that isn't Woodcutting.
    pub gathers: Option<SkillId>,
    /// What it does about visitors and winter.
    pub style: Style,
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
            guided: false,
            advice_taken: Vec::new(),
            gathers: None,
            style: Style::default(),
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
            .filter(|t| w.defs.tree(t.kind).skill == self.gathers.unwrap_or(self.wc))
            .filter_map(|t| {
                let walk = w.grid.approach_distance(&field, t.pos)?;
                let xp = w.defs.tree(t.kind).xp_per_log;
                // Crossing the river for a better tree isn't worth it.
                let reach = if self.gathers.is_some() { 40 } else { 16 };
                (walk <= reach).then_some((walk as i64 * 2 - xp as i64 / 15 * 6, t.id))
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
        if self.guided {
            self.take_advice(w);
            return;
        }
        let main = &w.main().storage;
        let target = w.settlement.order.target;
        if main.get(self.logs) >= main.get(w.settlement.order.output) && target < 120 {
            self.click(w, Command::SetProductionTarget(target + 40));
        }
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

    /// The first thing the storage message offers that can be done right now.
    /// Anything that's already been tried stays tried until it helps.
    fn take_advice(&mut self, w: &mut World) {
        use crate::advice::StorageFix as Fix;
        let idle_builder = w.active_project().is_none();
        for fix in w.storage_fixes() {
            let target = w.settlement.order.target;
            let cmd = match fix {
                Fix::RaisePlankTarget if target < 120 => Command::SetProductionTarget(target + 40),
                Fix::LowerPlankTarget if target > 10 => {
                    Command::SetProductionTarget(target.saturating_sub(40).max(10))
                }
                Fix::MakeRecipe(r) => Command::SetPlayerRecipe(Some(r)),
                Fix::PlanRoad(kind) | Fix::PlanRoom(kind) | Fix::SendCaravan(kind)
                    if idle_builder =>
                {
                    Command::QueueBuilding { kind, site: None }
                }
                Fix::LeaveLoad if w.player.actor.inventory.is_full() => Command::LeaveLoad,
                Fix::OrderRoom(kind) if idle_builder => {
                    let main = &w.main().storage;
                    let cost = &w.defs.building(kind).cost;
                    if !cost.iter().all(|(r, n)| main.available(*r) >= *n) {
                        continue;
                    }
                    Command::RequestBuilding(kind)
                }
                _ => continue,
            };
            self.advice_taken.push(fix);
            if self.click(w, cmd) {
                return;
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
        // What matters is room for what it's holding, which needn't be logs.
        let holding = bag.items().keys().next().copied().unwrap_or(self.logs);
        if bag.is_full() && w.player_room_for(holding) == 0 {
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
        if self.heeds_storage_advice {
            self.relieve_storage(w);
        }
        let Some(station) = w.training_bench().map(|b| b.pos) else {
            return;
        };
        let bag = &w.player.actor.inventory;
        let logs = bag.get(self.logs);
        let bench = w.bench_stock();
        let output = w.settlement.order.output;
        let holding = w.main_room_for(output) == 0 && bag.get(output) >= 14;
        if (bag.is_full() || holding) && logs < 2 {
            // A guided player has only the message to go on, and it acts on
            // that in `relieve_storage`.
            if self.guided {
                let main = w.main().pos;
                self.storage_trips += 1;
                self.click(w, Command::PlayerInteract(main));
                return;
            }
            // Planks with nowhere to go are what roads are for, as the storage
            // advice says.
            if w.main_room_for(output) == 0 && w.active_project().is_none() {
                let road = w
                    .plannable()
                    .into_iter()
                    .find(|k| !w.defs.building(*k).paves.is_empty());
                if let Some(kind) = road {
                    self.click(w, Command::QueueBuilding { kind, site: None });
                    return;
                }
            }
            // With Main Storage keeping its room, surplus goes to another store.
            let keep_free = bag.total() + 2;
            let overflow = w
                .outposts()
                .find(|b| b.storage.free() > keep_free)
                .map(|b| b.pos);
            // With nowhere left, spend planks, as the game suggests.
            if overflow.is_none() && w.main_room_for(output) == 0 {
                let rack = w.defs.building_kind_id("log_rack");
                if let Some(kind) = rack.filter(|_| w.active_project().is_none()) {
                    self.click(w, Command::QueueBuilding { kind, site: None });
                }
                return;
            }
            let target = match overflow {
                Some(p) if w.main_room_for(output) == 0 => p,
                _ => w.main().pos,
            };
            self.storage_trips += 1;
            self.click(w, Command::PlayerInteract(target));
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
        if w.tick.is_multiple_of(60 * w.defs.ticks_per_second as u64) {
            tend(w, self.style);
        }
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
            Goal::ChopTo(xp) => w.player.xp(self.gathers.unwrap_or(self.wc)) >= xp,
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
        // A session on a chosen recipe would run until the planks ran out.
        if self.guided {
            let _ = w.apply(Command::PlayerStop);
        }
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
    play_cycle(profile, true, Style::default())
}

/// The first cycle, with automatic targeting on or off.
fn play_cycle(
    profile: &str,
    auto_gather: bool,
    style: Style,
) -> Result<(World, PacingReport), String> {
    let mut w = World::new_journey(profile)?;
    if style.no_winter {
        for season in w.defs.seasons.iter_mut().filter(|s| s.growth == 0) {
            season.growth = 1;
        }
    }
    w.apply(Command::SetAutoGather(auto_gather))?;
    let mut bot = Bot::new(&w);
    bot.style = style;
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
    let (mut w, first_cycle) = play_cycle(profile, auto_gather, Style::default())?;
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

/// One run of a production layout, measured from a scenario's start.
#[derive(Clone, Debug)]
pub struct OutpostRun {
    pub name: String,
    pub seconds: u64,
    /// Beams that reached Main Storage, and when the first did.
    pub beams_home: u32,
    pub first_beam_home: Option<u64>,
    /// When Main Storage reached the order's target.
    pub reached_target: Option<u64>,
    pub beams_made: u64,
    pub hardwood_used: u64,
    pub hardwood_cut: u64,
    /// Tiles the Hauling Spirits walked, and the trips and units they finished.
    pub hauler_tiles: u64,
    pub hauler_trips: u32,
    pub hauler_units: u32,
    pub hauler: TimeUse,
    pub gatherers: TimeUse,
    pub workers: TimeUse,
}

/// Runs a world for `minutes` with nobody touching it.
pub fn run_layout(mut w: World, name: &str, minutes: u64) -> OutpostRun {
    let tps = w.defs.ticks_per_second as u64;
    let beams = w.defs.resource_id("beams").expect("beams");
    let hardwood = w.defs.resource_id("hardwood").expect("hardwood");
    let before = w.main().storage.get(beams);
    let start = w.tick;
    let target = w.order_for(beams).map_or(0, |o| o.target);
    let mut first = None;
    let mut reached = None;
    for _ in 0..minutes * 60 * tps {
        w.step();
        let home = w.main().storage.get(beams);
        if first.is_none() && home > before {
            first = Some((w.tick - start) / tps);
        }
        if reached.is_none() && home >= target {
            reached = Some((w.tick - start) / tps);
        }
    }
    let of = |job: JobKind| {
        let mut t = TimeUse::default();
        for s in w
            .spirits
            .iter()
            .filter(|s| w.defs.role(s.role).has_job(job))
        {
            t.add(&s.time.total);
        }
        t
    };
    let haulers: Vec<_> = w
        .spirits
        .iter()
        .filter(|s| w.defs.role(s.role).has_job(JobKind::Haul))
        .collect();
    let ledger =
        |m: &std::collections::BTreeMap<ResourceId, u64>, r| m.get(&r).copied().unwrap_or(0);
    OutpostRun {
        name: name.to_string(),
        seconds: (w.tick - start) / tps,
        beams_home: w.main().storage.get(beams) - before,
        first_beam_home: first,
        reached_target: reached,
        beams_made: ledger(&w.ledger.crafted, beams),
        hardwood_used: ledger(&w.ledger.processed, hardwood),
        hardwood_cut: ledger(&w.ledger.harvested, hardwood),
        hauler_tiles: haulers
            .iter()
            .map(|s| s.time.total.traveling / s.actor.mover.ticks_per_tile.max(1) as u64)
            .sum(),
        hauler_trips: haulers.iter().map(|s| s.trips).sum(),
        hauler_units: haulers.iter().map(|s| s.units_delivered).sum(),
        hauler: of(JobKind::Haul),
        gatherers: of(JobKind::Gather),
        workers: of(JobKind::Craft),
    }
}

/// Moves a finished building, for comparing placements from the same start.
fn move_building(w: &mut World, id: usize, to: crate::grid::TilePos) {
    let from = w.buildings[id].pos;
    w.grid.set_occupied(from, false);
    w.grid.set_occupied(to, true);
    w.buildings[id].pos = to;
}

/// The two approaches, plus what placement and assignment do to the second.
pub fn compare_layouts(minutes: u64) -> Vec<OutpostRun> {
    use crate::ScenarioKind::{CentralProduction, RemoteProduction};
    let central = World::from_scenario(CentralProduction);
    let remote = World::from_scenario(RemoteProduction);
    let bench = remote.stations().map(|b| b.id).max().expect("bench");
    let mut out = vec![
        run_layout(central, "Central: beams made at home", minutes),
        run_layout(remote.clone(), "Remote: beams made at Old Growth", minutes),
    ];

    // The same bench, placed by the bridge instead of beside the store.
    let mut placed = remote.clone();
    move_building(&mut placed, bench, crate::grid::TilePos::new(24, 3));
    out.push(run_layout(placed, "Remote: bench by the bridge", minutes));

    // The same bench, with the worker free to go wherever it likes.
    let mut free = remote.clone();
    for s in &mut free.spirits {
        s.policy.station = None;
    }
    out.push(run_layout(free, "Remote: worker not assigned", minutes));

    // A busy plank line at home as well, with one hauler or two. A second
    // hauler at the outpost is tied to it; in the central layout there's
    // nothing there to tie it to, so both go wherever they're needed.
    let site = remote.defs.site_id("old_growth").expect("old growth");
    let layouts = [
        (
            World::from_scenario(CentralProduction),
            false,
            "Central, one hauler, planks too",
        ),
        (remote.clone(), false, "Remote, one hauler, planks too"),
        (
            World::from_scenario(CentralProduction),
            true,
            "Central, two haulers, planks too",
        ),
        (remote.clone(), true, "Remote, two haulers, planks too"),
    ];
    for (mut w, second, name) in layouts {
        let logs = w.defs.objective.resource;
        let main = w.main_storage;
        let stocked = w.buildings[main].storage.deposit(logs, 240);
        *w.ledger.seeded.entry(logs).or_default() += stocked as u64;
        w.settlement.order.target = 120;
        let home = w.stations().map(|b| b.id).min().expect("home bench");
        let pos = w.main().pos;
        let at = crate::grid::TilePos::new(pos.x, pos.y + 1);
        let role = w.defs.role_id("woodworker").expect("woodworker");
        let id = w.add_spirit(role, 388, at);
        w.spirits[id].policy.station = Some(home);
        if second {
            let role = w.defs.role_id("hauler").expect("hauler");
            let xp = w.defs.xp_table.xp_for_level(5);
            let id = w.add_spirit(role, xp, at);
            if name.starts_with("Remote") {
                w.spirits[id].policy.site = Some(site);
            }
        }
        out.push(run_layout(w, name, minutes));
    }

    // Half the gatherers, to show what staffing does.
    let mut short = remote;
    let gatherer = short.gatherer_ids()[1];
    short.spirits[gatherer].policy.focus = Focus::Rest;
    out.push(run_layout(short, "Remote: one gatherer", minutes));
    out
}

/// What it took to set up an outpost from nothing.
#[derive(Clone, Debug, Default)]
pub struct Establishment {
    /// Seconds from the start to each building finishing.
    pub store_built: u64,
    pub workshop_built: u64,
    /// Beams the settlement made so it could build the Workshop.
    pub beams_for_building: u64,
    /// Everything construction used up.
    pub consumed: Vec<(String, u64)>,
    /// Spirit time on tasks, in percent, while building and afterwards.
    pub busy_building_pct: u64,
    pub busy_running_pct: u64,
    pub beams_home_running: u32,
    pub seconds_running: u64,
    /// Commands a person would have given.
    pub commands: u32,
}

fn busy_ticks(w: &World) -> (u64, u64) {
    let mut t = TimeUse::default();
    for s in &w.spirits {
        t.add(&s.time.total);
    }
    let busy = t.gathering + t.traveling + t.handling + t.building + t.crafting;
    (busy, t.total())
}

/// Sets an outpost up through commands alone and leaves it to run.
pub fn establish_outpost(run_minutes: u64) -> Result<(World, Establishment), String> {
    let mut w = World::from_scenario(crate::ScenarioKind::OutpostEstablishment);
    let tps = w.defs.ticks_per_second as u64;
    let limit = 3600 * tps;
    let site = w.defs.site_id("old_growth").ok_or("no Old Growth")?;
    let store = w
        .defs
        .building_kind_id("outpost_storage")
        .ok_or("no store")?;
    let workshop = w.defs.building_kind_id("workshop").ok_or("no workshop")?;
    let beams = w.defs.resource_id("beams").ok_or("no beams")?;
    let mut e = Establishment::default();
    let finished = |w: &World| w.active_project().is_none();
    let wait = |w: &mut World| {
        for _ in 0..limit {
            if finished(w) {
                return true;
            }
            w.step();
        }
        false
    };

    w.apply(Command::RequestBuildingAt {
        kind: store,
        site: crate::grid::TilePos::new(34, 4),
    })?;
    e.commands += 1;
    if !wait(&mut w) {
        return Err("the store was never finished".into());
    }
    e.store_built = w.tick / tps;

    w.apply(Command::RequestBuildingAt {
        kind: workshop,
        site: crate::grid::TilePos::new(36, 4),
    })?;
    e.commands += 1;
    if !wait(&mut w) {
        return Err("the workshop was never finished".into());
    }
    e.workshop_built = w.tick / tps;
    e.beams_for_building = w.ledger.crafted.get(&beams).copied().unwrap_or(0);
    let (busy, total) = busy_ticks(&w);
    e.busy_building_pct = busy * 100 / total.max(1);
    e.consumed = w
        .ledger
        .consumed
        .iter()
        .map(|(r, n)| (w.defs.resource(*r).name.clone(), *n))
        .collect();

    w.apply(Command::SetOutpostStaffed { site, on: true })?;
    e.commands += 1;
    let home = w.main().storage.get(beams);
    w.apply(Command::SetOrderTarget {
        output: beams,
        target: home + 30,
    })?;
    e.commands += 1;
    w.run_ticks(run_minutes * 60 * tps);
    let (busy2, total2) = busy_ticks(&w);
    e.busy_running_pct = (busy2 - busy) * 100 / (total2 - total).max(1);
    e.beams_home_running = w.main().storage.get(beams).saturating_sub(home);
    e.seconds_running = run_minutes * 60;
    Ok((w, e))
}

/// A layout's steady pace, after the first stock of grown trees has gone.
#[derive(Clone, Debug)]
pub struct SteadyRun {
    pub name: String,
    pub hardwood_per_min: f32,
    pub beams_per_min: f32,
    /// Shares of gatherers' time.
    pub gathering_pct: u64,
    pub waiting_pct: u64,
    pub hauler_tiles_per_min: f32,
    pub hauler_walking_pct: u64,
    /// Share of the bench workers' time at a bench.
    pub bench_pct: u64,
    pub hardwood_used: u64,
}

fn hardwood_cut(w: &World) -> u64 {
    let r = w.defs.resource_id("hardwood").expect("hardwood");
    w.ledger.harvested.get(&r).copied().unwrap_or(0)
}

/// Removes every limit but the trees, the workers and the walking: a target
/// nothing can reach, and room for what's made.
fn open_ended(w: &mut World) {
    let beams = w.defs.resource_id("beams").expect("beams");
    let main = w.main_storage;
    w.buildings[main].storage.capacity += 5000;
    if let Some(o) = w.order_for_mut(beams) {
        o.target = 5000;
    }
}

/// Measures a world's pace over `minutes`, after `warm_up` minutes.
pub fn steady_rate(mut w: World, name: &str, warm_up: u64, minutes: u64) -> SteadyRun {
    open_ended(&mut w);
    let tps = w.defs.ticks_per_second as u64;
    let beams = w.defs.resource_id("beams").expect("beams");
    let hardwood = w.defs.resource_id("hardwood").expect("hardwood");
    w.run_ticks(warm_up * 60 * tps);
    let time_of = |w: &World, job: JobKind| {
        let mut t = TimeUse::default();
        for s in w
            .spirits
            .iter()
            .filter(|s| w.defs.role(s.role).has_job(job))
        {
            t.add(&s.time.total);
        }
        t
    };
    let used = |w: &World| w.ledger.processed.get(&hardwood).copied().unwrap_or(0);
    let (cut0, home0, used0) = (hardwood_cut(&w), w.main().storage.get(beams), used(&w));
    let (g0, h0, b0) = (
        time_of(&w, JobKind::Gather),
        time_of(&w, JobKind::Haul),
        time_of(&w, JobKind::Craft),
    );
    w.run_ticks(minutes * 60 * tps);
    let g = time_of(&w, JobKind::Gather).since(&g0);
    let h = time_of(&w, JobKind::Haul).since(&h0);
    let b = time_of(&w, JobKind::Craft).since(&b0);
    let mins = minutes as f32;
    let per_tile = w
        .spirits
        .iter()
        .find(|s| w.defs.role(s.role).has_job(JobKind::Haul))
        .map_or(1, |s| s.actor.mover.ticks_per_tile.max(1)) as u64;
    SteadyRun {
        name: name.to_string(),
        hardwood_per_min: (hardwood_cut(&w) - cut0) as f32 / mins,
        beams_per_min: (w.main().storage.get(beams) - home0) as f32 / mins,
        gathering_pct: g.gathering * 100 / g.total().max(1),
        waiting_pct: g.waiting_recovery * 100 / g.total().max(1),
        hauler_tiles_per_min: (h.traveling / per_tile) as f32 / mins,
        hauler_walking_pct: h.traveling * 100 / h.total().max(1),
        bench_pct: b.crafting * 100 / b.total().max(1),
        hardwood_used: used(&w) - used0,
    }
}

fn add_gatherer(w: &mut World) {
    let site = w.defs.site_id("old_growth").expect("old growth");
    let role = w.defs.role_id("woodcutter").expect("woodcutter");
    let xp = w.defs.xp_table.xp_for_level(10);
    let pos = w.main().pos;
    let id = w.add_spirit(role, xp, crate::grid::TilePos::new(pos.x, pos.y + 1));
    w.spirits[id].policy.site = Some(site);
    w.spirits[id].policy.prefer_advanced = true;
}

fn add_hauler(w: &mut World) {
    let site = w.defs.site_id("old_growth").expect("old growth");
    let role = w.defs.role_id("hauler").expect("hauler");
    let xp = w.defs.xp_table.xp_for_level(5);
    let pos = w.main().pos;
    let id = w.add_spirit(role, xp, crate::grid::TilePos::new(pos.x, pos.y + 1));
    w.spirits[id].policy.site = Some(site);
}

/// The four settlements, and what a worker does instead of more trees.
pub fn compare_expansion(warm_up: u64, minutes: u64) -> Vec<SteadyRun> {
    use crate::ScenarioKind::{
        CentralProduction, ExpandedCentral, ExpandedRemote, RemoteProduction,
    };
    let run = |w: World, name: &str| steady_rate(w, name, warm_up, minutes);
    let mut out = vec![
        run(
            World::from_scenario(CentralProduction),
            "A. Original, central",
        ),
        run(
            World::from_scenario(RemoteProduction),
            "A. Original, remote",
        ),
        run(
            World::from_scenario(ExpandedCentral),
            "B. Expanded, central",
        ),
        run(World::from_scenario(ExpandedRemote), "C. Expanded, remote"),
    ];
    let mut dedicated = World::from_scenario(ExpandedRemote);
    add_hauler(&mut dedicated);
    out.push(run(dedicated, "D. Expanded, remote, posted hauler"));
    let mut worker = World::from_scenario(RemoteProduction);
    add_gatherer(&mut worker);
    out.push(run(worker, "Original, remote, third gatherer"));
    let mut both = World::from_scenario(ExpandedRemote);
    add_gatherer(&mut both);
    add_hauler(&mut both);
    out.push(run(both, "Expanded, remote, third gatherer, posted hauler"));
    out
}

/// A long stretch of Woodworking after the normal route, by one habit.
#[derive(Clone, Debug)]
pub struct LongTraining {
    pub name: String,
    pub minutes: u64,
    pub xp: u64,
    pub panels_made: u32,
    pub logs_and_planks_in_main: u32,
    pub main_used: u32,
    pub main_capacity: u32,
    pub storage_trips: u32,
    pub full_bag_seconds: u64,
    pub waiting_seconds: u64,
}

/// Picks up where the normal route ends and trains Woodworking by hand. With
/// `panels`, spare planks are made into panels once that's unlocked.
pub fn train_on(panels: bool, minutes: u64) -> Result<LongTraining, String> {
    let (mut w, _) = play_to_expansion("normal")?;
    let mut bot = Bot::new(&w);
    bot.heeds_storage_advice = true;
    let tps = w.defs.ticks_per_second as u64;
    let plank = w.defs.resource_id("planks").ok_or("no planks")?;
    let made = w.defs.resource_id("panels").ok_or("no panels")?;
    let recipe = w
        .defs
        .recipes
        .iter()
        .position(|r| r.id == "fitted_panels")
        .map(|i| crate::defs::RecipeId(i as u16))
        .ok_or("no recipe")?;
    let main_bench = w.stations().next().map(|b| b.id);
    w.apply(Command::SetTrainingBench(main_bench))?;
    w.apply(Command::SetTrainingMode(TrainingMode::Relaxed))?;
    // Sawing by hand needs logs coming in.
    w.apply(Command::SetObjectiveTarget(150))?;
    let start = w.player.xp(bot.ww);
    let log = w.player.time;
    for _ in 0..minutes {
        let open = w
            .recipe_options()
            .iter()
            .any(|o| o.id == recipe && o.unlocked);
        let spare = w.main().storage.available(plank) >= 30;
        let want = (panels && open && spare).then_some(recipe);
        // Panels are for spending, as the storage advice says.
        let buy = w.plannable().into_iter().find(|k| {
            let def = w.defs.building(*k);
            let priced = def.cost.iter().any(|c| c.0 == made);
            let afford = def
                .cost
                .iter()
                .all(|c| w.main().storage.available(c.0) >= c.1);
            priced && afford
        });
        if let Some(kind) = buy.filter(|_| w.active_project().is_none()) {
            w.apply(Command::QueueBuilding { kind, site: None })?;
        }
        if w.player.recipe != want {
            bot.stop(&mut w);
            w.apply(Command::SetPlayerRecipe(want))?;
        }
        bot.run(&mut w, Goal::SawTo(u64::MAX), 60 * tps);
    }
    w.check_conservation()?;
    let main = &w.main().storage;
    let spent = |now: u64, then: u64| (now - then) / tps;
    Ok(LongTraining {
        name: if panels {
            "Panels when there's plenty"
        } else {
            "Planks only"
        }
        .to_string(),
        minutes,
        xp: w.player.xp(bot.ww) - start,
        panels_made: *w.ledger.crafted.get(&made).unwrap_or(&0) as u32,
        logs_and_planks_in_main: main.get(plank) + main.get(bot.logs),
        main_used: main.total(),
        main_capacity: main.capacity,
        storage_trips: bot.storage_trips,
        full_bag_seconds: spent(w.player.time.blocked_storage, log.blocked_storage),
        waiting_seconds: spent(w.player.time.waiting_input, log.waiting_input),
    })
}

/// Beside Main Storage, and a spot in the Distant Forest across the river.
pub const QUARRY_HOME: crate::grid::TilePos = crate::grid::TilePos::new(6, 15);
pub const QUARRY_ACROSS: crate::grid::TilePos = crate::grid::TilePos::new(30, 21);

/// Seconds for the player to walk from home into the Distant Forest.
pub fn walk_across(causeway: bool) -> f64 {
    let mut w = World::from_scenario(crate::ScenarioKind::QuarryStart);
    if causeway {
        w.grid.open_crossing(crate::grid::TilePos::new(21, 20));
        w.grid.open_crossing(crate::grid::TilePos::new(22, 20));
    }
    w.player.actor.mover.pos = QUARRY_HOME;
    w.apply(Command::PlayerInteract(QUARRY_ACROSS))
        .expect("a walkable tile");
    let start = w.tick;
    while w.player.actor.pos() != QUARRY_ACROSS && w.tick < start + 6000 {
        w.step();
    }
    (w.tick - start) as f64 / w.defs.ticks_per_second as f64
}

/// How long the causeway takes, in seconds: from the plan to finished, with
/// the player out cutting stone too, or leaving it all to the spirit.
pub fn stone_time(alone: bool) -> Option<u64> {
    let mut w = World::from_scenario(crate::ScenarioKind::QuarryStart);
    let kind = w.defs.building_kind_id("stone_causeway")?;
    w.apply(Command::QueueBuilding { kind, site: None }).ok()?;
    let outcrop = crate::grid::TilePos::new(4, 22);
    let stone = w.defs.resource_id("stone")?;
    let tps = w.defs.ticks_per_second as u64;
    for _ in 0..3600 * tps {
        if w.buildings
            .iter()
            .any(|b| b.kind == kind && b.is_complete())
        {
            return Some(w.tick / tps);
        }
        let idle = w.player.actor.queue.is_empty() && w.player.session.is_none();
        if !alone && idle {
            // Cut a bagful, carry it home, and go back.
            let target = if w.player.actor.inventory.get(stone) > 0 {
                w.main().pos
            } else {
                outcrop
            };
            let _ = w.apply(Command::PlayerInteract(target));
        }
        w.step();
    }
    None
}

/// When each long-term goal was met in one long game, and how it ended.
#[derive(Clone, Debug, Default)]
pub struct AllGoals {
    /// Each goal met, in order, with the second it was noticed.
    pub met: Vec<(String, u64)>,
    pub missed: Vec<String>,
    pub seconds: u64,
    pub final_state: String,
    /// Everything the game said for the first time, and when.
    pub firsts: Vec<(u64, String)>,
}

/// Plans whatever can be paid for that a goal still needs.
fn buy_for_goals(w: &mut World) {
    if w.active_project().is_some() || !w.settlement.queue.is_empty() {
        return;
    }
    let wanted: Vec<String> = w
        .defs
        .ambitions
        .iter()
        .flat_map(|a| a.built.iter().cloned())
        .collect();
    for id in wanted {
        let Some(kind) = w.defs.building_kind_id(&id) else {
            continue;
        };
        // Room is worth buying again once the store is getting full.
        let more_room = w.defs.building(kind).capacity_bonus > 0
            && w.main().storage.free() < w.main().storage.capacity / 4;
        let built = w
            .buildings
            .iter()
            .any(|b| b.kind == kind && b.is_complete());
        if (built && !more_room) || !w.plannable().contains(&kind) {
            continue;
        }
        // A plan creates its own demand, so it needn't be affordable yet.
        let _ = w.apply(Command::QueueBuilding { kind, site: None });
        return;
    }
}

/// Sends a caravan if a goal needs something one hasn't found yet. Only worth
/// doing late on: finds early in the game outrun the skills they need.
fn send_for_finds(w: &mut World) {
    if w.active_project().is_some() || !w.settlement.queue.is_empty() {
        return;
    }
    let wanted_find = w
        .defs
        .ambitions
        .iter()
        .flat_map(|a| &a.built)
        .filter_map(|id| w.defs.building_kind_id(id))
        .map(|k| w.defs.building(k).needs_caravans)
        .max();
    if wanted_find > Some(w.settlement.caravans) {
        let caravan = w
            .plannable()
            .into_iter()
            .find(|k| w.defs.building(*k).repeatable);
        if let Some(kind) = caravan {
            let _ = w.apply(Command::QueueBuilding { kind, site: None });
        }
    }
}

/// A trade learned by hand: carry the raw stuff to the bench until it's
/// second nature, then hand that to a spirit.
struct HandTraining {
    skill: &'static str,
    raw: &'static str,
    made: &'static str,
    batch: u32,
    level: u32,
}

/// Works at a bench by hand up to a level, then gives that to a spirit. A
/// person would do this with clicks on the source and the bench.
fn train_by_hand(
    w: &mut World,
    end: u64,
    trade: HandTraining,
    style: Style,
    note: &mut dyn FnMut(&World),
) -> Result<(), String> {
    let skill = w.defs.skill_id(trade.skill).ok_or("no such skill")?;
    let raw = w.defs.resource_id(trade.raw).ok_or("no raw material")?;
    let made = w.defs.resource_id(trade.made).ok_or("no product")?;
    let need = w.defs.xp_table.xp_for_level(trade.level);
    let sources: Vec<_> = w
        .trees
        .iter()
        .filter(|t| w.defs.tree(t.kind).resource == raw)
        .map(|t| t.id)
        .collect();
    let bench = w.stations_for(made).first().map(|b| b.pos).ok_or_else(|| {
        let stuck = stuck_report(w);
        let caravans = w.settlement.caravans;
        format!(
            "no bench for {} after {caravans} caravans
{stuck}",
            trade.made
        )
    })?;
    let tps = w.defs.ticks_per_second as u64;
    while w.player.xp(skill) < need && w.tick < end {
        let idle = w.player.actor.queue.is_empty() && w.player.session.is_none();
        if idle {
            let bag = &w.player.actor.inventory;
            let target = if bag.get(raw) >= trade.batch {
                bench
            } else if bag.total() > 0 {
                w.main().pos
            } else {
                let grown = sources.iter().find(|t| w.trees[**t].is_grown());
                grown.map_or(w.main().pos, |t| w.trees[*t].pos)
            };
            let _ = w.apply(Command::PlayerInteract(target));
        }
        w.run_ticks(tps);
        if w.tick.is_multiple_of(60 * tps) {
            tend(w, style);
            note(w);
        }
    }
    let _ = w.apply(Command::PlayerStop);
    if w.player.xp(skill) >= need {
        w.apply(Command::SoulSeparate { skill, xp: need })?;
    }
    Ok(())
}

/// Brings home enough Stillwood for a Still Lantern, by hand, as it must be.
fn cut_stillwood(w: &mut World, end: u64) {
    let Some(wood) = w.defs.resource_id("stillwood") else {
        return;
    };
    let trees: Vec<_> = w
        .trees
        .iter()
        .filter(|t| w.defs.tree(t.kind).resource == wood)
        .map(|t| t.id)
        .collect();
    let tps = w.defs.ticks_per_second as u64;
    while w.main().storage.get(wood) < 30 && w.tick < end {
        let idle = w.player.actor.queue.is_empty() && w.player.session.is_none();
        if idle {
            let bag = &w.player.actor.inventory;
            let grown = trees.iter().find(|t| w.trees[**t].is_grown());
            let target = match grown {
                Some(t) if !bag.is_full() => w.trees[*t].pos,
                _ => w.main().pos,
            };
            let _ = w.apply(Command::PlayerInteract(target));
        }
        w.run_ticks(tps);
    }
    let _ = w.apply(Command::PlayerStop);
}

/// The guided game, carried on until every long-term goal is met or `hours`
/// run out. It cuts stone, makes panels when there's plenty of planks, buys
/// what the goals name, and trains both wood skills to the top.
pub fn play_all_goals(hours: u64) -> Result<(World, AllGoals), String> {
    play_all_goals_styled(hours, Style::default())
}

/// The same long game, played a particular way.
pub fn play_all_goals_styled(hours: u64, style: Style) -> Result<(World, AllGoals), String> {
    let (mut w, _) = play_route("normal", true, style)?;
    let mut bot = Bot::new(&w);
    bot.heeds_storage_advice = true;
    bot.guided = true;
    bot.style = style;
    let tps = w.defs.ticks_per_second as u64;
    let end = w.tick + hours * 3600 * tps;
    let table = w.defs.xp_table.clone();
    let top = table.xp_for_level(20);
    let quarrying = w.defs.skill_id("quarrying").ok_or("no quarrying")?;
    let plank = w.defs.resource_id("planks").ok_or("no planks")?;
    let recipe = w
        .defs
        .recipes
        .iter()
        .position(|r| r.id == "fitted_panels")
        .map(|i| crate::defs::RecipeId(i as u16))
        .ok_or("no recipe")?;
    let mut out = AllGoals::default();
    let note = |w: &World, out: &mut AllGoals| {
        for a in w.ambitions().into_iter().filter(|a| a.done) {
            if !out.met.iter().any(|m| m.0 == a.title) {
                out.met.push((a.title, w.tick / tps));
            }
        }
        for n in w.notices.iter() {
            if !out.firsts.iter().any(|f| f.1 == n.text) {
                out.firsts.push((n.tick / tps, n.text.clone()));
            }
        }
    };
    note(&w, &mut out);

    // Stone, by hand, up to the level the goal names.
    bot.stop(&mut w);
    w.apply(Command::SetPlayerRecipe(None))?;
    bot.gathers = Some(quarrying);
    let enough = table.xp_for_level(5);
    while w.player.xp(quarrying) < enough && w.tick < end {
        bot.run(&mut w, Goal::ChopTo(enough), 60 * tps);
        buy_for_goals(&mut w);
        note(&w, &mut out);
    }
    bot.stop(&mut w);
    bot.gathers = None;
    let bench = w.stations().next().map(|b| b.id);
    w.apply(Command::SetTrainingBench(bench))?;
    w.apply(Command::SetTrainingMode(TrainingMode::Relaxed))?;
    w.apply(Command::SetObjectiveTarget(150))?;

    // Woodworking to the top, on panels whenever there are planks to spare.
    while w.player.xp(bot.ww) < top && w.tick < end {
        let open = w
            .recipe_options()
            .iter()
            .any(|o| o.id == recipe && o.unlocked);
        let spare = w.main().storage.available(plank) >= 30;
        let want = (open && spare).then_some(recipe);
        if w.player.recipe != want {
            bot.stop(&mut w);
            w.apply(Command::SetPlayerRecipe(want))?;
        }
        bot.run(&mut w, Goal::SawTo(top), 60 * tps);
        buy_for_goals(&mut w);
        note(&w, &mut out);
    }
    bot.stop(&mut w);
    w.apply(Command::SetPlayerRecipe(None))?;

    // A spirit for the quarry, then the yard once caravans have opened it.
    w.apply(Command::SoulSeparate {
        skill: quarrying,
        xp: enough,
    })
    .map_err(|e| {
        format!(
            "{e}
{}",
            stuck_report(&w)
        )
    })?;
    let yard = w.defs.building_kind_id("mason_yard").ok_or("no yard")?;
    let ready = |w: &World| {
        w.buildings
            .iter()
            .any(|b| b.kind == yard && b.is_complete())
    };
    while !ready(&w) && w.tick < end {
        bot.chop_until(&mut w, 60 * tps, ready);
        buy_for_goals(&mut w);
        note(&w, &mut out);
    }
    bot.stop(&mut w);
    let masonry = HandTraining {
        skill: "masonry",
        raw: "stone",
        made: "dressed_stone",
        batch: 2,
        level: 8,
    };
    train_by_hand(&mut w, end, masonry, style, &mut |w| note(w, &mut out))?;
    let keys = w.defs.resource_id("keystones").ok_or("no keystones")?;
    w.apply(Command::SetOrderTarget {
        output: keys,
        target: 10,
    })?;

    // And Woodcutting.
    while w.player.xp(bot.wc) < top && w.tick < end {
        bot.run(&mut w, Goal::ChopTo(top), 60 * tps);
        buy_for_goals(&mut w);
        note(&w, &mut out);
    }
    // Across the south river, for what only the player can bring back.
    let crossing = w
        .defs
        .building_kind_id("south_crossing")
        .ok_or("no crossing")?;
    let open = |w: &World| {
        w.buildings
            .iter()
            .any(|b| b.kind == crossing && b.is_complete())
    };
    while !open(&w) && w.tick < end {
        bot.chop_until(&mut w, 60 * tps, open);
        buy_for_goals(&mut w);
        note(&w, &mut out);
    }
    bot.stop(&mut w);
    cut_stillwood(&mut w, end);
    buy_for_goals(&mut w);
    note(&w, &mut out);

    // A walk round the old stones.
    bot.stop(&mut w);
    for [x, y] in w.defs.markers.iter().map(|m| m.at).collect::<Vec<_>>() {
        let beside = crate::grid::TilePos::new(x, y + 1);
        let _ = w.apply(Command::PlayerInteract(beside));
        for _ in 0..120 * tps {
            if w.player.actor.pos() == beside {
                break;
            }
            w.step();
        }
        w.run_ticks(10);
    }
    note(&w, &mut out);

    // The kiln, once caravans have found the clay.
    let kiln = w.defs.building_kind_id("kiln").ok_or("no kiln")?;
    let lit = |w: &World| {
        w.buildings
            .iter()
            .any(|b| b.kind == kiln && b.is_complete())
    };
    while !lit(&w) && w.tick < end {
        bot.chop_until(&mut w, 60 * tps, lit);
        buy_for_goals(&mut w);
        send_for_finds(&mut w);
        note(&w, &mut out);
    }
    bot.stop(&mut w);
    let kilnwork = HandTraining {
        skill: "kilnwork",
        raw: "logs",
        made: "charcoal",
        batch: 3,
        level: 5,
    };
    train_by_hand(&mut w, end, kilnwork, style, &mut |w| note(w, &mut out))?;
    let bricks = w.defs.resource_id("bricks").ok_or("no bricks")?;
    w.apply(Command::SetOrderTarget {
        output: bricks,
        target: 30,
    })?;

    // Whatever's left, while the spirits get on with it.
    while w.next_ambition().is_some() && w.tick < end {
        bot.run(&mut w, Goal::Chop, 60 * tps);
        buy_for_goals(&mut w);
        note(&w, &mut out);
    }
    note(&w, &mut out);
    out.missed = w
        .ambitions()
        .into_iter()
        .filter(|a| !a.done)
        .map(|a| a.title)
        .collect();
    out.seconds = w.tick / tps;
    out.final_state = format!(
        "Woodworking XP {}, Woodcutting XP {}
{}",
        w.player.xp(bot.ww),
        w.player.xp(bot.wc),
        stuck_report(&w)
    );
    Ok((w, out))
}

/// A settlement with and without the nursery's trees, in steady running.
pub fn compare_nursery(warm_up: u64, minutes: u64) -> Vec<SteadyRun> {
    use crate::ScenarioKind::ExpandedCentral;
    // Spirits posted to Old Growth would never see trees near home.
    let unposted = || {
        let mut w = World::from_scenario(ExpandedCentral);
        for s in &mut w.spirits {
            s.policy.site = None;
        }
        w
    };
    let mut planted = unposted();
    let kind = planted
        .defs
        .building_kind_id("ironwood_nursery")
        .expect("nursery");
    for (tree, [x, y]) in planted.defs.building(kind).plants.clone() {
        let at = crate::grid::TilePos::new(x, y);
        let id = planted.trees.len();
        planted
            .trees
            .push(crate::scenario::new_tree(&planted.defs, tree, at, id));
        planted.grid.set_occupied(at, true);
    }
    vec![
        steady_rate(unposted(), "Expanded, central", warm_up, minutes),
        steady_rate(planted, "With the nursery", warm_up, minutes),
    ]
}

/// Every road on the map, laid at once, for measuring what they're worth.
pub fn lay_roads(w: &mut World) {
    let pct = w.defs.construction.road_pct;
    let tiles: Vec<[i32; 2]> = w
        .defs
        .buildings
        .iter()
        .flat_map(|b| b.paves.iter().copied())
        .collect();
    for [x, y] in tiles {
        w.grid.pave(crate::grid::TilePos::new(x, y), pct);
    }
}

/// Seconds for the player to walk between two places.
pub fn walk_seconds(roads: bool, from: crate::grid::TilePos, to: crate::grid::TilePos) -> f64 {
    let mut w = World::from_scenario(crate::ScenarioKind::ExpandedCentral);
    if roads {
        lay_roads(&mut w);
    }
    w.player.actor.mover.pos = from;
    w.apply(Command::PlayerInteract(to))
        .expect("a walkable tile");
    let start = w.tick;
    while w.player.actor.pos() != to && w.tick < start + 6000 {
        w.step();
    }
    (w.tick - start) as f64 / w.defs.ticks_per_second as f64
}

/// The same settlements with and without roads.
pub fn compare_roads(warm_up: u64, minutes: u64) -> Vec<SteadyRun> {
    use crate::ScenarioKind::{CentralProduction, ExpandedCentral, ExpandedRemote};
    let mut out = Vec::new();
    for (kind, name) in [
        (CentralProduction, "Original, central"),
        (ExpandedCentral, "Expanded, central"),
        (ExpandedRemote, "Expanded, remote"),
    ] {
        out.push(steady_rate(
            World::from_scenario(kind),
            name,
            warm_up,
            minutes,
        ));
        let mut w = World::from_scenario(kind);
        lay_roads(&mut w);
        out.push(steady_rate(
            w,
            &format!("{name}, with roads"),
            warm_up,
            minutes,
        ));
    }
    out
}

/// What a shared hauler does with and without job scoring.
#[derive(Clone, Debug)]
pub struct HaulingRun {
    pub name: String,
    pub tiles: u64,
    pub trips: u32,
    pub units: u32,
    pub walking_pct: u64,
    pub first_beam_home: Option<u64>,
    pub reached_target: Option<u64>,
}

pub fn compare_hauling(minutes: u64) -> Vec<HaulingRun> {
    use crate::ScenarioKind::SharedHauler;
    let run = |scored: bool, name: &str| {
        let mut w = World::from_scenario(SharedHauler);
        w.defs.logistics.scored_carrying = scored;
        let r = run_layout(w, name, minutes);
        HaulingRun {
            name: name.to_string(),
            tiles: r.hauler_tiles,
            trips: r.hauler_trips,
            units: r.hauler_units,
            walking_pct: r.hauler.traveling * 100 / r.hauler.total().max(1),
            first_beam_home: r.first_beam_home,
            reached_target: r.reached_target,
        }
    };
    vec![
        run(false, "Fixed job order (v0.8)"),
        run(true, "Scored jobs (v0.9)"),
    ]
}

/// When building the crossing starts to pay.
#[derive(Clone, Copy, Debug)]
pub struct Payback {
    /// Seconds until the crossing opened.
    pub built_at: Option<u64>,
    /// Seconds until total beams home caught up with not building it.
    pub caught_up_at: Option<u64>,
    /// Beams home after the whole run, without and with it.
    pub without: u32,
    pub with: u32,
}

/// Runs the same outpost with and without building the crossing from the same
/// moment, counting the beams it uses as beams not delivered.
pub fn crossing_payback(minutes: u64) -> Payback {
    use crate::ScenarioKind::RemoteProduction;
    let setup = || {
        let mut w = World::from_scenario(RemoteProduction);
        open_ended(&mut w);
        let main = w.main_storage;
        for (id, n) in [("logs", 60), ("planks", 40)] {
            let r = w.defs.resource_id(id).expect("material");
            w.buildings[main].storage.deposit(r, n);
            *w.ledger.seeded.entry(r).or_default() += n as u64;
        }
        w
    };
    let mut without = setup();
    let mut with = setup();
    let kind = with
        .defs
        .building_kind_id("forest_crossing")
        .expect("crossing");
    with.apply(Command::QueueBuilding { kind, site: None })
        .expect("the outpost store is standing");
    let beams = with.defs.resource_id("beams").expect("beams");
    let tps = with.defs.ticks_per_second as u64;
    let delivered = |w: &World| {
        w.main().storage.get(beams) as u64 + w.ledger.consumed.get(&beams).copied().unwrap_or(0)
    };
    let mut payback = Payback {
        built_at: None,
        caught_up_at: None,
        without: 0,
        with: 0,
    };
    for _ in 0..minutes * 60 * tps {
        without.step();
        with.step();
        if payback.built_at.is_none()
            && with
                .buildings
                .iter()
                .any(|b| b.kind == kind && b.is_complete())
        {
            payback.built_at = Some(with.tick / tps);
        }
        // What's spent on the crossing doesn't count as delivered.
        let spent = with.ledger.consumed.get(&beams).copied().unwrap_or(0) * 2;
        let ahead = delivered(&with).saturating_sub(spent) >= delivered(&without);
        if payback.built_at.is_some() {
            if !ahead {
                payback.caught_up_at = None;
            } else if payback.caught_up_at.is_none() {
                payback.caught_up_at = Some(with.tick / tps);
            }
        }
    }
    payback.without = without.main().storage.get(beams);
    payback.with = with.main().storage.get(beams);
    payback
}

/// The road from a new game to a working expansion under a profile.
#[derive(Clone, Debug, Default)]
pub struct ExpansionRoute {
    /// Each step and when it finished, in seconds.
    pub steps: Vec<(String, u64)>,
    /// Beams home over the observation window, and its length in minutes.
    pub beams_after: u32,
    pub observed_minutes: u64,
    pub hardwood_after: u64,
    pub report: String,
    pub consumed: Vec<(String, u64)>,
    pub seconds: u64,
    /// Where everything stood when the run ended.
    pub final_state: String,
    /// Roads laid along the way.
    pub roads: Vec<String>,
    /// Things done because the storage message offered them.
    pub advice_taken: u32,
}

/// What everything is doing, for when a reference run can't finish.
fn stuck_report(w: &World) -> String {
    let mut out = format!(
        "tick {} main {:?} / {}",
        w.tick,
        w.main().storage.contents(),
        w.main().storage.capacity
    );
    out += &format!(
        "\n  player at {} doing {:?} bag {:?} queue {} session {} fixes {:?}",
        w.player.actor.pos(),
        w.player.activity,
        w.player.actor.inventory.items(),
        w.player.actor.queue.len(),
        w.player.session.is_some(),
        w.storage_fixes(),
    );
    for v in w.plan_views() {
        out += &format!("\n  plan {} {:?} {:?}", v.name, v.stage, v.reason);
        for n in v.needs {
            out += &format!("\n    {} {}/{} {}", n.name, n.have, n.need, n.status);
        }
    }
    for s in &w.spirits {
        out += &format!(
            "\n  {} [{:?}] {}",
            s.name,
            w.worker_state(s),
            s.activity.text()
        );
    }
    for s in &w.spirits {
        out += &format!(
            "\n  at {} {} bag {:?} policy {:?} wanted {:?}",
            s.name,
            s.actor.pos(),
            s.actor.inventory.items(),
            s.policy,
            w.input_wanted(s.id)
        );
    }
    for i in 0..w.defs.resources.len() {
        let r = ResourceId(i as u16);
        out += &format!(
            "\n  res {} avail {} spare {} reserved {}",
            i,
            w.main().storage.available(r),
            w.any_spare(r),
            w.main().storage.reserved(r)
        );
    }
    for b in &w.buildings {
        out += &format!(
            "
  bld {} {} {:?} {:?}",
            b.name,
            b.pos,
            b.state,
            b.storage.contents()
        );
        let held: Vec<_> = (0..w.defs.resources.len())
            .map(|i| ResourceId(i as u16))
            .filter(|r| b.storage.reserved(*r) > 0)
            .map(|r| (r, b.storage.reserved(r)))
            .collect();
        if !held.is_empty() {
            out += &format!(" reserved {held:?}");
        }
    }
    for s in &w.spirits {
        out += &format!("\n  pile {} {:?}", s.name, w.output_pile(s.id));
    }
    for o in w.order_reports() {
        out += &format!(
            "\n  order {} {} / {}: {}",
            o.name, o.in_main, o.goal, o.status
        );
    }
    out
}

/// Plays from a new game to the crossing and watches the result. Nothing is
/// granted: every level is earned, and every building is paid for.
pub fn play_to_expansion(profile: &str) -> Result<(World, ExpansionRoute), String> {
    play_route(profile, false, Style::default())
}

/// The same route for a player who knows only what the game tells them: no
/// stashing planks elsewhere and no fewer logs kept, unless a message says so.
pub fn play_guided(profile: &str) -> Result<(World, ExpansionRoute), String> {
    play_route(profile, true, Style::default())
}

fn play_route(
    profile: &str,
    guided: bool,
    style: Style,
) -> Result<(World, ExpansionRoute), String> {
    let (mut w, cycle) = play_cycle(profile, true, style)?;
    let mut route = ExpansionRoute::default();
    let mut bot = Bot::new(&w);
    bot.heeds_storage_advice = true;
    bot.guided = guided;
    bot.style = style;
    let tps = w.defs.ticks_per_second as u64;
    let long = 6 * 3600 * tps;
    for (m, t) in &cycle.milestones {
        route.steps.push((m.label().to_string(), *t));
    }
    // The top tier a spirit can work, which is what the route is after.
    let top = w
        .defs
        .tree_tiers(bot.wc)
        .into_iter()
        .rfind(|k| !w.defs.tree(*k).spirits_avoid)
        .ok_or("no trees")?;
    let level = w.defs.tree(top).level_required;
    let need = w.defs.xp_table.xp_for_level(level);
    let mark = |route: &mut ExpansionRoute, w: &World, name: &str| {
        route.steps.push((name.to_string(), w.tick / tps));
    };

    // Advanced Woodcutting, and a spirit that keeps it.
    if !guided {
        w.apply(Command::SetObjectiveTarget(150))?;
    }
    if !bot.run(&mut w, Goal::ChopTo(need), long) {
        return Err("never reached the top Woodcutting tier".into());
    }
    mark(&mut route, &w, "Woodcutting level 10 reached");
    bot.stop(&mut w);
    w.apply(Command::SoulSeparate {
        skill: bot.wc,
        xp: need,
    })?;
    mark(&mut route, &w, "Level 10 Woodcutting Spirit created");

    // The same for Woodworking, or beams can't be automated. That makes far
    // more planks than the settlement needs, so a store goes up first, and
    // fewer logs are kept while they're being sawn.
    let kind = |w: &World, id: &str| w.defs.building_kind_id(id).ok_or("unknown building");
    let store = kind(&w, "outpost_storage")?;
    let at = crate::grid::TilePos::new;
    bot.chop_until(&mut w, long, |w| w.active_project().is_none());
    if !guided {
        w.apply(Command::SetObjectiveTarget(30))?;
    }
    w.apply(Command::QueueBuilding {
        kind: store,
        site: Some(at(34, 4)),
    })?;
    bot.chop_until(&mut w, long, |w| w.active_project().is_none());
    mark(&mut route, &w, "Outpost Storage finished");
    w.apply(Command::SetTrainingMode(TrainingMode::Relaxed))?;
    if !bot.run(&mut w, Goal::SawTo(need), long) {
        return Err(format!(
            "never reached the top Woodworking tier ({} XP, bag {:?}, activity {}): {}",
            w.player.xp(bot.ww),
            w.player.actor.inventory.items(),
            w.player.activity,
            stuck_report(&w)
        ));
    }
    mark(&mut route, &w, "Woodworking level 10 reached");
    bot.stop(&mut w);
    w.apply(Command::SoulSeparate {
        skill: bot.ww,
        xp: need,
    })?;
    mark(&mut route, &w, "Level 10 Woodworking Spirit created");

    // One plan, then back to chopping.
    let workshop = kind(&w, "workshop")?;
    let crossing = kind(&w, "forest_crossing")?;
    bot.chop_until(&mut w, long, |w| w.active_project().is_none());
    w.apply(Command::QueueBuilding {
        kind: workshop,
        site: Some(at(36, 4)),
    })?;
    w.apply(Command::QueueBuilding {
        kind: crossing,
        site: None,
    })?;
    mark(&mut route, &w, "Outpost and crossing planned");
    let site = w.defs.site_id("old_growth").ok_or("no Old Growth")?;
    let bench = |w: &World| w.outpost_report(site).workshop.is_some();
    if !bot.chop_until(&mut w, long, bench) {
        return Err(format!(
            "the outpost workshop was never finished: {}",
            stuck_report(&w)
        ));
    }
    mark(&mut route, &w, "Outpost Workshop finished");
    w.apply(Command::SetOutpostStaffed { site, on: true })?;
    let open = |w: &World| {
        w.buildings
            .iter()
            .any(|b| b.kind == crossing && b.is_complete())
    };
    if !bot.chop_until(&mut w, long, open) {
        return Err(format!(
            "the crossing was never finished: {}",
            stuck_report(&w)
        ));
    }
    mark(&mut route, &w, "Forest Crossing finished");

    // Main Storage is mostly surplus planks by now, so spend some the way the
    // game suggests: an Annex at the outpost bench.
    let beams = w.defs.resource_id("beams").ok_or("no beams")?;
    let annex = kind(&w, "workshop_annex")?;
    let bench_id = w.outpost_report(site).workshop;
    w.apply(Command::SetTrainingBench(bench_id))?;
    w.apply(Command::QueueBuilding {
        kind: annex,
        site: None,
    })?;
    let built = |w: &World| {
        w.buildings
            .iter()
            .any(|b| b.kind == annex && b.is_complete())
    };
    if !bot.chop_until(&mut w, long, built) {
        return Err(format!(
            "the annex was never finished: {}",
            stuck_report(&w)
        ));
    }
    mark(&mut route, &w, "Workshop Annex finished");

    // The rest of the surplus goes into the roads still to be laid.
    let left: Vec<_> = (0..w.defs.buildings.len())
        .map(|i| crate::defs::BuildingKindId(i as u16))
        .filter(|k| {
            !w.defs.building(*k).paves.is_empty() && !w.buildings.iter().any(|b| b.kind == *k)
        })
        .collect();
    for kind in left {
        w.apply(Command::QueueBuilding { kind, site: None })?;
    }
    let laid = |w: &World| w.active_project().is_none() && w.settlement.queue.is_empty();
    if !bot.chop_until(&mut w, long, laid) {
        return Err(format!("the roads were never laid: {}", stuck_report(&w)));
    }
    mark(&mut route, &w, "Roads laid");

    // Leave it alone and see what it does.
    bot.stop(&mut w);
    let target = w.main().storage.get(beams) + 200;
    w.apply(Command::SetOrderTarget {
        output: beams,
        target,
    })?;
    let (home, cut) = (w.main().storage.get(beams), hardwood_cut(&w));
    route.observed_minutes = 15;
    w.run_ticks(route.observed_minutes * 60 * tps);
    route.beams_after = w.main().storage.get(beams).saturating_sub(home);
    route.final_state = stuck_report(&w);
    route.hardwood_after = hardwood_cut(&w) - cut;
    route.report = w.play_report();
    route.consumed = w
        .ledger
        .consumed
        .iter()
        .map(|(r, n)| (w.defs.resource(*r).name.clone(), *n))
        .collect();
    route.roads = w
        .buildings
        .iter()
        .filter(|b| b.is_complete() && !w.defs.building(b.kind).paves.is_empty())
        .map(|b| b.name.clone())
        .collect();
    route.seconds = w.tick / tps;
    route.advice_taken = bot.advice_taken.len() as u32;
    Ok((w, route))
}
