//! Continuous player activities. A session is something the player started
//! on purpose (chopping in a grove, working at a bench) that carries on from
//! one step to the next until something ends it.
//!
//! Every step is an ordinary task, so a session earns XP and moves resources
//! exactly the way clicking would. It adds no rules of its own, only the
//! decision of what to do when the current task finishes, and it only runs
//! while the game does: nothing here happens for a player who isn't there.
//!
//! A session is part of the player's saved state and carries ids rather than
//! a plan, so a loaded game picks up at the same step.

use crate::defs::{RecipeId, ResourceId, SiteId, TreeKindId};
use crate::entities::{BuildingId, TreeId, TreeState};
use crate::events::EventKind;
use crate::grid::TilePos;
use crate::metrics::Regain;
use crate::tasks::{self, Goal, Task};
use crate::world::World;
use serde::{Deserialize, Serialize};

/// How often a waiting session looks again, in ticks.
const WAIT_CHECK_TICKS: u64 = 5;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionKind {
    Gathering {
        /// Where it started. It stays inside this site.
        site: Option<SiteId>,
        /// What it's gathering. It never switches to another material.
        resource: ResourceId,
        /// The kind of tree the player clicked, chosen first when there is one.
        prefer: TreeKindId,
        /// The tree being walked to or cut.
        target: TreeId,
    },
    Crafting {
        building: BuildingId,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub kind: SessionKind,
    pub started: u64,
    /// Set while nothing can be done yet: when the wait began.
    pub waiting_since: Option<u64>,
    /// Logs cut or units made.
    pub units: u32,
    pub xp: u64,
}

/// Why a session ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionEnd {
    /// The player moved, cancelled or started something else.
    Player,
    BagFull,
    /// Automatic targeting is off and the tree is finished.
    TreeFinished,
    /// Nothing in reach, or nothing ready in time.
    NoTargets,
    /// Materials, room, a level or a place is missing.
    Blocked,
}

/// A finished session.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSummary {
    pub what: String,
    pub end: SessionEnd,
    /// In plain words, with what to do about it.
    pub detail: String,
    pub units: u32,
    pub xp: u64,
    pub ticks: u64,
    pub ended: u64,
}

/// What crafting needs that it hasn't got.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CraftBlock {
    Level {
        recipe: RecipeId,
        level: u32,
    },
    Missing {
        recipe: RecipeId,
        resource: ResourceId,
        need: u32,
        have: u32,
    },
    /// Neither the bag nor the bench can take the result.
    NoRoom {
        recipe: RecipeId,
    },
}

/// The next step of a gathering session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NextTree {
    Tree(TreeId),
    /// None is ready, but one will be at this tick.
    WaitUntil(u64),
    /// Nothing ready soon enough. The tick is the soonest, if there is one.
    Nothing(Option<u64>),
}

/// How the player's activity stands, for showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivityState {
    Idle,
    Working,
    Waiting,
    /// Stopped by something the player needs to deal with.
    Blocked,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActivityReport {
    pub state: ActivityState,
    pub title: String,
    /// The reason or next step, when there is one worth saying.
    pub detail: Option<String>,
    /// Numbers for the session under way.
    pub progress: Option<SessionProgress>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SessionProgress {
    pub units: u32,
    pub xp: u64,
    pub seconds: u64,
    pub xp_per_min: u64,
}

/// One recipe as the player would see it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecipeOption {
    pub id: RecipeId,
    pub name: String,
    /// "2 Logs" -> "1 Planks".
    pub inputs: String,
    pub output: String,
    pub level_required: u32,
    pub unlocked: bool,
    pub selected: bool,
    pub xp: u64,
    /// At the player's level, with materials to hand.
    pub xp_per_min: u64,
    pub output_per_min: u64,
    /// What's short right now, with the bag and bench counted.
    pub missing: Option<String>,
}

impl World {
    /// The tree the session is on, or the bench.
    pub fn session_target(&self) -> Option<TilePos> {
        match self.player.session.as_ref()?.kind {
            SessionKind::Gathering { target, .. } => Some(self.trees[target].pos),
            SessionKind::Crafting { building } => Some(self.buildings[building].pos),
        }
    }

    pub(crate) fn begin_gathering(&mut self, tree: TreeId) {
        let t = &self.trees[tree];
        let (site, prefer) = (t.site, t.kind);
        let resource = self.defs.tree(prefer).resource;
        // Another tree in the same place is the same session.
        if let Some(s) = &mut self.player.session
            && let SessionKind::Gathering {
                site: at,
                resource: r,
                prefer: p,
                target,
            } = &mut s.kind
            && *at == site
            && *r == resource
        {
            *p = prefer;
            *target = tree;
            s.waiting_since = None;
            return;
        }
        self.end_session(SessionEnd::Player, String::new());
        self.start_session(SessionKind::Gathering {
            site,
            resource,
            prefer,
            target: tree,
        });
    }

    pub(crate) fn begin_crafting(&mut self, building: BuildingId) {
        if let Some(s) = &mut self.player.session
            && s.kind == (SessionKind::Crafting { building })
        {
            s.waiting_since = None;
            return;
        }
        self.end_session(SessionEnd::Player, String::new());
        self.start_session(SessionKind::Crafting { building });
        // Where you work is where support should come.
        if self.training_bench().is_none_or(|b| b.id != building) {
            self.release_bench_stock();
            self.settlement.training.bench = Some(building);
        }
    }

    fn start_session(&mut self, kind: SessionKind) {
        self.player.log.sessions += 1;
        self.player.session = Some(Session {
            kind,
            started: self.tick,
            waiting_since: None,
            units: 0,
            xp: 0,
        });
    }

    /// What a session is doing, in a few words.
    fn session_title(&self, s: &Session) -> String {
        match s.kind {
            SessionKind::Gathering { prefer, .. } => {
                format!("Chopping {}", self.defs.tree(prefer).name)
            }
            SessionKind::Crafting { building } => {
                format!("Working at the {}", self.buildings[building].name)
            }
        }
    }

    /// Ends the session, if there is one, and records how it went.
    pub(crate) fn end_session(&mut self, end: SessionEnd, detail: String) {
        let Some(s) = self.player.session.take() else {
            return;
        };
        let what = self.session_title(&s);
        let ticks = self.tick - s.started;
        let tps = self.defs.ticks_per_second as u64;
        if s.units > 0 {
            let msg = format!("{what}: {} units, {} XP in {}s", s.units, s.xp, ticks / tps);
            self.events.push_detail(self.tick, EventKind::Player, msg);
        }
        if end != SessionEnd::Player {
            self.events
                .push(self.tick, EventKind::Player, detail.clone());
            self.player.activity = detail.clone();
        }
        self.player.last_session = Some(SessionSummary {
            what,
            end,
            detail,
            units: s.units,
            xp: s.xp,
            ticks,
            ended: self.tick,
        });
    }

    /// Stops what the player is doing where they stand.
    pub(crate) fn player_stop(&mut self) -> String {
        self.end_session(SessionEnd::Player, String::new());
        let actor = &mut self.player.actor;
        actor.queue.clear();
        actor.action_ticks = 0;
        // A step under way finishes, so nobody stops between two tiles.
        actor
            .mover
            .path
            .truncate(usize::from(actor.mover.progress > 0));
        if let Some(&next) = actor.mover.path.front() {
            actor.queue.push_back(Task::Move {
                goal: Goal::Exact(next),
                label: next.to_string(),
            });
        }
        self.player.activity = "Idle".into();
        "Stopped".into()
    }

    pub(crate) fn set_auto_gather(&mut self, on: bool) -> String {
        self.player.auto_gather = on;
        let msg = if on {
            "When one is finished you'll move on to another nearby"
        } else {
            "When one is finished you'll stop and wait for your next click"
        };
        self.events.push(self.tick, EventKind::Player, msg);
        msg.into()
    }

    /// Chooses what the player makes at a bench, or the best they can.
    pub(crate) fn set_player_recipe(&mut self, recipe: Option<RecipeId>) -> Result<String, String> {
        let msg = match recipe {
            Some(r) => {
                let Some(def) = self.defs.recipes.get(r.index()) else {
                    return Err("Unknown recipe".into());
                };
                let level = self.player.level(def.skill, &self.defs.xp_table);
                if level < def.level_required {
                    return Err(format!(
                        "You need {} level {} to make {} (you are level {level})",
                        self.defs.skill(def.skill).name,
                        def.level_required,
                        def.name
                    ));
                }
                format!("You'll make {} until you change it", def.name)
            }
            None => "You'll make the best recipe your materials allow".to_string(),
        };
        if self.player.recipe != recipe {
            // What's set aside for the old recipe is no use to the new one.
            self.release_bench_stock();
            self.player.recipe = recipe;
            // Time spent on a batch of the old recipe doesn't count.
            if matches!(self.player.actor.queue.front(), Some(Task::Craft { .. })) {
                self.player.actor.action_ticks = 0;
            }
        }
        self.events.push(self.tick, EventKind::Player, msg.clone());
        Ok(msg)
    }

    /// Picks the session's next tree: the kind the player clicked if any is
    /// ready, else anything of the same material, nearest first. It looks only
    /// at trees the player may cut, in the same site, within walking reach.
    fn next_tree(
        &self,
        site: Option<SiteId>,
        resource: ResourceId,
        prefer: TreeKindId,
    ) -> NextTree {
        let reach = self.defs.player.gather_reach_tiles;
        let field = self.grid.distance_field(&[self.player.actor.pos()]);
        let mut best = None;
        let mut soonest: Option<u64> = None;
        for t in &self.trees {
            if t.site != site
                || self.defs.tree(t.kind).resource != resource
                || !self.player_can_cut(t.kind)
            {
                continue;
            }
            let Some(walk) = self.grid.approach_distance(&field, t.pos) else {
                continue;
            };
            if walk > reach {
                continue;
            }
            match t.state {
                TreeState::Grown { .. } => {
                    let rank = (t.kind != prefer, t.claimed_by.is_some(), walk, t.id);
                    if best.is_none_or(|b| rank < b) {
                        best = Some(rank);
                    }
                }
                TreeState::Depleted { regrow_at } => {
                    soonest = Some(soonest.map_or(regrow_at, |s| s.min(regrow_at)));
                }
            }
        }
        if let Some((.., id)) = best {
            return NextTree::Tree(id);
        }
        match soonest {
            Some(at) if at <= self.tick + self.defs.player.gather_wait_ticks => {
                NextTree::WaitUntil(at)
            }
            other => NextTree::Nothing(other),
        }
    }

    /// Decides the next step once the current one has finished. Runs after
    /// everything else in a tick, so a level just earned already counts.
    pub(crate) fn update_session(&mut self) {
        let Some(s) = &self.player.session else {
            return;
        };
        if !self.player.actor.queue.is_empty() {
            return;
        }
        if s.waiting_since
            .is_some_and(|since| !(self.tick - since).is_multiple_of(WAIT_CHECK_TICKS))
        {
            return;
        }
        match s.kind {
            SessionKind::Gathering {
                site,
                resource,
                prefer,
                ..
            } => self.continue_gathering(site, resource, prefer),
            SessionKind::Crafting { building } => self.continue_crafting(building),
        }
    }

    fn continue_gathering(
        &mut self,
        site: Option<SiteId>,
        resource: ResourceId,
        prefer: TreeKindId,
    ) {
        let name = self.defs.tree(prefer).name.clone();
        if self.player.actor.inventory.is_full() {
            let detail = format!("Your bag is full. {}", self.drop_off_hint());
            self.end_session(SessionEnd::BagFull, detail);
            return;
        }
        if !self.player.auto_gather {
            self.end_session(
                SessionEnd::TreeFinished,
                format!("The {name} is finished. Click another tree to carry on."),
            );
            return;
        }
        let tps = self.defs.ticks_per_second as u64;
        match self.next_tree(site, resource, prefer) {
            NextTree::Tree(id) => {
                let t = &self.trees[id];
                let label = self.defs.tree(t.kind).name.clone();
                let pos = t.pos;
                let tasks = [
                    Task::Move {
                        goal: Goal::Adjacent(pos),
                        label: label.clone(),
                    },
                    Task::Chop { tree: id },
                ];
                self.player.actor.queue.extend(tasks);
                self.player.activity = format!("Chopping {label}");
                if let Some(s) = &mut self.player.session {
                    s.waiting_since = None;
                    if let SessionKind::Gathering { target, .. } = &mut s.kind {
                        *target = id;
                    }
                }
            }
            NextTree::WaitUntil(at) => {
                let secs = (at - self.tick).div_ceil(tps);
                self.player.activity = format!("Waiting for it to recover ({secs}s)");
                let tick = self.tick;
                if let Some(s) = &mut self.player.session {
                    s.waiting_since.get_or_insert(tick);
                }
            }
            NextTree::Nothing(soonest) => {
                let detail = match soonest {
                    Some(at) => format!(
                        "No {name} will be ready here for {}s. Try another site.",
                        (at - self.tick) / tps
                    ),
                    None => format!("There's nothing else you can cut near this {name}."),
                };
                self.end_session(SessionEnd::NoTargets, detail);
            }
        }
    }

    /// Where the player can empty their bag, for messages.
    fn drop_off_hint(&self) -> String {
        match self.training_cache() {
            Some(c) if c.storage.free() > 0 => {
                format!("Click your {} or Main Storage to unload.", c.name)
            }
            _ => "Click Main Storage to unload.".to_string(),
        }
    }

    fn continue_crafting(&mut self, building: BuildingId) {
        let Some(block) = self.craft_block(building) else {
            self.player.actor.action_ticks = 0;
            self.player.actor.queue.push_back(Task::Craft {
                building,
                output: None,
            });
            self.player.activity = self.making_text(building);
            if let Some(s) = &mut self.player.session {
                s.waiting_since = None;
            }
            return;
        };
        let help = self.support_for(&block);
        let waited = self
            .player
            .session
            .as_ref()
            .and_then(|s| s.waiting_since)
            .map_or(0, |since| self.tick - since);
        match help {
            Some(wait) if waited < self.defs.player.craft_wait_ticks => {
                self.player.activity = wait;
                let tick = self.tick;
                if let Some(s) = &mut self.player.session {
                    s.waiting_since.get_or_insert(tick);
                }
            }
            Some(_) => {
                let secs = waited / self.defs.ticks_per_second as u64;
                let status = self.training_report().supply_status;
                let detail = format!(
                    "{} Nothing arrived in {secs}s. {status}",
                    self.craft_block_text(&block)
                );
                self.end_session(SessionEnd::Blocked, detail);
            }
            None => {
                let detail = self.craft_block_text(&block);
                self.end_session(SessionEnd::Blocked, detail);
            }
        }
    }

    /// What the player would make next at this bench.
    fn making_text(&self, building: BuildingId) -> String {
        let b = &self.buildings[building];
        match self.player_recipe_at(building) {
            Some(r) => format!("Making {} at {}", self.defs.recipe(r).name, b.name),
            None => format!("Working at {}", b.name),
        }
    }

    /// The recipe the player would use next here, if they can use one.
    pub fn player_recipe_at(&self, building: BuildingId) -> Option<crate::defs::RecipeId> {
        let b = &self.buildings[building];
        if let Some(r) = self.player.recipe {
            return (self.defs.recipe(r).station == b.kind).then_some(r);
        }
        let level = self.player_station_level(b);
        let bag = &self.player.actor.inventory;
        self.defs
            .best_recipe(b.kind, level, |r| bag.get(r) + b.storage.get(r))
    }

    /// Why the player can't work at this bench right now. Matches what the
    /// craft task itself checks, so a session never spins on a task that
    /// finishes at once.
    pub fn craft_block(&self, building: BuildingId) -> Option<CraftBlock> {
        let b = &self.buildings[building];
        let recipes = self.defs.station_recipes(b.kind);
        let level = self.player_station_level(b);
        let bag = &self.player.actor.inventory;
        let have = |r: ResourceId| bag.get(r) + b.storage.get(r);
        let recipe = match self.player.recipe.filter(|r| recipes.contains(r)) {
            Some(r) => r,
            None => match self.defs.best_recipe(b.kind, level, have) {
                Some(r) => r,
                None => *recipes.first()?,
            },
        };
        let def = self.defs.recipe(recipe);
        if level < def.level_required {
            return Some(CraftBlock::Level { recipe, level });
        }
        for &(resource, need) in &def.inputs {
            if have(resource) < need {
                return Some(CraftBlock::Missing {
                    recipe,
                    resource,
                    need,
                    have: have(resource),
                });
            }
        }
        let (station_room, bag_room) = tasks::craft_room(self.defs.building(b.kind), b, bag, def);
        if !station_room && !bag_room {
            return Some(CraftBlock::NoRoom { recipe });
        }
        None
    }

    /// A craft block in plain words, with what to do about it.
    pub fn craft_block_text(&self, block: &CraftBlock) -> String {
        match block {
            CraftBlock::Level { recipe, level } => {
                let d = self.defs.recipe(*recipe);
                format!(
                    "You need {} level {} to make {} (you are level {level})",
                    self.defs.skill(d.skill).name,
                    d.level_required,
                    d.name
                )
            }
            CraftBlock::Missing {
                recipe,
                resource,
                need,
                have,
            } => {
                let name = &self.defs.resource(*resource).name;
                let tail = if *have > 0 {
                    format!(" (you have {have})")
                } else {
                    String::new()
                };
                format!(
                    "You need {need} {name} to make {}{tail}: take some from Main Storage",
                    self.defs.recipe(*recipe).name
                )
            }
            CraftBlock::NoRoom { recipe } => {
                let out = &self.defs.resource(self.defs.recipe(*recipe).output.0).name;
                format!(
                    "Your bag and the bench's shelf are both full of {out}. Unload at Main Storage."
                )
            }
        }
    }

    /// When spirits can fix a craft block, what the player is waiting for.
    fn support_for(&self, block: &CraftBlock) -> Option<String> {
        use crate::defs::JobKind;
        match block {
            CraftBlock::Missing { resource, .. }
                if self.settlement.training.supply
                    && *resource == self.training_input()
                    && self.has_worker_for(JobKind::TrainingSupply) =>
            {
                let name = &self.defs.resource(*resource).name;
                Some(format!("Waiting for spirits to bring {name}"))
            }
            CraftBlock::NoRoom { .. } if self.player_output_at_station() => {
                Some("Waiting for a hauler to clear the bench".into())
            }
            _ => None,
        }
    }

    /// Whether clicking the bench can start a session right now: yes if the
    /// player can work, or if spirits are on their way to make that possible.
    pub(crate) fn bench_ready(&self, building: BuildingId) -> Result<String, String> {
        match self.craft_block(building) {
            None => Ok(self.making_text(building)),
            Some(block) => self
                .support_for(&block)
                .ok_or_else(|| self.craft_block_text(&block)),
        }
    }

    /// Every recipe at the player's bench, with what it takes.
    pub fn recipe_options(&self) -> Vec<RecipeOption> {
        let Some(b) = self.training_bench() else {
            return Vec::new();
        };
        let tpm = self.defs.ticks_per_second as u64 * 60;
        let bag = &self.player.actor.inventory;
        self.defs
            .station_recipes(b.kind)
            .into_iter()
            .map(|id| {
                let d = self.defs.recipe(id);
                let level = self.player.level(d.skill, &self.defs.xp_table);
                let ticks = d.craft_ticks_at(level).max(1) as u64;
                let inputs: Vec<String> = d
                    .inputs
                    .iter()
                    .map(|(r, n)| format!("{n} {}", self.defs.resource(*r).name))
                    .collect();
                let missing: Vec<String> = d
                    .inputs
                    .iter()
                    .filter_map(|&(r, n)| {
                        let have = bag.get(r) + b.storage.get(r);
                        (have < n).then(|| format!("{have}/{n} {}", self.defs.resource(r).name))
                    })
                    .collect();
                RecipeOption {
                    id,
                    name: d.name.clone(),
                    inputs: inputs.join(", "),
                    output: format!("{} {}", d.output.1, self.defs.resource(d.output.0).name),
                    level_required: d.level_required,
                    unlocked: level >= d.level_required,
                    selected: self.player.recipe == Some(id),
                    xp: d.xp,
                    xp_per_min: d.xp * tpm / ticks,
                    output_per_min: d.output.1 as u64 * tpm / ticks,
                    missing: (!missing.is_empty()).then(|| missing.join(", ")),
                }
            })
            .collect()
    }

    /// What the player is doing and how it stands.
    pub fn activity_report(&self) -> ActivityReport {
        let p = &self.player;
        let tps = self.defs.ticks_per_second as u64;
        if let Some(s) = &p.session {
            let seconds = (self.tick - s.started) / tps;
            let progress = SessionProgress {
                units: s.units,
                xp: s.xp,
                seconds,
                xp_per_min: s.xp * 60 / seconds.max(1),
            };
            let waiting = p.actor.queue.is_empty();
            let detail = match s.kind {
                SessionKind::Crafting { .. } if waiting => {
                    Some(self.training_report().supply_status)
                }
                SessionKind::Crafting { building } => self.bench_note(building),
                SessionKind::Gathering { .. } => self.gathering_note(s),
            };
            // The words chosen at the click can be out of date by now.
            let title = match s.kind {
                SessionKind::Crafting { building } if !waiting => self.making_text(building),
                _ => p.activity.clone(),
            };
            return ActivityReport {
                state: if waiting {
                    ActivityState::Waiting
                } else {
                    ActivityState::Working
                },
                title,
                detail,
                progress: Some(progress),
            };
        }
        if !p.actor.queue.is_empty() {
            return ActivityReport {
                state: ActivityState::Working,
                title: p.activity.clone(),
                detail: None,
                progress: None,
            };
        }
        if let Some(last) = &p.last_session
            && last.end != SessionEnd::Player
            && last.end != SessionEnd::TreeFinished
            && (last.end != SessionEnd::BagFull || p.actor.inventory.is_full())
            && last.ended + 60 * tps > self.tick
        {
            return ActivityReport {
                state: ActivityState::Blocked,
                title: "Stopped".into(),
                detail: Some(last.detail.clone()),
                progress: None,
            };
        }
        if p.actor.inventory.is_full() {
            return ActivityReport {
                state: ActivityState::Blocked,
                title: "Bag full".into(),
                detail: Some(self.drop_off_hint()),
                progress: None,
            };
        }
        ActivityReport {
            state: ActivityState::Idle,
            title: "Idle".into(),
            detail: None,
            progress: None,
        }
    }

    fn gathering_note(&self, s: &Session) -> Option<String> {
        let free = self.player.actor.inventory.free();
        let target = self.session_target()?;
        let name = self.tree_at(target).map(|t| &self.defs.tree(t.kind).name)?;
        let tail = if self.player.auto_gather {
            "moves on when it's finished"
        } else {
            "stops when it's finished"
        };
        (s.waiting_since.is_none()).then(|| format!("{name} at {target}, {tail}. Bag room: {free}"))
    }

    fn bench_note(&self, building: BuildingId) -> Option<String> {
        let b = &self.buildings[building];
        let slots = self.slots_of(b);
        Some(format!(
            "{} of {slots} places in use",
            self.slots_used(building)
        ))
    }

    /// Records a sacrifice so the time to earn it back can be measured.
    pub(crate) fn track_regain(&mut self, skill: crate::defs::SkillId, level: u32, xp: u64) {
        let started = self.tick;
        self.player.log.regains.push(Regain {
            skill,
            level,
            xp_given: xp,
            started,
            done: None,
        });
    }

    /// Bookkeeping for the play log, once a tick. Reads, never decides.
    pub(crate) fn update_play_log(&mut self) {
        if self.player.session.is_some() {
            let busy = self
                .spirits
                .iter()
                .filter(|s| !s.actor.queue.is_empty())
                .count();
            let log = &mut self.player.log;
            log.training_ticks += 1;
            log.spirit_ticks += self.spirits.len() as u64;
            log.spirit_busy_ticks += busy as u64;
        }
        let table = &self.defs.xp_table;
        let player = &self.player;
        let mut done = Vec::new();
        for (i, r) in player.log.regains.iter().enumerate() {
            if r.done.is_none() && player.level(r.skill, table) >= r.level {
                done.push(i);
            }
        }
        for i in done {
            self.player.log.regains[i].done = Some(self.tick);
        }
    }
}
