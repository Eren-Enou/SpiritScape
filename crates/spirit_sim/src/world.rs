//! The simulation state and its fixed-step update.

use crate::defs::{GameDefs, JobKind, ResourceId, RoleId, SkillId};
use crate::entities::{
    Activity, Actor, Building, BuildingId, Player, Project, Spirit, SpiritId, Tree, TreeState,
};
use crate::events::{EventKind, EventLog};
use crate::grid::{Grid, TilePos};
use crate::planner::PlannerState;
use crate::tasks::{self, Goal, SimEvent, StepResult, Task, TaskCtx, Who};
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};

const SPIRIT_NAMES: &[&str] = &[
    "Oakroot",
    "Mossbell",
    "Ashwhisper",
    "Fernlight",
    "Birchsong",
    "Emberleaf",
    "Thistledown",
    "Willowmere",
];

/// Running totals used to verify conservation of resources and XP.
#[derive(Clone, Debug, Default)]
pub struct Ledger {
    /// Resources placed by the scenario at start.
    pub seeded: BTreeMap<ResourceId, u64>,
    /// Resources harvested from trees.
    pub harvested: BTreeMap<ResourceId, u64>,
    /// Resources permanently consumed by construction.
    pub consumed: BTreeMap<ResourceId, u64>,
    /// XP the player earned through work.
    pub xp_earned: u64,
    /// XP the player transferred into spirits.
    pub xp_separated: u64,
}

#[derive(Clone, Debug)]
pub struct Objective {
    pub resource: ResourceId,
    pub target: u32,
}

#[derive(Clone, Debug)]
pub struct Settlement {
    pub objective: Objective,
    pub allow_autonomous_construction: bool,
    pub planner: PlannerState,
}

/// Player-issued commands. Applied between ticks so runs can be replayed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// Click on a tile: walk there, chop a tree, or deposit into a storage.
    PlayerInteract(TilePos),
    /// Keyboard movement: step one tile in a direction (dx, dy each -1..=1).
    /// Repeating it while a step is in progress queues the following tile,
    /// so a held key walks smoothly.
    PlayerStep {
        dx: i32,
        dy: i32,
    },
    SoulSeparate {
        skill: SkillId,
        xp: u64,
    },
    SetObjectiveTarget(u32),
    SetAutonomousConstruction(bool),
}

#[derive(Clone, Debug)]
pub struct World {
    pub defs: GameDefs,
    pub tick: u64,
    pub grid: Grid,
    pub trees: Vec<Tree>,
    pub buildings: Vec<Building>,
    pub spirits: Vec<Spirit>,
    pub player: Player,
    pub projects: Vec<Project>,
    pub settlement: Settlement,
    pub events: EventLog,
    pub ledger: Ledger,
    pub main_storage: BuildingId,
}

impl World {
    // ------------------------------------------------------------------
    // Construction helpers (used by scenarios)
    // ------------------------------------------------------------------

    pub(crate) fn add_spirit(&mut self, role: RoleId, xp: u64, pos: TilePos) -> SpiritId {
        let id = self.spirits.len();
        let role_def = self.defs.role(role);
        let name = match SPIRIT_NAMES.get(id) {
            Some(n) => n.to_string(),
            None => format!("Spirit {}", id + 1),
        };
        let level = self.defs.xp_table.level_for_xp(xp);
        let actor = Actor::new(pos, role_def.move_ticks, role_def.capacity_at(level));
        let spirit = Spirit::new(
            id,
            name,
            role,
            role_def.skill,
            xp,
            &self.defs.xp_table,
            actor,
        );
        self.spirits.push(spirit);
        id
    }

    // ------------------------------------------------------------------
    // Queries
    // ------------------------------------------------------------------

    pub fn main(&self) -> &Building {
        &self.buildings[self.main_storage]
    }

    pub fn tree_at(&self, p: TilePos) -> Option<&Tree> {
        self.trees.iter().find(|t| t.pos == p)
    }

    pub fn building_at(&self, p: TilePos) -> Option<&Building> {
        self.buildings.iter().find(|b| b.pos == p)
    }

    pub fn spirit_level_name(&self, s: &Spirit) -> String {
        format!("{} lvl {}", self.defs.skill(s.skill()).name, s.level())
    }

    pub fn active_project(&self) -> Option<&Project> {
        self.projects.iter().find(|p| p.is_active())
    }

    /// Stock counted toward the objective: unreserved units in main storage
    /// plus units already on their way there (in outposts or carried by
    /// gathering/hauling spirits). Construction materials are excluded.
    pub fn projected_stock(&self, r: ResourceId) -> u32 {
        let main = self.main().storage.available(r);
        let outposts: u32 = self
            .buildings
            .iter()
            .filter(|b| b.id != self.main_storage && b.is_complete())
            .map(|b| b.storage.get(r))
            .sum();
        let carried: u32 = self
            .spirits
            .iter()
            .filter(|s| s.job != Some(JobKind::Construct))
            .map(|s| s.actor.inventory.get(r))
            .sum();
        main + outposts + carried
    }

    pub fn objective_unmet(&self) -> bool {
        let o = &self.settlement.objective;
        self.projected_stock(o.resource) < o.target
    }

    /// Total units of `r` physically present anywhere (storages, inventories, sites).
    pub fn resource_in_world(&self, r: ResourceId) -> u64 {
        let storages: u64 = self.buildings.iter().map(|b| b.storage.get(r) as u64).sum();
        let carried: u64 = self
            .spirits
            .iter()
            .map(|s| s.actor.inventory.get(r) as u64)
            .sum::<u64>()
            + self.player.actor.inventory.get(r) as u64;
        let sites: u64 = self
            .projects
            .iter()
            .map(|p| p.delivered.get(&r).copied().unwrap_or(0) as u64)
            .sum();
        storages + carried + sites
    }

    /// Checks seeded + harvested == present + consumed for every resource.
    pub fn check_conservation(&self) -> Result<(), String> {
        for i in 0..self.defs.resources.len() {
            let r = ResourceId(i as u16);
            let get = |m: &BTreeMap<ResourceId, u64>| m.get(&r).copied().unwrap_or(0);
            let created = get(&self.ledger.seeded) + get(&self.ledger.harvested);
            let accounted = self.resource_in_world(r) + get(&self.ledger.consumed);
            if created != accounted {
                return Err(format!(
                    "{}: seeded+harvested={created} but present+consumed={accounted}",
                    self.defs.resource(r).name
                ));
            }
        }
        Ok(())
    }

    /// Hash of the simulation state, used to verify deterministic replays.
    pub fn state_hash(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.tick.hash(&mut h);
        for t in &self.trees {
            (t.pos, t.logs_left(), t.claimed_by).hash(&mut h);
        }
        for b in &self.buildings {
            (b.pos, b.is_complete(), b.storage.contents().clone()).hash(&mut h);
        }
        let mut actor = |a: &Actor| {
            (
                a.pos(),
                a.mover.progress,
                a.inventory.items().clone(),
                a.queue.len(),
            )
                .hash(&mut h)
        };
        actor(&self.player.actor);
        for s in &self.spirits {
            actor(&s.actor);
        }
        for p in &self.projects {
            (p.site, p.state as u8, p.build_progress).hash(&mut h);
        }
        self.events.total.hash(&mut h);
        h.finish()
    }

    // ------------------------------------------------------------------
    // Commands
    // ------------------------------------------------------------------

    pub fn apply(&mut self, cmd: Command) -> Result<String, String> {
        match cmd {
            Command::PlayerInteract(p) => self.player_interact(p),
            Command::PlayerStep { dx, dy } => self.player_step(dx, dy),
            Command::SoulSeparate { skill, xp } => self
                .soul_separate(skill, xp)
                .map(|id| format!("Created {}", self.spirits[id].name)),
            Command::SetObjectiveTarget(target) => {
                let o = &mut self.settlement.objective;
                o.target = target;
                let msg = format!(
                    "Objective changed: maintain {target} {} in Main Storage",
                    self.defs.resource(o.resource).name
                );
                self.events.push(self.tick, EventKind::Player, msg.clone());
                Ok(msg)
            }
            Command::SetAutonomousConstruction(allow) => {
                self.settlement.allow_autonomous_construction = allow;
                let msg = if allow {
                    "Autonomous construction permitted"
                } else {
                    "Autonomous construction forbidden"
                };
                self.events.push(self.tick, EventKind::Player, msg);
                Ok(msg.into())
            }
        }
    }

    fn player_step(&mut self, dx: i32, dy: i32) -> Result<String, String> {
        let actor = &mut self.player.actor;
        // Mid-step, keep walking into the tile we're entering and plan from there.
        let mid_step = actor.mover.progress > 0;
        let from = match actor.mover.path.front() {
            Some(&next) if mid_step => next,
            _ => actor.pos(),
        };
        let target = TilePos::new(from.x + dx.signum(), from.y + dy.signum());
        let goal = Goal::Exact(target);
        if matches!(actor.queue.front(), Some(Task::Move { goal: g, .. }) if *g == goal) {
            return Ok("Walking".into());
        }
        if !self.grid.is_walkable(target) {
            return Err("Blocked".into());
        }
        actor.queue.clear();
        actor.action_ticks = 0;
        // Pre-fill the route so there is no pause at tile boundaries.
        actor.mover.path.truncate(if mid_step { 1 } else { 0 });
        actor.mover.path.push_back(target);
        actor.queue.push_back(Task::Move {
            goal,
            label: target.to_string(),
        });
        self.player.activity = "Walking".into();
        Ok("Walking".into())
    }

    fn player_interact(&mut self, p: TilePos) -> Result<String, String> {
        if !self.grid.in_bounds(p) {
            return Err("Outside the map".into());
        }
        let (tasks, activity) = if let Some(tree) = self.tree_at(p) {
            let name = &self.defs.tree(tree.kind).name;
            if !tree.is_grown() {
                return Err(format!("The {name} is depleted"));
            }
            (
                vec![
                    Task::Move {
                        goal: Goal::Adjacent(p),
                        label: name.clone(),
                    },
                    Task::Chop { tree: tree.id },
                ],
                format!("Chopping {name}"),
            )
        } else if let Some(b) = self.building_at(p) {
            if !b.is_complete() {
                return Err(format!("{} is still under construction", b.name));
            }
            (
                vec![
                    Task::Move {
                        goal: Goal::Adjacent(p),
                        label: b.name.clone(),
                    },
                    Task::Deposit { building: b.id },
                ],
                format!("Depositing at {}", b.name),
            )
        } else if self.grid.is_walkable(p) {
            (
                vec![Task::Move {
                    goal: Goal::Exact(p),
                    label: p.to_string(),
                }],
                format!("Walking to {p}"),
            )
        } else {
            return Err("You can't walk there".into());
        };
        // Repeated clicks on the same target keep the current order going.
        if self.player.actor.redirect(tasks) {
            self.player.activity = activity.clone();
        }
        Ok(activity)
    }

    /// Soul Separation: moves `xp` from the player's skill into a new spirit.
    pub fn soul_separate(&mut self, skill: SkillId, xp: u64) -> Result<SpiritId, String> {
        let skill_def = self.defs.skill(skill).clone();
        let Some(role) = self.defs.soul_role_for(skill) else {
            return Err(format!("{} cannot form a spirit yet", skill_def.name));
        };
        if xp < self.defs.soul_min_xp {
            return Err(format!(
                "At least {} XP is required to form a spirit",
                self.defs.soul_min_xp
            ));
        }
        let table = &self.defs.xp_table;
        let before_level = self.player.level(skill, table);
        if !self.player.take_xp(skill, xp) {
            return Err(format!(
                "Not enough {} XP ({} available)",
                skill_def.name,
                self.player.xp(skill)
            ));
        }
        let after_level = self.player.level(skill, &self.defs.xp_table);
        self.ledger.xp_separated += xp;
        let pos = self.player.actor.pos();
        let id = self.add_spirit(role, xp, pos);
        let s = &self.spirits[id];
        let msg = format!(
            "Soul Separation: sacrificed {xp} {} XP (level {before_level} -> {after_level}). {} awakens as a {} at level {}.",
            skill_def.name,
            s.name,
            self.defs.role(role).name,
            s.level()
        );
        self.events.push(self.tick, EventKind::Player, msg);
        Ok(id)
    }

    // ------------------------------------------------------------------
    // Simulation step
    // ------------------------------------------------------------------

    pub fn step(&mut self) {
        self.tick += 1;
        self.regrow_trees();
        self.update_projects();
        if self
            .tick
            .is_multiple_of(self.defs.planner.eval_interval_ticks)
        {
            self.evaluate_logistics();
        }

        let mut events = Vec::new();
        self.step_player(&mut events);
        self.step_spirits(&mut events);
        for e in events {
            self.handle_event(e);
        }
    }

    pub fn run_ticks(&mut self, n: u64) {
        for _ in 0..n {
            self.step();
        }
    }

    fn regrow_trees(&mut self) {
        for t in &mut self.trees {
            if let TreeState::Depleted { regrow_at } = t.state
                && self.tick >= regrow_at
            {
                t.state = TreeState::Grown {
                    logs: self.defs.trees[t.kind.index()].logs,
                };
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn task_ctx<'a>(
        defs: &'a GameDefs,
        grid: &'a Grid,
        trees: &'a mut [Tree],
        buildings: &'a mut [Building],
        projects: &'a mut [Project],
        ledger: &'a mut Ledger,
        tick: u64,
        events: &'a mut Vec<SimEvent>,
    ) -> TaskCtx<'a> {
        TaskCtx {
            defs,
            grid,
            trees,
            buildings,
            projects,
            ledger,
            tick,
            events,
        }
    }

    fn step_player(&mut self, events: &mut Vec<SimEvent>) {
        let level = self.player_chop_level();
        let World {
            defs,
            grid,
            trees,
            buildings,
            projects,
            ledger,
            tick,
            player,
            ..
        } = self;
        if player.actor.queue.is_empty() {
            return;
        }
        let mut ctx = Self::task_ctx(
            defs, grid, trees, buildings, projects, ledger, *tick, events,
        );
        match tasks::step(&mut player.actor, Who::Player, level, None, &mut ctx) {
            StepResult::Continue => {}
            StepResult::Done => {
                player.actor.queue.pop_front();
                if player.actor.queue.is_empty() {
                    player.activity = if player.actor.inventory.is_full() {
                        "Inventory full - click a storage to deposit".into()
                    } else {
                        "Idle".into()
                    };
                }
            }
            StepResult::Failed(reason) => {
                player.actor.clear_tasks();
                player.activity = reason;
            }
        }
    }

    /// The player's level in whatever skill their current chop target uses.
    fn player_chop_level(&self) -> u32 {
        let skill = match self.player.actor.queue.front() {
            Some(Task::Chop { tree }) => self.defs.tree(self.trees[*tree].kind).skill,
            _ => return 1,
        };
        self.player.level(skill, &self.defs.xp_table)
    }

    fn step_spirits(&mut self, events: &mut Vec<SimEvent>) {
        let o = &self.settlement.objective;
        let mut budget = o.target as i64 - self.projected_stock(o.resource) as i64;
        for sid in 0..self.spirits.len() {
            if self.spirits[sid].actor.queue.is_empty() && self.tick >= self.spirits[sid].next_think
            {
                self.think(sid);
            }
            let World {
                defs,
                grid,
                trees,
                buildings,
                projects,
                ledger,
                tick,
                spirits,
                ..
            } = self;
            let spirit = &mut spirits[sid];
            if spirit.actor.queue.is_empty() {
                continue;
            }
            let level = spirit.level();
            let mut ctx = Self::task_ctx(
                defs, grid, trees, buildings, projects, ledger, *tick, events,
            );
            match tasks::step(
                &mut spirit.actor,
                Who::Spirit(sid),
                level,
                Some(&mut budget),
                &mut ctx,
            ) {
                StepResult::Continue => {}
                StepResult::Done => {
                    spirit.actor.queue.pop_front();
                }
                StepResult::Failed(reason) => {
                    spirit.actor.clear_tasks();
                    spirit.next_think = *tick + defs.idle_rethink_ticks;
                    let msg = format!("{}: task failed - {reason}", spirit.name);
                    let key = format!("fail:{reason}");
                    spirit.activity = Activity::Blocked(reason);
                    self.log_spirit(sid, key, msg);
                }
            }
        }
    }

    fn handle_event(&mut self, e: SimEvent) {
        match e {
            SimEvent::Chopped {
                who: Who::Player,
                tree,
                xp,
                ..
            } => {
                let skill = self.defs.tree(self.trees[tree].kind).skill;
                let table = &self.defs.xp_table;
                let before = self.player.level(skill, table);
                self.player.add_xp(skill, xp);
                self.ledger.xp_earned += xp;
                let after = self.player.level(skill, &self.defs.xp_table);
                if after > before {
                    let msg = format!(
                        "{} level up! You are now level {after}.",
                        self.defs.skill(skill).name
                    );
                    self.events.push(self.tick, EventKind::Player, msg);
                }
            }
            // Spirits harvest but never gain experience.
            SimEvent::Chopped {
                who: Who::Spirit(sid),
                ..
            } => {
                self.spirits[sid].units_gathered += 1;
            }
            SimEvent::Deposited {
                who: Who::Spirit(sid),
                building,
                amount,
                leg_tiles,
                work_ticks,
                gather_pos,
            } => {
                self.spirits[sid].units_delivered += amount;
                let role = self.defs.role(self.spirits[sid].role);
                if role.has_job(JobKind::Gather)
                    && let Some(gather_pos) = gather_pos
                {
                    let move_ticks = role.move_ticks;
                    self.record_delivery(
                        sid, building, gather_pos, leg_tiles, work_ticks, amount, move_ticks,
                    );
                }
            }
            SimEvent::Deposited {
                who: Who::Player, ..
            } => {}
            SimEvent::SiteSupplied {
                project, amount, ..
            } => {
                let p = &self.projects[project];
                let def = self.defs.building(p.kind);
                let msg = format!(
                    "{} site received {amount} materials ({} delivered)",
                    def.name,
                    self.defs.describe_amounts(&p.delivered)
                );
                self.events.push(self.tick, EventKind::Construction, msg);
            }
            SimEvent::ProjectCompleted { project } => self.on_project_completed(project),
        }
    }
}
