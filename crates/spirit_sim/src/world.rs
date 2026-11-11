//! The simulation state and its fixed-step update.

use crate::defs::{
    BuildingKindId, GameDefs, JobKind, RecipeId, ResourceId, RoleId, SiteId, SkillId, TreeKindId,
};
use crate::entities::{
    Activity, Actor, Building, BuildingId, BuildingState, Player, Project, ProjectId, Spirit,
    SpiritId, SpiritPolicy, Tree, TreeState,
};
use crate::events::{EventKind, EventLog, Notices};
use crate::grid::{Grid, TilePos};
use crate::journey::Journey;
use crate::metrics::{SNAPSHOT_TICKS, SiteStats, WaitKind};
use crate::planner::PlannerState;
use crate::production::ProductionOrder;
use crate::session::SessionEnd;
use crate::tasks::{self, Budgets, Goal, SimEvent, StepResult, Task, TaskCtx, Who};
use crate::training::{TrainingMode, TrainingSupport};
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};

pub const SPIRIT_NAMES: &[&str] = &[
    "Oakroot",
    "Mossbell",
    "Ashwhisper",
    "Fernlight",
    "Birchsong",
    "Emberleaf",
    "Thistledown",
    "Willowmere",
    "Hazelhush",
    "Rowanrest",
    "Bramblewick",
    "Sedgewhisper",
    "Larchling",
    "Dewfall",
    "Hollowcap",
    "Yarrowsong",
    "Pinewake",
    "Lichenlight",
    "Alderhum",
    "Tansybrook",
    "Quillfern",
    "Marrowmoss",
    "Sorrelshade",
    "Elderdrift",
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
    /// Resources the player left behind because nowhere could take them.
    #[serde(default)]
    pub left_behind: BTreeMap<ResourceId, u64>,
    /// Resources created as recipe outputs.
    pub crafted: BTreeMap<ResourceId, u64>,
    /// Completed batches of each recipe.
    pub batches: BTreeMap<RecipeId, u64>,
    /// XP the player earned through work.
    pub xp_earned: u64,
    /// XP the player transferred into spirits.
    pub xp_separated: u64,
    /// XP that came back to the player when spirits went home on the Still
    /// Boat. It was the player's before, so it isn't counted as earned again.
    #[serde(default)]
    pub xp_returned: u64,
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
    /// Tips already given. None until they've first been read, so an older
    /// save isn't told everything again.
    #[serde(default)]
    pub tips_seen: Option<Vec<String>>,
    /// Who's at the settlement right now.
    #[serde(default)]
    pub visitor: Option<crate::visitors::VisitorHere>,
    /// Visitors so far, and offers taken.
    #[serde(default)]
    pub visitors_met: u32,
    #[serde(default)]
    pub offers_taken: u32,
    /// Whose offers were taken or turned down, for those who remember.
    #[serde(default)]
    pub offers_accepted: Vec<String>,
    #[serde(default)]
    pub offers_refused: Vec<String>,
    /// Who has come, past the point where everyone has.
    #[serde(default)]
    pub visitors_seen: Vec<u32>,
    /// When someone last arrived, so they don't come on each other's heels.
    #[serde(default)]
    pub visitor_last: Option<u64>,
    /// Raw materials to keep in stock with nothing asking for them.
    #[serde(default)]
    pub standing: BTreeMap<ResourceId, u32>,
    /// The spirits called home to the boat, once that happens.
    #[serde(default)]
    pub homecoming: Option<crate::homecoming::Homecoming>,
    /// Spirits do small things here, though it isn't a real game.
    #[serde(default)]
    pub moments: bool,
    /// The player has been told the ice won't last, this winter.
    #[serde(default)]
    pub ice_warned: bool,
    /// What's been found on the far shore.
    #[serde(default)]
    pub relics_found: Vec<String>,
    /// How many moments each pair of spirits has shared, lower id first.
    #[serde(default)]
    pub shared_moments: Vec<(usize, usize, u32)>,
    /// The stone the player carved, once it's over.
    #[serde(default)]
    pub your_stone: Option<crate::your_stone::YourStone>,
    /// Stones carved in earlier games, standing in this one.
    #[serde(default)]
    pub left_before: Vec<crate::your_stone::LeftStone>,
    /// The last of the old helpers, once you've met it.
    #[serde(default)]
    pub last_helper: Option<crate::last_helper::LastHelper>,
    /// The totals at the end of each year.
    #[serde(default)]
    pub years: Vec<crate::stories::YearEnd>,
    /// The year turns here, though it isn't a real game.
    #[serde(default)]
    pub seasons: bool,
    /// The season last announced.
    #[serde(default)]
    pub season_seen: Option<String>,
    /// Old stones the player has read, by id.
    #[serde(default)]
    pub markers_read: Vec<String>,
    /// When the Hearth was lit, in ticks.
    #[serde(default)]
    pub ended_at: Option<u64>,
    /// Trade Caravans sent so far.
    #[serde(default)]
    pub caravans: u32,
    /// Long-term goals already announced. None until they've first been read,
    /// so an older save isn't told everything it's already done.
    #[serde(default)]
    pub ambitions_seen: Option<Vec<String>>,
    /// More that every spirit can carry, from what's been built.
    #[serde(default)]
    pub carry_bonus: u32,
    /// Room added at stations for logs set aside for the player.
    #[serde(default)]
    pub bench_bonus: u32,
    /// Buildings planned after the current project.
    #[serde(default)]
    pub queue: Vec<crate::projects::QueuedProject>,
    #[serde(default)]
    pub next_queue_id: u32,
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
    /// Keep this much of a raw material in stock, wanted or not.
    SetStandingStock {
        resource: ResourceId,
        target: u32,
    },
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
    /// Stop what the player is doing, where they stand.
    PlayerStop,
    /// Leave what's in the bag behind, when nowhere can take it.
    LeaveLoad,
    /// Take or turn down what a visitor is offering.
    AnswerVisitor(bool),
    /// Call every spirit to the Still Boat. It can't be undone.
    CallHome,
    /// Once it's over, carve a stone where you stand, ending with this line.
    CarveStone(usize),
    /// Ask the last of the old helpers home, or leave it where it is.
    AnswerHelper(bool),
    /// Give a spirit a name of the player's choosing.
    RenameSpirit {
        id: SpiritId,
        name: String,
    },
    /// Whether a gathering session moves on to another tree by itself.
    SetAutoGather(bool),
    /// Choose what to make at a bench, or the best available with `None`.
    SetPlayerRecipe(Option<RecipeId>),
    /// Set the training-support options to a named way of training.
    SetTrainingMode(TrainingMode),
    /// Choose the bench to train at, or the default with `None`.
    SetTrainingBench(Option<BuildingId>),
    /// Stock haulers leave at a store for local use.
    SetLocalStock {
        building: BuildingId,
        resource: ResourceId,
        amount: u32,
    },
    /// Send everyone who can do a site's work there, or release them.
    SetOutpostStaffed {
        site: SiteId,
        on: bool,
    },
    /// Add a building to the end of the construction plan.
    QueueBuilding {
        kind: BuildingKindId,
        site: Option<TilePos>,
    },
    /// Move a planned building one place up or down.
    MoveQueued {
        id: u32,
        up: bool,
    },
    /// Take a building off the plan.
    CancelQueued(u32),
    /// Call off a project nobody has started on.
    CancelProject(ProjectId),
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
        let capacity = role_def.capacity_at(level) + self.settlement.carry_bonus;
        let actor = Actor::new(pos, role_def.move_ticks, capacity);
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
        self.buildings
            .iter()
            .filter(|b| b.pos == p && b.state != BuildingState::Removed)
            // A finished road is ground, not something to walk up to.
            .find(|b| !(b.is_complete() && !self.defs.building(b.kind).paves.is_empty()))
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
            let made: u32 = self.stations_for(r).iter().map(|b| b.storage.get(r)).sum();
            // What's set aside for the player's training isn't going anywhere.
            let held = if self.training_input() == r {
                self.bench_stock()
            } else {
                0
            };
            made.saturating_sub(held)
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
                + get(&self.ledger.processed)
                + get(&self.ledger.left_behind);
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
        let session = self.player.session.as_ref();
        (
            session.map(|s| (s.started, s.waiting_since, s.units, s.xp)),
            self.player.auto_gather,
            self.player.recipe,
            self.player.log.sessions,
        )
            .hash(&mut h);
        for s in &self.spirits {
            s.time.total.hash(&mut h);
        }
        for st in &self.site_stats {
            st.harvested_total.hash(&mut h);
        }
        for p in &self.projects {
            (p.site, p.state as u8, p.build_progress).hash(&mut h);
        }
        for q in &self.settlement.queue {
            (q.id, q.kind, q.site).hash(&mut h);
        }
        self.ledger.crafted.hash(&mut h);
        self.ledger.batches.hash(&mut h);
        self.events.total.hash(&mut h);
        self.notices.total.hash(&mut h);
        self.ledger.xp_earned.hash(&mut h);
        self.ledger.xp_separated.hash(&mut h);
        self.ledger.xp_returned.hash(&mut h);
        self.player.time.hash(&mut h);
        h.finish()
    }

    // ------------------------------------------------------------------
    // Commands
    // ------------------------------------------------------------------

    pub fn apply(&mut self, cmd: Command) -> Result<String, String> {
        match cmd {
            Command::PlayerInteract(p) => {
                let result = self.player_interact(p);
                if result.is_err() {
                    self.player.log.refused_interactions += 1;
                }
                result
            }
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
            Command::SetStandingStock { resource, target } => {
                let target = target.min(self.main().storage.capacity);
                if target == 0 {
                    self.settlement.standing.remove(&resource);
                } else {
                    self.settlement.standing.insert(resource, target);
                }
                let msg = format!(
                    "Spirits will keep {target} {} in stock",
                    self.defs.resource(resource).name
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
            Command::PlayerStop => Ok(self.player_stop()),
            Command::LeaveLoad => self.leave_load(),
            Command::AnswerVisitor(accept) => self.answer_visitor(accept),
            Command::CallHome => self.call_home(),
            Command::CarveStone(choice) => self.carve_stone(choice),
            Command::AnswerHelper(come) => self.answer_helper(come),
            Command::RenameSpirit { id, name } => self.rename_spirit(id, &name),
            Command::SetAutoGather(on) => Ok(self.set_auto_gather(on)),
            Command::SetPlayerRecipe(r) => self.set_player_recipe(r),
            Command::SetTrainingMode(mode) => Ok(self.set_training_mode(mode)),
            Command::SetTrainingBench(bench) => self.set_training_bench(bench),
            Command::SetLocalStock {
                building,
                resource,
                amount,
            } => self.set_local_stock(building, resource, amount),
            Command::SetOutpostStaffed { site, on } => self.set_outpost_staffed(site, on),
            Command::QueueBuilding { kind, site } => self.queue_building(kind, site),
            Command::MoveQueued { id, up } => self.move_queued(id, up),
            Command::CancelQueued(id) => self.cancel_queued(id),
            Command::CancelProject(id) => self.cancel_project(id),
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
        if !self.grid.is_walkable(target) && !self.grid.is_ice(target) {
            return Err("Blocked".into());
        }
        // Walking away is how a player leaves whatever they were doing.
        self.end_session(SessionEnd::Player, String::new());
        let actor = &mut self.player.actor;
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
        // What the click starts: the session ends if it's none of these.
        enum Starts {
            Gathering(usize),
            Crafting(usize),
            Nothing,
        }
        let tree_here = self.trees.iter().any(|t| t.pos == p);
        let station_here = self
            .building_at(p)
            .map(|b| self.defs.building(b.kind).is_station);
        let log = &mut self.player.log;
        match station_here {
            _ if tree_here => log.gather_interactions += 1,
            Some(true) => log.workshop_interactions += 1,
            Some(false) => log.storage_interactions += 1,
            None => log.walk_interactions += 1,
        }
        let (tasks, activity, starts) = if let Some(tree) = self.tree_at(p) {
            let def = self.defs.tree(tree.kind);
            let name = &def.name;
            if !self.player_can_cut(tree.kind) {
                let level = self.player.level(def.skill, &self.defs.xp_table);
                let mut msg = format!(
                    "You need {} level {} to cut {name} (you are level {level})",
                    self.defs.skill(def.skill).name,
                    def.level_required
                );
                let cutter = self
                    .spirits
                    .iter()
                    .find(|s| self.spirit_can_cut(s, tree.kind));
                if let Some(s) = cutter {
                    msg += &format!(". {} can cut it for you", s.name);
                }
                return Err(msg);
            }
            if !tree.is_grown() {
                return Err(format!("The {name} is depleted"));
            }
            if let Some(k) = tree.site.and_then(|s| self.site_locked_by(s)) {
                return Err(format!(
                    "You can't reach it yet: it needs a {}",
                    self.defs.building(k).name
                ));
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
                Starts::Gathering(tree.id),
            )
        } else if let Some(b) = self
            .building_at(p)
            .filter(|b| !(b.is_complete() && self.grid.is_walkable(b.pos)))
        {
            if !b.is_complete() {
                return Err(format!("{} is still under construction", b.name));
            }
            let id = b.id;
            let def = self.defs.building(b.kind).clone();
            if def.is_station {
                self.player_station_activity(id)?;
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
                let activity = self.player_station_activity(b.id)?;
                let craft = Task::Craft {
                    building: b.id,
                    output: None,
                };
                (vec![walk, craft], activity, Starts::Crafting(b.id))
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
                    Starts::Nothing,
                )
            } else {
                (
                    vec![walk, Task::Deposit { building: b.id }],
                    format!("Depositing at {}", b.name),
                    Starts::Nothing,
                )
            }
        } else if self.grid.is_walkable(p) || self.grid.is_ice(p) {
            (
                vec![Task::Move {
                    goal: Goal::Exact(p),
                    label: p.to_string(),
                }],
                format!("Walking to {p}"),
                Starts::Nothing,
            )
        } else {
            return Err("You can't walk there".into());
        };
        match starts {
            Starts::Gathering(tree) => self.begin_gathering(tree),
            Starts::Crafting(building) => self.begin_crafting(building),
            Starts::Nothing => self.end_session(SessionEnd::Player, String::new()),
        }
        // Repeated clicks on the same target keep the current order going.
        if self.player.actor.redirect(tasks) {
            self.player.activity = activity.clone();
        }
        Ok(activity)
    }

    /// What empty hands pick up at a storage: what the chosen recipe needs,
    /// or the input of the best recipe the player can make from what's
    /// there, or else ordinary logs.
    fn player_takes(&self, b: &Building) -> ResourceId {
        if let Some(r) = self.player.recipe {
            let inputs = &self.defs.recipe(r).inputs;
            let stocked = inputs.iter().find(|(res, _)| b.storage.available(*res) > 0);
            if let Some((res, _)) = stocked.or(inputs.first()) {
                return *res;
            }
        }
        let mut recipes: Vec<_> = self.defs.recipes.iter().collect();
        recipes.sort_by_key(|d| std::cmp::Reverse(d.level_required));
        recipes
            .into_iter()
            .filter(|d| !d.chosen_only)
            .filter(|d| self.player.level(d.skill, &self.defs.xp_table) >= d.level_required)
            .flat_map(|d| d.inputs.iter())
            .find(|(r, n)| b.storage.available(*r) >= *n)
            .map(|(r, _)| *r)
            .unwrap_or_else(|| {
                // With nothing to make, ordinary logs, or whatever is here.
                let basic = self.defs.objective.resource;
                let plenty = b.storage.contents().keys().copied();
                let most = plenty.max_by_key(|r| (b.storage.available(*r), std::cmp::Reverse(*r)));
                most.filter(|_| b.storage.available(basic) == 0)
                    .unwrap_or(basic)
            })
    }

    /// What the player would make at a station, or why they can't.
    fn player_station_activity(&self, id: BuildingId) -> Result<String, String> {
        let b = &self.buildings[id];
        if self.defs.station_recipes(b.kind).is_empty() {
            return Err(format!("Nothing can be made at {}", b.name));
        }
        self.bench_ready(id)
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
        let tiers = self.tiers_in_world(skill);
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
                .filter(|k| spirit_level >= required(k) && !self.defs.tree(*k).spirits_avoid)
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

    /// The kinds of tree a skill works on that are actually out there. Some
    /// only appear later, and there's no point announcing them before that.
    pub fn tiers_in_world(&self, skill: SkillId) -> Vec<TreeKindId> {
        let mut tiers = self.defs.tree_tiers(skill);
        tiers.retain(|k| self.trees.iter().any(|t| t.kind == *k));
        tiers
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
        self.track_regain(skill, preview.current_level, xp);
        // A level drop can make the tree being cut, or walked to, illegal.
        let illegal = self.player.actor.queue.iter().find_map(|t| match t {
            Task::Chop { tree } if !self.player_can_cut(self.trees[*tree].kind) => {
                Some(self.trees[*tree].kind)
            }
            _ => None,
        });
        if let Some(kind) = illegal {
            self.player.actor.clear_tasks();
            self.player.activity = "Idle".into();
            let name = self.defs.tree(kind).name.clone();
            self.end_session(
                SessionEnd::Blocked,
                format!("You can no longer cut {name}, so you stopped chopping."),
            );
        }
        let pos = self.player.actor.pos();
        let id = self.add_spirit(role, xp, pos);
        self.begin_story(id, xp);
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
        if self
            .tick
            .is_multiple_of(self.defs.construction.stranded_check_ticks)
        {
            self.free_the_stranded();
        }
        self.update_queue();
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
        self.update_homecoming();
        self.update_moments();
        self.update_session();
        self.update_play_log();
        self.update_storage_warning();
        self.update_journey();
        if self.tick.is_multiple_of(SNAPSHOT_TICKS) {
            self.update_ambitions();
            self.update_deeds();
            self.update_visitors();
            self.update_tips();
        }
        if self.tick.is_multiple_of(5) {
            self.update_markers();
            self.update_relics();
            self.update_left_before();
            self.update_last_helper();
        }
    }

    pub fn run_ticks(&mut self, n: u64) {
        for _ in 0..n {
            self.step();
        }
    }

    fn regrow_trees(&mut self) {
        self.update_season();
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
        player_recipe: Option<RecipeId>,
        player_room: Option<(BuildingId, BTreeMap<ResourceId, u32>)>,
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
            player_recipe,
            player_room,
        }
    }

    fn step_player(&mut self, events: &mut Vec<SimEvent>) {
        let level = self.player_task_level();
        let at_station = self.player_output_at_station();
        let room =
            matches!(self.player.actor.queue.front(), Some(Task::Deposit { .. })).then(|| {
                let carried = self.player.actor.inventory.items().keys();
                let limits = carried.map(|r| (*r, self.player_room_for(*r))).collect();
                (self.main_storage, limits)
            });
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
        let recipe = player.recipe;
        let mut ctx = Self::task_ctx(
            defs, grid, trees, buildings, projects, ledger, *tick, events, at_station, recipe, room,
        );
        let mut failure = None;
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
                // A spirit finishing the tree isn't a reason to stop.
                let gone = matches!(
                    player.actor.queue.front(),
                    Some(Task::Chop { tree }) if !trees[*tree].is_grown()
                );
                player.actor.clear_tasks();
                player.activity = reason.clone();
                if !gone {
                    failure = Some(reason);
                }
            }
        }
        if let Some(reason) = failure {
            self.end_session(SessionEnd::Blocked, reason);
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
            if self.spirits[sid].aboard {
                continue;
            }
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
                defs, grid, trees, buildings, projects, ledger, *tick, events, false, None, None,
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

    pub(crate) fn player_earned_xp(&mut self, skill: SkillId, xp: u64) {
        self.ledger.xp_earned += xp;
        *self.player.log.xp_earned.entry(skill).or_default() += xp;
        if let Some(s) = &mut self.player.session {
            s.xp += xp;
        }
        self.give_player_xp(skill, xp);
    }

    /// XP a spirit gives back when it goes home. It was the player's before,
    /// so it goes in its own column rather than counting as earned again.
    pub(crate) fn player_regained_xp(&mut self, skill: SkillId, xp: u64) {
        self.ledger.xp_returned += xp;
        self.give_player_xp(skill, xp);
    }

    /// Adds XP and says what that opens up.
    fn give_player_xp(&mut self, skill: SkillId, xp: u64) {
        let table = &self.defs.xp_table;
        let before = self.player.level(skill, table);
        let highest_before = self.player.history(skill).highest_level;
        self.player.add_xp(skill, xp, &self.defs.xp_table);
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
            .tiers_in_world(skill)
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
                        if let Some(s) = &mut self.player.session {
                            s.units += 1;
                        }
                        self.player_earned_xp(self.defs.tree(self.trees[tree].kind).skill, xp)
                    }
                    // Spirits harvest but never gain experience.
                    Who::Spirit(sid) => {
                        self.spirits[sid].units_gathered += 1;
                        if self.player.session.is_some() {
                            self.player.log.spirit_gathered += 1;
                        }
                    }
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
                self.spirits[sid].trips += 1;
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
                    Who::Player => {
                        if let Some(s) = &mut self.player.session {
                            s.units += amount;
                        }
                        self.player_earned_xp(skill, xp)
                    }
                    // Spirits produce but never gain experience.
                    Who::Spirit(sid) => {
                        self.spirits[sid].units_crafted += amount;
                        if self.player.session.is_some() {
                            self.player.log.spirit_crafted += amount;
                        }
                    }
                }
            }
        }
    }
}
