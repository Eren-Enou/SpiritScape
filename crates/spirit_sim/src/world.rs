//! The simulation state and its fixed-step update.

use crate::defs::{
    BuildingKindId, GameDefs, JobKind, RecipeId, ResourceId, RoleId, SiteId, SkillId, TreeKindId,
};
use crate::entities::{
    Activity, Actor, Building, BuildingId, Player, Project, Spirit, SpiritId, SpiritPolicy, Tree,
    TreeState,
};
use crate::events::{EventKind, EventLog, Notices};
use crate::grid::{Grid, TilePos};
use crate::journey::Journey;
use crate::metrics::{SNAPSHOT_TICKS, SiteStats, WaitKind};
use crate::planner::PlannerState;
use crate::production::ProductionOrder;
use crate::tasks::{self, Budgets, Goal, SimEvent, StepResult, Task, TaskCtx, Who};
use crate::training::TrainingSupport;
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
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Ledger {
    /// Resources placed by the scenario at start.
    pub seeded: BTreeMap<ResourceId, u64>,
    /// Resources harvested from trees.
    pub harvested: BTreeMap<ResourceId, u64>,
    /// Resources permanently consumed by construction.
    pub consumed: BTreeMap<ResourceId, u64>,
    /// Resources used up as recipe inputs.
    pub processed: BTreeMap<ResourceId, u64>,
    /// Resources created as recipe outputs.
    pub crafted: BTreeMap<ResourceId, u64>,
    /// Completed batches of each recipe.
    pub batches: BTreeMap<RecipeId, u64>,
    /// XP the player earned through work.
    pub xp_earned: u64,
    /// XP the player transferred into spirits.
    pub xp_separated: u64,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Objective {
    pub resource: ResourceId,
    pub target: u32,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Settlement {
    pub objective: Objective,
    pub allow_autonomous_construction: bool,
    pub planner: PlannerState,
    /// Per site: may spirits gather there? Closing a site reserves it for
    /// the player's own training.
    pub site_open: Vec<bool>,
    pub order: ProductionOrder,
    /// How the settlement helps the player's own skilling.
    pub training: TrainingSupport,
    /// Orders beyond the main one.
    #[serde(default)]
    pub extra_orders: Vec<ProductionOrder>,
    /// Room added at stations for logs set aside for the player.
    #[serde(default)]
    pub bench_bonus: u32,
}

/// What a Soul Separation would do, shown before the player confirms.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SoulPreview {
    pub current_xp: u64,
    pub current_level: u32,
    pub transfer: u64,
    pub after_xp: u64,
    pub after_level: u32,
    /// The new spirit's permanent level.
    pub spirit_level: u32,
    /// Tree types the player can cut now but could not afterwards.
    pub lost: Vec<TreeKindId>,
    /// Tree types the new spirit will be able to cut.
    pub spirit_can_cut: Vec<TreeKindId>,
    /// Recipes the player can make now but could not afterwards.
    pub lost_recipes: Vec<RecipeId>,
    /// Recipes the new spirit will be able to make.
    pub spirit_recipes: Vec<RecipeId>,
    /// What the spirit would add to the settlement, in plain words.
    pub benefit: String,
    /// Why the ritual can't be performed, if it can't.
    pub error: Option<String>,
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
    /// Open or close a gathering site to spirits.
    SetSiteOpen {
        site: SiteId,
        open: bool,
    },
    /// Stock of the production output to maintain in Main Storage.
    SetProductionTarget(u32),
    /// Raw materials production must leave in Main Storage.
    SetInputReserve(u32),
    SetProductionEnabled(bool),
    /// Put a crafting spirit on or take it off the production order.
    SetSpiritAssigned {
        spirit: SpiritId,
        assigned: bool,
    },
    /// Approve a building the planner won't start on its own.
    RequestBuilding(BuildingKindId),
    /// Stock of a product to maintain, for any order.
    SetOrderTarget {
        output: ResourceId,
        target: u32,
    },
    /// Input stock an order must leave in Main Storage.
    SetOrderReserve {
        output: ResourceId,
        reserve: u32,
    },
    SetOrderEnabled {
        output: ResourceId,
        enabled: bool,
    },
    /// Higher goes first when workers, haulers or benches are short.
    SetOrderPriority {
        output: ResourceId,
        priority: u32,
    },
    /// Build at a tile the player chose.
    RequestBuildingAt {
        kind: BuildingKindId,
        site: TilePos,
    },
    /// Replace one spirit's instructions.
    SetSpiritPolicy {
        spirit: SpiritId,
        policy: SpiritPolicy,
    },
    /// Leave one place at every bench free for the player.
    SetKeepBenchPlace(bool),
    /// Choose where the player trains, or no such place. Spirits stay out.
    SetTrainingArea(Option<SiteId>),
    /// Have a Hauling Spirit empty the Training Cache.
    SetTrainingCollect(bool),
    /// Have a Hauling Spirit keep the bench stocked for the player.
    SetTrainingSupply(bool),
    /// Put training support ahead of production for Hauling Spirits.
    SetTrainingFirst(bool),
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
    /// Harvest records per gathering site.
    pub site_stats: Vec<SiteStats>,
    /// Record of finished production output.
    pub output_stats: SiteStats,
    /// Short messages about important moments.
    pub notices: Notices,
    /// Guidance through a real playthrough. Scenarios don't have it.
    pub journey: Option<Journey>,
    /// Finished units of each product.
    pub product_stats: BTreeMap<ResourceId, SiteStats>,
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

    /// Stock counted toward a goal: unreserved units in main storage plus
    /// units already on their way there (in outposts, finished at a station,
    /// or carried by spirits). Construction materials and raw materials
    /// committed to a station are excluded.
    pub fn projected_stock(&self, r: ResourceId) -> u32 {
        let main = self.main().storage.available(r);
        let outposts: u32 = self.outposts().map(|b| b.storage.get(r)).sum();
        let finished: u32 = if self.order_for(r).is_some() {
            self.stations_for(r).iter().map(|b| b.storage.get(r)).sum()
        } else {
            0
        };
        let carried: u32 = self
            .spirits
            .iter()
            .filter(|s| {
                !matches!(
                    s.job,
                    Some(
                        JobKind::Construct
                            | JobKind::Supply
                            | JobKind::Craft
                            | JobKind::TrainingSupply
                    )
                )
            })
            .map(|s| s.actor.inventory.get(r))
            .sum();
        main + outposts + finished + carried
    }

    pub fn objective_unmet(&self) -> bool {
        let o = &self.settlement.objective;
        self.projected_stock(o.resource) < self.gather_goal()
    }

    /// Completed storages other than Main Storage that gatherers may use.
    pub(crate) fn outposts(&self) -> impl Iterator<Item = &Building> {
        self.buildings.iter().filter(|b| {
            b.id != self.main_storage
                && b.is_complete()
                && self.defs.building(b.kind).accepts_gatherer_deposits
        })
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

    /// Checks, for every resource, that everything created is accounted for:
    /// seeded + harvested + crafted == present + consumed + processed.
    /// Also checks that recipe totals match the batches actually completed.
    pub fn check_conservation(&self) -> Result<(), String> {
        for i in 0..self.defs.resources.len() {
            let r = ResourceId(i as u16);
            let get = |m: &BTreeMap<ResourceId, u64>| m.get(&r).copied().unwrap_or(0);
            let created =
                get(&self.ledger.seeded) + get(&self.ledger.harvested) + get(&self.ledger.crafted);
            let accounted = self.resource_in_world(r)
                + get(&self.ledger.consumed)
                + get(&self.ledger.processed);
            if created != accounted {
                return Err(format!(
                    "{}: seeded+harvested+crafted={created} but present+consumed+processed={accounted}",
                    self.defs.resource(r).name
                ));
            }
            let batches = |amounts: &dyn Fn(RecipeId) -> u64| -> u64 {
                self.ledger
                    .batches
                    .iter()
                    .map(|(id, n)| amounts(*id) * n)
                    .sum()
            };
            let used = batches(&|id| {
                self.defs
                    .recipe(id)
                    .inputs
                    .iter()
                    .filter(|(res, _)| *res == r)
                    .map(|(_, n)| *n as u64)
                    .sum()
            });
            let made = batches(&|id| {
                let (res, n) = self.defs.recipe(id).output;
                if res == r { n as u64 } else { 0 }
            });
            if used != get(&self.ledger.processed) || made != get(&self.ledger.crafted) {
                return Err(format!(
                    "{}: recipe batches don't match the ledger",
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
            let reserved: Vec<u32> = (0..self.defs.resources.len())
                .map(|i| b.storage.reserved(ResourceId(i as u16)))
                .collect();
            (
                b.pos,
                b.is_complete(),
                b.storage.contents().clone(),
                b.storage.capacity,
                reserved,
            )
                .hash(&mut h);
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
        for s in &self.spirits {
            s.time.total.hash(&mut h);
        }
        for st in &self.site_stats {
            st.harvested_total.hash(&mut h);
        }
        for p in &self.projects {
            (p.site, p.state as u8, p.build_progress).hash(&mut h);
        }
        self.ledger.crafted.hash(&mut h);
        self.ledger.batches.hash(&mut h);
        self.events.total.hash(&mut h);
        self.notices.total.hash(&mut h);
        self.ledger.xp_earned.hash(&mut h);
        self.ledger.xp_separated.hash(&mut h);
        self.player.time.hash(&mut h);
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
            Command::SetSiteOpen { site, open } => {
                let Some(slot) = self.settlement.site_open.get_mut(site.index()) else {
                    return Err("Unknown site".into());
                };
                *slot = open;
                if open && self.settlement.training.area == Some(site) {
                    self.settlement.training.area = None;
                }
                let name = &self.defs.site(site).name;
                let msg = if open {
                    format!("{name} is open to spirits")
                } else {
                    format!(
                        "{name} is reserved for your own training: spirits will not gather there"
                    )
                };
                self.events.push(self.tick, EventKind::Player, msg.clone());
                Ok(msg)
            }
            Command::SetProductionTarget(target) => {
                let order = &mut self.settlement.order;
                order.target = target;
                let msg = format!(
                    "Production target: maintain {target} {} in Main Storage",
                    self.defs.resource(order.output).name
                );
                self.events.push(self.tick, EventKind::Player, msg.clone());
                Ok(msg)
            }
            Command::SetInputReserve(reserve) => {
                self.settlement.order.input_reserve = reserve;
                let msg = format!(
                    "Log reserve: production leaves at least {reserve} {} in Main Storage",
                    self.defs.resource(self.defs.objective.resource).name
                );
                self.events.push(self.tick, EventKind::Player, msg.clone());
                Ok(msg)
            }
            Command::SetProductionEnabled(enabled) => {
                self.settlement.order.enabled = enabled;
                let msg = if enabled {
                    "Production enabled"
                } else {
                    "Production disabled"
                };
                self.events.push(self.tick, EventKind::Player, msg);
                Ok(msg.into())
            }
            Command::SetSpiritAssigned { spirit, assigned } => {
                let Some(s) = self.spirits.get_mut(spirit) else {
                    return Err("Unknown spirit".into());
                };
                s.assigned = assigned;
                let output = &self.defs.resource(self.settlement.order.output).name;
                let msg = if assigned {
                    format!("{} assigned to make {output}", s.name)
                } else {
                    format!("{} taken off {output} production", s.name)
                };
                self.events.push(self.tick, EventKind::Player, msg.clone());
                Ok(msg)
            }
            Command::RequestBuilding(kind) => self.request_building(kind),
            Command::RequestBuildingAt { kind, site } => self.request_building_at(kind, site),
            Command::SetOrderTarget { output, target } => {
                let name = self.defs.resource(output).name.clone();
                let order = self.order_for_mut(output).ok_or("No such order")?;
                order.target = target;
                let msg = format!("Production target: maintain {target} {name} in Main Storage");
                self.events.push(self.tick, EventKind::Player, msg.clone());
                Ok(msg)
            }
            Command::SetOrderReserve { output, reserve } => {
                let order = self.order_for_mut(output).ok_or("No such order")?;
                order.input_reserve = reserve;
                let msg = format!(
                    "{} now keeps {reserve} in reserve",
                    self.defs.resource(output).name
                );
                self.events.push(self.tick, EventKind::Player, msg.clone());
                Ok(msg)
            }
            Command::SetOrderEnabled { output, enabled } => {
                let order = self.order_for_mut(output).ok_or("No such order")?;
                order.enabled = enabled;
                let state = if enabled { "on" } else { "off" };
                let msg = format!("{} production {state}", self.defs.resource(output).name);
                self.events.push(self.tick, EventKind::Player, msg.clone());
                Ok(msg)
            }
            Command::SetOrderPriority { output, priority } => {
                let order = self.order_for_mut(output).ok_or("No such order")?;
                order.priority = priority;
                let msg = format!(
                    "{} priority set to {priority}",
                    self.defs.resource(output).name
                );
                self.events.push(self.tick, EventKind::Player, msg.clone());
                Ok(msg)
            }
            Command::SetSpiritPolicy { spirit, policy } => {
                let valid = policy
                    .station
                    .is_none_or(|b| self.buildings.get(b).is_some())
                    && policy
                        .site
                        .is_none_or(|s| s.index() < self.defs.sites.len());
                if !valid {
                    return Err("That place doesn't exist".into());
                }
                let Some(s) = self.spirits.get_mut(spirit) else {
                    return Err("Unknown spirit".into());
                };
                s.policy = policy;
                let msg = format!("{} has new instructions", s.name);
                self.events.push(self.tick, EventKind::Player, msg.clone());
                Ok(msg)
            }
            Command::SetKeepBenchPlace(on) => {
                self.settlement.training.keep_bench_place = on;
                let msg = if on {
                    "Spirits will leave a place free for you at every bench"
                } else {
                    "Spirits may take every place at a bench"
                };
                self.events.push(self.tick, EventKind::Player, msg);
                Ok(msg.into())
            }
            Command::SetTrainingArea(site) => self.set_training_area(site),
            Command::SetTrainingCollect(on) => Ok(self.set_training_collect(on)),
            Command::SetTrainingSupply(on) => Ok(self.set_training_supply(on)),
            Command::SetTrainingFirst(on) => Ok(self.set_training_first(on)),
        }
    }

    /// Whether the player's current level allows cutting this tree type.
    pub fn player_can_cut(&self, kind: TreeKindId) -> bool {
        let def = self.defs.tree(kind);
        self.player.level(def.skill, &self.defs.xp_table) >= def.level_required
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
            let def = self.defs.tree(tree.kind);
            let name = &def.name;
            if !self.player_can_cut(tree.kind) {
                let level = self.player.level(def.skill, &self.defs.xp_table);
                return Err(format!(
                    "You need {} level {} to cut {name} (you are level {level})",
                    self.defs.skill(def.skill).name,
                    def.level_required
                ));
            }
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
            let id = b.id;
            let def = self.defs.building(b.kind).clone();
            if def.is_station {
                self.player_station_activity(&self.buildings[id])?;
                self.claim_player_slot(id)?;
            }
            let b = &self.buildings[id];
            let walk = Task::Move {
                goal: Goal::Adjacent(p),
                label: b.name.clone(),
            };
            let logs = self.player_takes(b);
            let bag = &self.player.actor.inventory;
            if def.is_station {
                let activity = self.player_station_activity(b)?;
                let craft = Task::Craft {
                    building: b.id,
                    output: None,
                };
                (vec![walk, craft], activity)
            } else if def.capacity == 0 {
                return Err(format!(
                    "The {} adds {} to Main Storage's capacity",
                    b.name, def.capacity_bonus
                ));
            } else if bag.total() == 0 && b.storage.available(logs) > 0 {
                // Empty-handed at a storage: take logs instead of depositing.
                let amount = bag.free().min(b.storage.available(logs));
                let task = Task::Withdraw {
                    building: b.id,
                    resource: logs,
                    amount,
                    reserved: false,
                };
                let name = &self.defs.resource(logs).name;
                (
                    vec![walk, task],
                    format!("Taking {amount} {name} from {}", b.name),
                )
            } else {
                (
                    vec![walk, Task::Deposit { building: b.id }],
                    format!("Depositing at {}", b.name),
                )
            }
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

    /// What empty hands pick up at a storage: the input of the best recipe
    /// the player can make from what's there, or else ordinary logs.
    fn player_takes(&self, b: &Building) -> ResourceId {
        let mut recipes: Vec<_> = self.defs.recipes.iter().collect();
        recipes.sort_by_key(|d| std::cmp::Reverse(d.level_required));
        recipes
            .into_iter()
            .filter(|d| self.player.level(d.skill, &self.defs.xp_table) >= d.level_required)
            .flat_map(|d| d.inputs.iter())
            .find(|(r, n)| b.storage.available(*r) >= *n)
            .map_or(self.defs.objective.resource, |(r, _)| *r)
    }

    /// What the player would make at a station, or why they can't.
    fn player_station_activity(&self, b: &Building) -> Result<String, String> {
        let recipes = self.defs.station_recipes(b.kind);
        let first = recipes.first().map(|r| self.defs.recipe(*r));
        let first = first.ok_or_else(|| format!("Nothing can be made at {}", b.name))?;
        let level = self.player.level(first.skill, &self.defs.xp_table);
        if level < first.level_required {
            return Err(format!(
                "You need {} level {} to make {} (you are level {level})",
                self.defs.skill(first.skill).name,
                first.level_required,
                first.name
            ));
        }
        let bag = &self.player.actor.inventory;
        let have = |r| bag.get(r) + b.storage.get(r);
        match self.defs.best_recipe(b.kind, level, have) {
            Some(r) => Ok(format!("Making {} at {}", self.defs.recipe(r).name, b.name)),
            None => {
                let (r, n) = first.inputs[0];
                Err(format!(
                    "You need {n} {} to make {}: take some from Main Storage",
                    self.defs.resource(r).name,
                    first.name
                ))
            }
        }
    }

    /// The player's level in the skill used at a station.
    pub fn player_station_level(&self, b: &Building) -> u32 {
        self.defs.station_recipes(b.kind).first().map_or(1, |r| {
            self.player
                .level(self.defs.recipe(*r).skill, &self.defs.xp_table)
        })
    }

    /// Predicts a Soul Separation of `xp` from `skill` without performing it.
    pub fn soul_preview(&self, skill: SkillId, xp: u64) -> SoulPreview {
        let table = &self.defs.xp_table;
        let current_xp = self.player.xp(skill);
        let current_level = table.level_for_xp(current_xp);
        let after_xp = current_xp.saturating_sub(xp);
        let after_level = table.level_for_xp(after_xp);
        let spirit_level = table.level_for_xp(xp);
        let tiers = self.defs.tree_tiers(skill);
        let required = |k: &TreeKindId| self.defs.tree(*k).level_required;
        let recipes = self.defs.recipe_tiers(skill);
        let recipe_level = |r: &RecipeId| self.defs.recipe(*r).level_required;
        let skill_name = &self.defs.skill(skill).name;
        let error = if self.defs.soul_role_for(skill).is_none() {
            Some(format!("{skill_name} cannot form a spirit yet"))
        } else if xp < self.defs.soul_min_xp {
            Some(format!(
                "At least {} XP is required to form a spirit",
                self.defs.soul_min_xp
            ))
        } else if xp > current_xp {
            Some(format!(
                "Not enough {skill_name} XP ({current_xp} available)"
            ))
        } else {
            None
        };
        SoulPreview {
            current_xp,
            current_level,
            transfer: xp,
            after_xp,
            after_level,
            spirit_level,
            lost: tiers
                .iter()
                .copied()
                .filter(|k| current_level >= required(k) && after_level < required(k))
                .collect(),
            spirit_can_cut: tiers
                .iter()
                .copied()
                .filter(|k| spirit_level >= required(k))
                .collect(),
            lost_recipes: recipes
                .iter()
                .copied()
                .filter(|r| current_level >= recipe_level(r) && after_level < recipe_level(r))
                .collect(),
            spirit_recipes: recipes
                .iter()
                .copied()
                .filter(|r| spirit_level >= recipe_level(r))
                .collect(),
            benefit: self.spirit_benefit(skill, spirit_level),
            error,
        }
    }

    pub(crate) fn tree_names(&self, kinds: &[TreeKindId]) -> String {
        kinds
            .iter()
            .map(|k| self.defs.tree(*k).name.clone())
            .collect::<Vec<_>>()
            .join(", ")
    }

    pub(crate) fn recipe_names(&self, ids: &[RecipeId]) -> String {
        ids.iter()
            .map(|r| self.defs.recipe(*r).name.clone())
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Soul Separation: moves `xp` from the player's skill into a new spirit.
    pub fn soul_separate(&mut self, skill: SkillId, xp: u64) -> Result<SpiritId, String> {
        let preview = self.soul_preview(skill, xp);
        if let Some(e) = preview.error {
            return Err(e);
        }
        let skill_name = self.defs.skill(skill).name.clone();
        let role = self.defs.soul_role_for(skill).expect("checked by preview");
        if !self.player.take_xp(skill, xp) {
            return Err(format!("Not enough {skill_name} XP"));
        }
        self.player.record_sacrifice(skill, xp);
        self.ledger.xp_separated += xp;
        // A level drop can make the current chop target illegal.
        if matches!(self.player.actor.queue.front(), Some(Task::Chop { tree }) if !self.player_can_cut(self.trees[*tree].kind))
        {
            self.player.actor.clear_tasks();
            self.player.activity = "Idle".into();
        }
        let pos = self.player.actor.pos();
        let id = self.add_spirit(role, xp, pos);
        let s = &self.spirits[id];
        let mut msg = format!(
            "Soul Separation: sacrificed {xp} {skill_name} XP (level {} -> {}). {} awakens as a {} at permanent level {}",
            preview.current_level,
            preview.after_level,
            s.name,
            self.defs.role(role).name,
            s.level()
        );
        if !preview.spirit_can_cut.is_empty() {
            msg += &format!(" and can cut {}", self.tree_names(&preview.spirit_can_cut));
        }
        if !preview.spirit_recipes.is_empty() {
            msg += &format!(
                " and can make {}",
                self.recipe_names(&preview.spirit_recipes)
            );
        }
        msg.push('.');
        if !preview.lost.is_empty() {
            msg += &format!(
                " You can no longer cut {} until you retrain.",
                self.tree_names(&preview.lost)
            );
        }
        if !preview.lost_recipes.is_empty() {
            msg += &format!(
                " You can no longer make {} until you retrain.",
                self.recipe_names(&preview.lost_recipes)
            );
        }
        self.events.push(self.tick, EventKind::Player, msg);
        let notice = self.spirit_notice(role, &preview);
        self.notices.push(self.tick, notice);
        Ok(id)
    }

    // ------------------------------------------------------------------
    // Simulation step
    // ------------------------------------------------------------------

    pub fn step(&mut self) {
        self.tick += 1;
        self.regrow_trees();
        let window = self.defs.gathering.window_ticks;
        for st in &mut self.site_stats {
            st.trim(self.tick, window);
        }
        self.output_stats.trim(self.tick, window);
        for st in self.product_stats.values_mut() {
            st.trim(self.tick, window);
        }
        if self.tick.is_multiple_of(SNAPSHOT_TICKS) {
            for s in &mut self.spirits {
                s.time.snapshot(self.tick, window);
            }
        }
        self.update_projects();
        if self
            .tick
            .is_multiple_of(self.defs.planner.eval_interval_ticks)
        {
            self.evaluate_logistics();
        }

        let mut events = Vec::new();
        self.player.account_tick();
        self.step_player(&mut events);
        self.step_spirits(&mut events);
        for e in events {
            self.handle_event(e);
        }
        self.update_journey();
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
        player_output_at_station: bool,
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
            player_output_at_station,
        }
    }

    fn step_player(&mut self, events: &mut Vec<SimEvent>) {
        let level = self.player_task_level();
        let at_station = self.player_output_at_station();
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
            defs, grid, trees, buildings, projects, ledger, *tick, events, at_station,
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

    /// The player's level in whatever skill their current task uses.
    fn player_task_level(&self) -> u32 {
        match self.player.actor.queue.front() {
            Some(Task::Chop { tree }) => {
                let skill = self.defs.tree(self.trees[*tree].kind).skill;
                self.player.level(skill, &self.defs.xp_table)
            }
            Some(Task::Craft { building, .. }) => {
                self.player_station_level(&self.buildings[*building])
            }
            _ => 1,
        }
    }

    fn step_spirits(&mut self, events: &mut Vec<SimEvent>) {
        let mut budgets = Budgets::default();
        for i in 0..self.defs.resources.len() {
            let r = ResourceId(i as u16);
            let stock = self.projected_stock(r) as i64;
            budgets
                .gather
                .insert(r, self.gather_goal_of(r) as i64 - stock);
            budgets.craft.insert(r, self.deficit_of(r) as i64);
        }
        for sid in 0..self.spirits.len() {
            self.switch_craft_if_needed(sid);
            if self.spirits[sid].actor.queue.is_empty() && self.tick >= self.spirits[sid].next_think
            {
                self.think(sid);
            }
            self.spirits[sid].account_tick();
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
                defs, grid, trees, buildings, projects, ledger, *tick, events, false,
            );
            match tasks::step(
                &mut spirit.actor,
                Who::Spirit(sid),
                level,
                Some(&mut budgets),
                &mut ctx,
            ) {
                StepResult::Continue => {}
                StepResult::Done => {
                    spirit.actor.queue.pop_front();
                }
                StepResult::Failed(reason) => {
                    // Materials set aside for this trip go back to general use.
                    // A project's reservation belongs to the project, not the trip.
                    if spirit.job != Some(JobKind::Construct) {
                        for t in &spirit.actor.queue {
                            if let Task::Withdraw {
                                building,
                                resource,
                                amount,
                                reserved: true,
                            } = t
                            {
                                buildings[*building]
                                    .storage
                                    .release_reserved(*resource, *amount);
                            }
                        }
                    }
                    spirit.actor.clear_tasks();
                    spirit.next_think = *tick + defs.idle_rethink_ticks;
                    spirit.wait = Some(WaitKind::Other);
                    let msg = format!("{}: task failed - {reason}", spirit.name);
                    let key = format!("fail:{reason}");
                    spirit.activity = Activity::Blocked(reason);
                    self.log_spirit(sid, key, msg);
                }
            }
        }
    }

    fn player_earned_xp(&mut self, skill: SkillId, xp: u64) {
        let table = &self.defs.xp_table;
        let before = self.player.level(skill, table);
        let highest_before = self.player.history(skill).highest_level;
        self.player.add_xp(skill, xp, &self.defs.xp_table);
        self.ledger.xp_earned += xp;
        let after = self.player.level(skill, &self.defs.xp_table);
        if after <= before {
            return;
        }
        let skill_name = &self.defs.skill(skill).name;
        let mut msg = if after <= highest_before {
            format!("{skill_name} level {after} regained (highest ever: {highest_before}).")
        } else {
            format!("{skill_name} level up! You are now level {after}.")
        };
        let unlocked: Vec<TreeKindId> = self
            .defs
            .tree_tiers(skill)
            .into_iter()
            .filter(|k| (before + 1..=after).contains(&self.defs.tree(*k).level_required))
            .collect();
        if !unlocked.is_empty() {
            msg += &format!(" You can now cut {}.", self.tree_names(&unlocked));
        }
        let recipes: Vec<RecipeId> = self
            .defs
            .recipe_tiers(skill)
            .into_iter()
            .filter(|r| (before + 1..=after).contains(&self.defs.recipe(*r).level_required))
            .collect();
        if !recipes.is_empty() {
            msg += &format!(" You can now make {}.", self.recipe_names(&recipes));
        }
        self.events.push(self.tick, EventKind::Player, msg);
        let notice = self.level_notice(skill, after, highest_before, &unlocked, &recipes);
        self.notices.push(self.tick, notice);
    }

    fn handle_event(&mut self, e: SimEvent) {
        match e {
            SimEvent::Chopped { who, tree, xp, .. } => {
                if let Some(site) = self.trees[tree].site {
                    let window = self.defs.gathering.window_ticks;
                    self.site_stats[site.index()].record(self.tick, window);
                }
                match who {
                    Who::Player => {
                        self.player_earned_xp(self.defs.tree(self.trees[tree].kind).skill, xp)
                    }
                    // Spirits harvest but never gain experience.
                    Who::Spirit(sid) => self.spirits[sid].units_gathered += 1,
                }
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
                if self.spirits[sid].job == Some(JobKind::TrainingCollect) {
                    self.on_training_collected(sid, amount);
                }
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
            SimEvent::Crafted {
                who,
                recipe,
                building,
            } => {
                let def = self.defs.recipe(recipe);
                let (skill, xp, amount) = (def.skill, def.xp, def.output.1);
                let product = def.output.0;
                let window = self.defs.gathering.window_ticks;
                self.buildings[building].crafted += amount;
                let stats = self.product_stats.entry(product).or_default();
                for _ in 0..amount {
                    stats.record(self.tick, window);
                }
                if product == self.settlement.order.output {
                    for _ in 0..amount {
                        self.output_stats.record(self.tick, window);
                    }
                }
                match who {
                    Who::Player => self.player_earned_xp(skill, xp),
                    // Spirits produce but never gain experience.
                    Who::Spirit(sid) => self.spirits[sid].units_crafted += amount,
                }
            }
        }
    }
}
