//! Primitive tasks and their per-tick execution.
//!
//! A job (see `behavior`) expands into a short queue of tasks. Tasks are the
//! only code that moves resources between trees, inventories, storages and
//! construction sites, which keeps conservation easy to audit.

use crate::defs::{BuildingDef, GameDefs, RecipeDef, RecipeId, ResourceId};
use crate::entities::{
    Actor, Building, BuildingId, BuildingState, Project, ProjectId, ProjectState, SpiritId, Tree,
    TreeId, TreeState,
};
use crate::grid::{Grid, TilePos};
use crate::inventory::Inventory;
use crate::world::Ledger;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
    /// Like `Deposit`, but sets what's left here aside for the player.
    DepositForPlayer {
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
    /// Repeats the best available recipe until materials, room or demand run
    /// out. A spirit makes `output`; the player, with none, makes what they can.
    Craft {
        building: BuildingId,
        #[serde(default)]
        output: Option<ResourceId>,
    },
}

impl Task {
    pub fn describe(&self) -> String {
        match self {
            Task::Move { label, .. } => format!("Walk to {label}"),
            Task::Chop { tree } => format!("Chop tree #{tree}"),
            Task::Deposit { building } => format!("Deposit at building #{building}"),
            Task::DepositForPlayer { building } => {
                format!("Set materials aside at building #{building}")
            }
            Task::Withdraw {
                building, amount, ..
            } => format!("Collect {amount} from building #{building}"),
            Task::DeliverToSite { project } => format!("Supply project #{project}"),
            Task::Build { project } => format!("Build project #{project}"),
            Task::Craft { building, .. } => format!("Craft at building #{building}"),
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
    Crafted {
        who: Who,
        recipe: RecipeId,
        building: BuildingId,
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
    /// The player's finished goods stay at the station for a hauler.
    pub player_output_at_station: bool,
    /// The recipe the player chose. They make nothing else while it's set.
    pub player_recipe: Option<RecipeId>,
    /// Main Storage, and how much of each thing the player may put in it.
    pub player_room: Option<(BuildingId, BTreeMap<ResourceId, u32>)>,
}

/// How many more units spirits should harvest and craft this tick, so they
/// stop as soon as the settlement's goals are met.
#[derive(Clone, Debug, Default)]
pub struct Budgets {
    pub gather: BTreeMap<ResourceId, i64>,
    pub craft: BTreeMap<ResourceId, i64>,
}

/// Executes one tick of the actor's current task.
///
/// `level` is the actor's competency for timed work. `budgets`, when
/// present, limits what the actor should still produce; the player has none.
pub fn step(
    actor: &mut Actor,
    who: Who,
    level: u32,
    budgets: Option<&mut Budgets>,
    ctx: &mut TaskCtx,
) -> StepResult {
    let Some(task) = actor.queue.front().cloned() else {
        return StepResult::Done;
    };
    match task {
        Task::Move { goal, label } => step_move(actor, who, goal, &label, ctx),
        Task::Chop { tree } => {
            let resource = ctx.defs.trees[ctx.trees[tree].kind.index()].resource;
            let budget = budgets.map(|b| b.gather.entry(resource).or_insert(0));
            step_chop(actor, who, level, tree, budget, ctx)
        }
        Task::Deposit { building } => step_deposit(actor, who, building, false, ctx),
        Task::DepositForPlayer { building } => step_deposit(actor, who, building, true, ctx),
        Task::Withdraw {
            building,
            resource,
            amount,
            reserved,
        } => step_withdraw(actor, building, resource, amount, reserved, ctx),
        Task::DeliverToSite { project } => step_supply(actor, who, project, ctx),
        Task::Build { project } => step_build(actor, level, project, ctx),
        Task::Craft { building, output } => step_craft(
            actor,
            who,
            level,
            building,
            output,
            budgets.map(|b| &mut b.craft),
            ctx,
        ),
    }
}

fn step_move(
    actor: &mut Actor,
    who: Who,
    goal: Goal,
    label: &str,
    ctx: &mut TaskCtx,
) -> StepResult {
    let m = &mut actor.mover;
    if m.progress == 0 && goal.reached(m.pos) {
        m.path.clear();
        return StepResult::Done;
    }
    if m.path.is_empty() {
        let found = ctx.grid.find_path(m.pos, |p| goal.reached(p));
        // Spirits stop at the water. Only the player goes out on the ice, and
        // only if there's no way round.
        let found = found.or_else(|| {
            let bold = who == Who::Player;
            bold.then(|| ctx.grid.path_over_ice(m.pos, |p| goal.reached(p)))
                .flatten()
        });
        match found {
            None => return StepResult::Failed(format!("No path to {label}")),
            Some(p) if p.is_empty() => return StepResult::Done,
            Some(p) => m.path = p.into(),
        }
    }
    let next = *m.path.front().unwrap();
    let on_foot = who == Who::Player && ctx.grid.is_ice(next);
    if !ctx.grid.is_walkable(next) && !on_foot {
        // Something was built in the way; re-plan the route next tick.
        m.path.clear();
        m.progress = 0;
        return StepResult::Continue;
    }
    m.stride = ctx.grid.step_ticks(next, m.ticks_per_tile);
    m.progress += 1;
    if m.progress >= m.stride {
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

/// One tick at a station. Inputs come from the actor's inventory first, then
/// from the station's stock, and are taken only when a batch finishes, so an
/// interrupted batch costs nothing. Materials reserved at a station are set
/// aside for the player: spirits can't use them. Spirits leave the result in
/// the station for collection; the player keeps it in their bag unless
/// training support is collecting it.
fn step_craft(
    actor: &mut Actor,
    who: Who,
    level: u32,
    building_id: BuildingId,
    output: Option<ResourceId>,
    craft_budgets: Option<&mut BTreeMap<ResourceId, i64>>,
    ctx: &mut TaskCtx,
) -> StepResult {
    let station = &mut ctx.buildings[building_id];
    if !station.is_complete() || !actor.pos().is_adjacent(station.pos) {
        return StepResult::Failed(format!("Cannot reach {}", station.name));
    }
    let lowest = ctx.defs.station_recipes(station.kind);
    let Some(first) = lowest.first().map(|r| ctx.defs.recipe(*r)) else {
        return StepResult::Failed(format!("Nothing can be made at {}", station.name));
    };
    if level < first.level_required {
        let skill = &ctx.defs.skill(first.skill).name;
        return StepResult::Failed(format!(
            "Requires {skill} level {} to make {}",
            first.level_required, first.name
        ));
    }
    let is_player = who == Who::Player;
    let have = |r| {
        let stock = if is_player {
            station.storage.get(r)
        } else {
            station.storage.available(r)
        };
        actor.inventory.get(r) + stock
    };
    let chosen = match (output, ctx.player_recipe.filter(|_| is_player)) {
        (Some(o), _) => ctx.defs.best_recipe_for(station.kind, level, o, have),
        (None, Some(r)) => {
            let def = ctx.defs.recipe(r);
            let ready = def.station == station.kind
                && level >= def.level_required
                && def.inputs.iter().all(|&(res, n)| have(res) >= n);
            ready.then_some(r)
        }
        (None, None) => ctx.defs.best_recipe(station.kind, level, have),
    };
    let Some(recipe_id) = chosen else {
        actor.action_ticks = 0;
        return StepResult::Done; // Out of materials.
    };
    let recipe = ctx.defs.recipe(recipe_id);
    let (out, out_n) = recipe.output;
    let craft_budget = craft_budgets.map(|m| m.entry(out).or_insert(0));
    if craft_budget.as_deref().is_some_and(|b| *b <= 0) {
        actor.action_ticks = 0;
        return StepResult::Done;
    }
    let def = &ctx.defs.buildings[station.kind.index()];
    let (station_room, bag_room) = craft_room(def, station, &actor.inventory, recipe);
    let to_bag = is_player && bag_room && !(ctx.player_output_at_station && station_room);
    if !to_bag && !station_room {
        actor.action_ticks = 0;
        return StepResult::Done;
    }
    actor.action_ticks += 1;
    if actor.action_ticks < recipe.craft_ticks_at(level) {
        return StepResult::Continue;
    }
    actor.action_ticks = 0;
    for &(r, n) in &recipe.inputs {
        let from_bag = actor.inventory.remove(r, n);
        let set_aside = if is_player {
            station.storage.withdraw_reserved(r, n - from_bag)
        } else {
            0
        };
        let from_station = station
            .storage
            .withdraw_available(r, n - from_bag - set_aside);
        debug_assert_eq!(from_bag + set_aside + from_station, n);
        *ctx.ledger.processed.entry(r).or_default() += n as u64;
    }
    let stored = if to_bag {
        actor.inventory.add(out, out_n)
    } else {
        station.storage.deposit(out, out_n)
    };
    debug_assert_eq!(stored, out_n);
    *ctx.ledger.crafted.entry(out).or_default() += out_n as u64;
    *ctx.ledger.batches.entry(recipe_id).or_default() += 1;
    if let Some(b) = craft_budget {
        *b -= out_n as i64;
    }
    ctx.events.push(SimEvent::Crafted {
        who,
        recipe: recipe_id,
        building: building_id,
    });
    StepResult::Continue
}

/// Whether a batch's result fits at the station, and in this bag.
pub(crate) fn craft_room(
    def: &BuildingDef,
    station: &Building,
    bag: &Inventory,
    recipe: &RecipeDef,
) -> (bool, bool) {
    let (out, out_n) = recipe.output;
    let output_limit = def.capacity.saturating_sub(def.input_buffer);
    let station_room = station.storage.free() >= out_n
        && station.storage.available(out) + out_n <= output_limit.max(out_n);
    let bag_room = bag.free()
        + recipe
            .inputs
            .iter()
            .map(|&(r, n)| bag.get(r).min(n))
            .sum::<u32>()
        >= out_n;
    (station_room, bag_room)
}

fn step_deposit(
    actor: &mut Actor,
    who: Who,
    building_id: BuildingId,
    set_aside: bool,
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
    let limits = match (&ctx.player_room, who) {
        (Some((main, room)), Who::Player) if *main == building_id => Some(room),
        _ => None,
    };
    for (r, n) in carried {
        let n = limits.map_or(n, |l| l.get(&r).copied().unwrap_or(n).min(n));
        let stored = b.storage.deposit(r, n);
        actor.inventory.remove(r, stored);
        amount += stored;
        if set_aside {
            let reserved = b.storage.try_reserve(r, stored);
            debug_assert!(reserved, "just stored");
        }
    }
    if amount == 0 && actor.inventory.total() > 0 {
        return StepResult::Failed(if b.storage.free() > 0 {
            format!(
                "{} is keeping its room for what's needed. Spend what you've made on building.",
                b.name
            )
        } else {
            format!("{} is full", b.name)
        });
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
