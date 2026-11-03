//! Primitive tasks and their per-tick execution.
//!
//! A job (see `behavior`) expands into a short queue of tasks. Tasks are the
//! only code that moves resources between trees, inventories, storages and
//! construction sites, which keeps conservation easy to audit.

use crate::defs::{GameDefs, ResourceId};
use crate::entities::{
    Actor, Building, BuildingId, BuildingState, Project, ProjectId, ProjectState, SpiritId, Tree,
    TreeId, TreeState,
};
use crate::grid::{Grid, TilePos};
use crate::world::Ledger;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Goal {
    Exact(TilePos),
    Adjacent(TilePos),
}

impl Goal {
    pub fn reached(self, p: TilePos) -> bool {
        match self {
            Goal::Exact(t) => p == t,
            Goal::Adjacent(t) => p.is_adjacent(t),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Task {
    Move {
        goal: Goal,
        label: String,
    },
    Chop {
        tree: TreeId,
    },
    Deposit {
        building: BuildingId,
    },
    Withdraw {
        building: BuildingId,
        resource: ResourceId,
        amount: u32,
        reserved: bool,
    },
    DeliverToSite {
        project: ProjectId,
    },
    Build {
        project: ProjectId,
    },
}

impl Task {
    pub fn describe(&self) -> String {
        match self {
            Task::Move { label, .. } => format!("Walk to {label}"),
            Task::Chop { tree } => format!("Chop tree #{tree}"),
            Task::Deposit { building } => format!("Deposit at building #{building}"),
            Task::Withdraw {
                building, amount, ..
            } => format!("Collect {amount} from building #{building}"),
            Task::DeliverToSite { project } => format!("Supply project #{project}"),
            Task::Build { project } => format!("Build project #{project}"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Who {
    Player,
    Spirit(SpiritId),
}

/// Side effects reported back to the world after a task step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SimEvent {
    Chopped {
        who: Who,
        tree: TreeId,
        resource: ResourceId,
        xp: u64,
    },
    Deposited {
        who: Who,
        building: BuildingId,
        amount: u32,
        leg_tiles: u32,
        work_ticks: u32,
        gather_pos: Option<TilePos>,
    },
    SiteSupplied {
        who: Who,
        project: ProjectId,
        amount: u32,
    },
    ProjectCompleted {
        project: ProjectId,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StepResult {
    Continue,
    Done,
    Failed(String),
}

/// Mutable world state a task may touch.
pub struct TaskCtx<'a> {
    pub defs: &'a GameDefs,
    pub grid: &'a Grid,
    pub trees: &'a mut [Tree],
    pub buildings: &'a mut [Building],
    pub projects: &'a mut [Project],
    pub ledger: &'a mut Ledger,
    pub tick: u64,
    pub events: &'a mut Vec<SimEvent>,
}

/// Executes one tick of the actor's current task.
///
/// `level` is the actor's competency for timed work. `gather_budget`, when
/// present, limits how many more units the actor should harvest (used to
/// stop spirits once the settlement objective is met).
pub fn step(
    actor: &mut Actor,
    who: Who,
    level: u32,
    gather_budget: Option<&mut i64>,
    ctx: &mut TaskCtx,
) -> StepResult {
    let Some(task) = actor.queue.front().cloned() else {
        return StepResult::Done;
    };
    match task {
        Task::Move { goal, label } => step_move(actor, goal, &label, ctx),
        Task::Chop { tree } => step_chop(actor, who, level, tree, gather_budget, ctx),
        Task::Deposit { building } => step_deposit(actor, who, building, ctx),
        Task::Withdraw {
            building,
            resource,
            amount,
            reserved,
        } => step_withdraw(actor, building, resource, amount, reserved, ctx),
        Task::DeliverToSite { project } => step_supply(actor, who, project, ctx),
        Task::Build { project } => step_build(actor, level, project, ctx),
    }
}

fn step_move(actor: &mut Actor, goal: Goal, label: &str, ctx: &mut TaskCtx) -> StepResult {
    let m = &mut actor.mover;
    if m.progress == 0 && goal.reached(m.pos) {
        m.path.clear();
        return StepResult::Done;
    }
    if m.path.is_empty() {
        match ctx.grid.find_path(m.pos, |p| goal.reached(p)) {
            None => return StepResult::Failed(format!("No path to {label}")),
            Some(p) if p.is_empty() => return StepResult::Done,
            Some(p) => m.path = p.into(),
        }
    }
    let next = *m.path.front().unwrap();
    if !ctx.grid.is_walkable(next) {
        // Something was built in the way; re-plan the route next tick.
        m.path.clear();
        m.progress = 0;
        return StepResult::Continue;
    }
    m.progress += 1;
    if m.progress >= m.ticks_per_tile {
        m.pos = next;
        m.path.pop_front();
        m.progress = 0;
        actor.leg_tiles += 1;
        if m.path.is_empty() && goal.reached(m.pos) {
            return StepResult::Done;
        }
    }
    StepResult::Continue
}

fn step_chop(
    actor: &mut Actor,
    who: Who,
    level: u32,
    tree_id: TreeId,
    gather_budget: Option<&mut i64>,
    ctx: &mut TaskCtx,
) -> StepResult {
    let tree = &mut ctx.trees[tree_id];
    let def = &ctx.defs.trees[tree.kind.index()];
    if !actor.pos().is_adjacent(tree.pos) {
        return StepResult::Failed(format!("Not next to the {}", def.name));
    }
    if level < def.level_required {
        let skill = &ctx.defs.skill(def.skill).name;
        return StepResult::Failed(format!(
            "Requires {skill} level {} to cut {}",
            def.level_required, def.name
        ));
    }
    let TreeState::Grown { logs } = tree.state else {
        return StepResult::Failed(format!("The {} is depleted", def.name));
    };
    if actor.inventory.is_full() {
        return StepResult::Done;
    }
    let mut budget = gather_budget;
    if budget.as_deref().is_some_and(|b| *b <= 0) {
        return StepResult::Done;
    }
    actor.action_ticks += 1;
    actor.work_ticks += 1;
    if actor.action_ticks < def.chop_ticks_at(level) {
        return StepResult::Continue;
    }
    actor.action_ticks = 0;
    let added = actor.inventory.add(def.resource, 1);
    debug_assert_eq!(added, 1);
    *ctx.ledger.harvested.entry(def.resource).or_default() += 1;
    if let Some(b) = budget.as_deref_mut() {
        *b -= 1;
    }
    actor.leg_tiles = 0;
    actor.last_gather = Some(tree.pos);
    ctx.events.push(SimEvent::Chopped {
        who,
        tree: tree_id,
        resource: def.resource,
        xp: def.xp_per_log,
    });
    if logs <= 1 {
        tree.state = TreeState::Depleted {
            regrow_at: ctx.tick + tree.regrow_ticks,
        };
        tree.claimed_by = None;
        return StepResult::Done;
    }
    tree.state = TreeState::Grown { logs: logs - 1 };
    if actor.inventory.is_full() || budget.is_some_and(|b| *b <= 0) {
        return StepResult::Done;
    }
    StepResult::Continue
}

fn step_deposit(
    actor: &mut Actor,
    who: Who,
    building_id: BuildingId,
    ctx: &mut TaskCtx,
) -> StepResult {
    let b = &mut ctx.buildings[building_id];
    if !b.is_complete() || !actor.pos().is_adjacent(b.pos) {
        return StepResult::Failed(format!("Cannot reach {}", b.name));
    }
    let carried: Vec<(ResourceId, u32)> = actor
        .inventory
        .items()
        .iter()
        .map(|(r, n)| (*r, *n))
        .collect();
    let mut amount = 0;
    for (r, n) in carried {
        let stored = b.storage.deposit(r, n);
        actor.inventory.remove(r, stored);
        amount += stored;
    }
    if amount == 0 && actor.inventory.total() > 0 {
        return StepResult::Failed(format!("{} is full", b.name));
    }
    ctx.events.push(SimEvent::Deposited {
        who,
        building: building_id,
        amount,
        leg_tiles: actor.leg_tiles,
        work_ticks: actor.work_ticks,
        gather_pos: actor.last_gather,
    });
    actor.leg_tiles = 0;
    actor.work_ticks = 0;
    actor.last_gather = None;
    StepResult::Done
}

fn step_withdraw(
    actor: &mut Actor,
    building_id: BuildingId,
    resource: ResourceId,
    amount: u32,
    reserved: bool,
    ctx: &mut TaskCtx,
) -> StepResult {
    let b = &mut ctx.buildings[building_id];
    if !b.is_complete() || !actor.pos().is_adjacent(b.pos) {
        return StepResult::Failed(format!("Cannot reach {}", b.name));
    }
    let want = amount.min(actor.inventory.free());
    let taken = if reserved {
        b.storage.withdraw_reserved(resource, want)
    } else {
        b.storage.withdraw_available(resource, want)
    };
    if taken == 0 {
        return StepResult::Failed(format!("Nothing to collect at {}", b.name));
    }
    actor.inventory.add(resource, taken);
    StepResult::Done
}

fn step_supply(
    actor: &mut Actor,
    who: Who,
    project_id: ProjectId,
    ctx: &mut TaskCtx,
) -> StepResult {
    let project = &mut ctx.projects[project_id];
    let site = ctx.buildings[project.building].pos;
    if !actor.pos().is_adjacent(site) {
        return StepResult::Failed("Not at the construction site".into());
    }
    let def = &ctx.defs.buildings[project.kind.index()];
    let mut amount = 0;
    for &(r, need) in &def.cost {
        let have = project.delivered.get(&r).copied().unwrap_or(0);
        let give = actor.inventory.remove(r, need.saturating_sub(have));
        if give > 0 {
            *project.delivered.entry(r).or_default() += give;
            amount += give;
        }
    }
    if amount > 0 {
        ctx.events.push(SimEvent::SiteSupplied {
            who,
            project: project_id,
            amount,
        });
    }
    StepResult::Done
}

fn step_build(
    actor: &mut Actor,
    level: u32,
    project_id: ProjectId,
    ctx: &mut TaskCtx,
) -> StepResult {
    let project = &mut ctx.projects[project_id];
    let building = &mut ctx.buildings[project.building];
    if !actor.pos().is_adjacent(building.pos) {
        return StepResult::Failed("Not at the construction site".into());
    }
    let def = &ctx.defs.buildings[project.kind.index()];
    let complete = def
        .cost
        .iter()
        .all(|&(r, n)| project.delivered.get(&r).copied().unwrap_or(0) >= n);
    if !complete {
        return StepResult::Failed("Construction materials are missing".into());
    }
    if project.build_required == 0 {
        project.build_required = def.build_ticks_at(level);
    }
    project.state = ProjectState::Building;
    project.build_progress += 1;
    if project.build_progress < project.build_required {
        return StepResult::Continue;
    }
    // Materials become part of the building and leave the economy.
    for (r, n) in std::mem::take(&mut project.delivered) {
        *ctx.ledger.consumed.entry(r).or_default() += n as u64;
        *project.consumed.entry(r).or_default() += n;
    }
    project.state = ProjectState::Complete;
    project.completed_tick = Some(ctx.tick);
    project.blocked = None;
    building.state = BuildingState::Complete;
    ctx.events.push(SimEvent::ProjectCompleted {
        project: project_id,
    });
    StepResult::Done
}
