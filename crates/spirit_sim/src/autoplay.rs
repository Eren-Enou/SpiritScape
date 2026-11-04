//! A reference player: plays a real game through commands alone, the way a
//! steady person would, so the whole progression loop can be measured and
//! tested without a window.
//!
//! It has no special access. It reads the world, waits a reaction delay after
//! each action finishes, and issues the same commands a click would. It plays
//! efficiently and never gets distracted, so its times are a lower bound on a
//! person's, not a prediction of them.

use crate::defs::{ResourceId, SkillId};
use crate::entities::TreeId;
use crate::journey::{Milestone, Phase};
use crate::metrics::TimeUse;
use crate::tasks::Task;
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

    fn chop_step(&mut self, w: &mut World) {
        let bag = &w.player.actor.inventory;
        let spot = self.nearest_tree(w);
        let empty_handed = bag.total() == 0;
        if bag.is_full() || spot.is_none() && !empty_handed {
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
        if logs >= 2 || bench >= 2 {
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
        if !w.player.actor.queue.is_empty() || w.tick < self.next_action {
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
        w.player.actor.clear_tasks();
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
    let mut w = World::new_journey(profile)?;
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
