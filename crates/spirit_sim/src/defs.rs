//! Data-driven game definitions loaded from `data/defs.toml`.
//!
//! The TOML is parsed into `Raw*` structs that use string ids, then resolved
//! into [`GameDefs`], which uses compact index ids and simulation ticks.

use crate::skills::XpTable;
use serde::Deserialize;
use std::collections::BTreeMap;

pub const BUILTIN_DEFS: &str = include_str!("../../../data/defs.toml");

macro_rules! id_type {
    ($($name:ident),*) => {$(
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(pub u16);
        impl $name {
            pub fn index(self) -> usize {
                self.0 as usize
            }
        }
    )*};
}
id_type!(
    ResourceId,
    SkillId,
    RoleId,
    BuildingKindId,
    TreeKindId,
    SiteId,
    RecipeId
);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    Gather,
    Deliver,
    Haul,
    Construct,
    /// Process materials at a station.
    Craft,
    /// Carry raw materials from Main Storage to a station.
    Supply,
    /// Carry finished goods from a station to Main Storage.
    Collect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Condition {
    Always,
    InventoryFull,
    CarryingAny,
    ObjectiveUnmet,
    OutpostStockAvailable,
    ProjectAssigned,
    /// This spirit is assigned and the production order is short of its goal.
    ProductionWanted,
    WorkshopNeedsInput,
    WorkshopOutputReady,
    /// Finished goods are ready and no hauler exists to take them.
    OutputUnattended,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliverPolicy {
    /// Nearest completed storage that accepts gatherer deposits.
    Nearest,
    /// Always the main settlement storage.
    Main,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub struct JobRule {
    pub job: JobKind,
    pub priority: i32,
    pub when: Condition,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ResourceDef {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SkillDef {
    pub id: String,
    pub name: String,
    pub trainable: bool,
}

#[derive(Clone, Debug)]
pub struct TreeTypeDef {
    pub id: String,
    pub name: String,
    pub map_char: char,
    pub resource: ResourceId,
    pub skill: SkillId,
    /// Minimum skill level to cut this tree. Applies to the player and spirits.
    pub level_required: u32,
    pub logs: u32,
    pub regrow_ticks: u64,
    pub xp_per_log: u64,
    pub chop_ticks: u32,
    pub chop_ticks_per_level_x10: u32,
    pub min_chop_ticks: u32,
}

impl TreeTypeDef {
    /// Ticks needed to cut one log at the given skill level.
    pub fn chop_ticks_at(&self, level: u32) -> u32 {
        let reduction = level.saturating_sub(1) * self.chop_ticks_per_level_x10 / 10;
        self.chop_ticks
            .saturating_sub(reduction)
            .max(self.min_chop_ticks)
            .max(1)
    }
}

/// Timed processing at a station: consumes `inputs`, creates `output`.
#[derive(Clone, Debug)]
pub struct RecipeDef {
    pub id: String,
    pub name: String,
    pub skill: SkillId,
    pub station: BuildingKindId,
    /// Minimum skill level. Applies to the player and spirits.
    pub level_required: u32,
    pub inputs: Vec<(ResourceId, u32)>,
    pub output: (ResourceId, u32),
    pub craft_ticks: u32,
    pub craft_ticks_per_level_x10: u32,
    pub min_craft_ticks: u32,
    pub xp: u64,
}

impl RecipeDef {
    /// Ticks needed for one batch at the given skill level.
    pub fn craft_ticks_at(&self, level: u32) -> u32 {
        let reduction = level.saturating_sub(1) * self.craft_ticks_per_level_x10 / 10;
        self.craft_ticks
            .saturating_sub(reduction)
            .max(self.min_craft_ticks)
            .max(1)
    }
}

/// Settlement production settings that aren't player-adjustable.
#[derive(Clone, Debug)]
pub struct ProductionRules {
    pub output: ResourceId,
    /// Most extra input a production shortfall may request at once.
    pub upstream_cap: u32,
}

/// A named, permanent gathering location: every tree inside `area` belongs to it.
#[derive(Clone, Debug)]
pub struct SiteDef {
    pub id: String,
    pub name: String,
    /// Inclusive tile bounds [x0, y0, x1, y1].
    pub area: [i32; 4],
    /// Regrowth time of trees here, as a percentage of the tree type's time.
    pub regrow_pct: u32,
}

impl SiteDef {
    pub fn contains(&self, x: i32, y: i32) -> bool {
        let [x0, y0, x1, y1] = self.area;
        (x0..=x1).contains(&x) && (y0..=y1).contains(&y)
    }

    pub fn center(&self) -> (f32, f32) {
        let [x0, y0, x1, y1] = self.area;
        ((x0 + x1) as f32 / 2.0, (y0 + y1) as f32 / 2.0)
    }
}

/// Measurement and gathering-behavior tuning.
#[derive(Clone, Debug)]
pub struct GatheringRules {
    /// Rolling window for measured throughput and time use.
    pub window_ticks: u64,
}

#[derive(Clone, Debug)]
pub struct PlayerRules {
    pub move_ticks: u32,
    pub carry_capacity: u32,
}

#[derive(Clone, Debug)]
pub struct RoleDef {
    pub id: String,
    pub name: String,
    pub skill: SkillId,
    pub from_soul_separation: bool,
    pub move_ticks: u32,
    pub carry_capacity: u32,
    pub capacity_per_level: u32,
    pub deliver_to: DeliverPolicy,
    pub haul_min: u32,
    pub color: [f32; 3],
    /// Sorted by descending priority.
    pub jobs: Vec<JobRule>,
}

impl RoleDef {
    pub fn has_job(&self, job: JobKind) -> bool {
        self.jobs.iter().any(|j| j.job == job)
    }

    pub fn capacity_at(&self, level: u32) -> u32 {
        self.carry_capacity + self.capacity_per_level * level.saturating_sub(1)
    }
}

#[derive(Clone, Debug)]
pub struct BuildingDef {
    pub id: String,
    pub name: String,
    pub map_char: Option<char>,
    pub is_main: bool,
    pub capacity: u32,
    pub accepts_gatherer_deposits: bool,
    pub autonomous: bool,
    pub max_count: Option<u32>,
    pub cost: Vec<(ResourceId, u32)>,
    pub build_ticks: u32,
    pub build_ticks_per_level_x10: u32,
    pub min_build_ticks: u32,
    pub color: [f32; 3],
    /// Recipes can be made here.
    pub is_station: bool,
    /// A station's limit on stored raw materials.
    pub input_buffer: u32,
    /// Added to Main Storage's capacity when this building is completed.
    pub capacity_bonus: u32,
    /// Built on the player's request.
    pub manual: bool,
}

impl BuildingDef {
    pub fn build_ticks_at(&self, level: u32) -> u32 {
        let reduction = level.saturating_sub(1) * self.build_ticks_per_level_x10 / 10;
        self.build_ticks
            .saturating_sub(reduction)
            .max(self.min_build_ticks)
            .max(1)
    }
}

#[derive(Clone, Debug)]
pub struct ObjectiveDef {
    pub resource: ResourceId,
    pub target: u32,
}

#[derive(Clone, Debug)]
pub struct PlannerRules {
    pub eval_interval_ticks: u64,
    pub delivery_window: usize,
    pub max_sample_age_ticks: u64,
    pub travel_share_threshold_pct: u32,
    pub min_leg_tiles: u32,
    pub min_savings_pct: u32,
    pub site_search_radius: i32,
    pub reject_cooldown_ticks: u64,
}

#[derive(Clone, Debug)]
pub struct GameDefs {
    pub ticks_per_second: u32,
    pub idle_rethink_ticks: u64,
    pub event_log_capacity: usize,
    pub xp_table: XpTable,
    pub resources: Vec<ResourceDef>,
    pub skills: Vec<SkillDef>,
    pub trees: Vec<TreeTypeDef>,
    pub recipes: Vec<RecipeDef>,
    pub production: ProductionRules,
    pub sites: Vec<SiteDef>,
    pub gathering: GatheringRules,
    pub player: PlayerRules,
    pub soul_min_xp: u64,
    pub roles: Vec<RoleDef>,
    pub buildings: Vec<BuildingDef>,
    pub objective: ObjectiveDef,
    pub planner: PlannerRules,
}

// ---------------------------------------------------------------------------
// Raw TOML schema
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct RawDefs {
    simulation: RawSimulation,
    progression: RawProgression,
    resources: Vec<ResourceDef>,
    skills: Vec<SkillDef>,
    tree_types: Vec<RawTreeType>,
    #[serde(default)]
    recipes: Vec<RawRecipe>,
    production: RawProduction,
    #[serde(default)]
    sites: Vec<RawSite>,
    gathering: RawGathering,
    player: RawPlayer,
    soul_separation: RawSoul,
    roles: Vec<RawRole>,
    buildings: Vec<RawBuilding>,
    objective: RawObjective,
    planner: RawPlanner,
}

#[derive(Deserialize)]
struct RawSimulation {
    ticks_per_second: u32,
    idle_rethink_seconds: f32,
    event_log_capacity: usize,
}

#[derive(Deserialize)]
struct RawProgression {
    xp_table: Vec<u64>,
}

#[derive(Deserialize)]
struct RawTreeType {
    id: String,
    name: String,
    map_char: String,
    resource: String,
    skill: String,
    #[serde(default = "one")]
    level_required: u32,
    logs: u32,
    regrow_seconds: f32,
    xp_per_log: u64,
    chop_seconds: f32,
    chop_seconds_per_level: f32,
    min_chop_seconds: f32,
}

#[derive(Deserialize)]
struct RawRecipe {
    id: String,
    name: String,
    skill: String,
    station: String,
    #[serde(default = "one")]
    level_required: u32,
    inputs: Vec<RawCost>,
    output: RawCost,
    craft_seconds: f32,
    #[serde(default)]
    craft_seconds_per_level: f32,
    min_craft_seconds: f32,
    xp: u64,
}

#[derive(Deserialize)]
struct RawProduction {
    output: String,
    upstream_cap: u32,
}

#[derive(Deserialize)]
struct RawSite {
    id: String,
    name: String,
    area: [i32; 4],
    #[serde(default = "hundred")]
    regrow_pct: u32,
}

#[derive(Deserialize)]
struct RawGathering {
    measure_window_seconds: f32,
}

fn one() -> u32 {
    1
}

fn hundred() -> u32 {
    100
}

#[derive(Deserialize)]
struct RawPlayer {
    move_seconds_per_tile: f32,
    carry_capacity: u32,
}

#[derive(Deserialize)]
struct RawSoul {
    min_xp: u64,
}

#[derive(Deserialize)]
struct RawRole {
    id: String,
    name: String,
    skill: String,
    #[serde(default)]
    from_soul_separation: bool,
    move_seconds_per_tile: f32,
    carry_capacity: u32,
    #[serde(default)]
    capacity_per_level: u32,
    deliver_to: DeliverPolicy,
    #[serde(default)]
    haul_min: u32,
    #[serde(default = "default_color")]
    color: [f32; 3],
    jobs: Vec<JobRule>,
}

#[derive(Deserialize)]
struct RawCost {
    resource: String,
    amount: u32,
}

#[derive(Deserialize)]
struct RawBuilding {
    id: String,
    name: String,
    #[serde(default)]
    map_char: Option<String>,
    #[serde(default)]
    is_main: bool,
    capacity: u32,
    accepts_gatherer_deposits: bool,
    #[serde(default)]
    autonomous: bool,
    #[serde(default)]
    max_count: Option<u32>,
    #[serde(default)]
    cost: Vec<RawCost>,
    #[serde(default)]
    build_seconds: f32,
    #[serde(default)]
    build_seconds_per_level: f32,
    #[serde(default)]
    min_build_seconds: f32,
    #[serde(default = "default_color")]
    color: [f32; 3],
    #[serde(default)]
    input_buffer: u32,
    #[serde(default)]
    capacity_bonus: u32,
    #[serde(default)]
    manual: bool,
}

#[derive(Deserialize)]
struct RawObjective {
    resource: String,
    target: u32,
}

#[derive(Deserialize)]
struct RawPlanner {
    eval_interval_seconds: f32,
    delivery_window: usize,
    max_sample_age_seconds: f32,
    travel_share_threshold_pct: u32,
    min_leg_tiles: u32,
    min_savings_pct: u32,
    site_search_radius: i32,
    reject_cooldown_seconds: f32,
}

fn default_color() -> [f32; 3] {
    [0.8, 0.8, 0.8]
}

fn single_char(s: &str, what: &str) -> Result<char, String> {
    let mut chars = s.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => Ok(c),
        _ => Err(format!(
            "{what}: map_char must be exactly one character, got {s:?}"
        )),
    }
}

impl GameDefs {
    /// The definitions shipped in `data/defs.toml`, embedded at compile time.
    pub fn builtin() -> Self {
        Self::from_toml(BUILTIN_DEFS).expect("built-in data/defs.toml must be valid")
    }

    pub fn from_toml(text: &str) -> Result<Self, String> {
        let raw: RawDefs = toml::from_str(text).map_err(|e| format!("defs.toml: {e}"))?;
        let tps = raw.simulation.ticks_per_second.max(1);
        let ticks = |secs: f32| -> u64 { (secs * tps as f32).round().max(0.0) as u64 };
        // Per-level reductions are kept in tenths of a tick to avoid floats in the sim.
        let tenth_ticks = |secs: f32| -> u32 { (secs * tps as f32 * 10.0).round().max(0.0) as u32 };

        let xp_table = XpTable::new(raw.progression.xp_table)?;
        let resources = raw.resources;
        let skills = raw.skills;

        let resource_id = |name: &str| -> Result<ResourceId, String> {
            resources
                .iter()
                .position(|r| r.id == name)
                .map(|i| ResourceId(i as u16))
                .ok_or_else(|| format!("unknown resource {name:?}"))
        };
        let skill_id = |name: &str| -> Result<SkillId, String> {
            skills
                .iter()
                .position(|s| s.id == name)
                .map(|i| SkillId(i as u16))
                .ok_or_else(|| format!("unknown skill {name:?}"))
        };

        let mut trees = Vec::new();
        for t in raw.tree_types {
            trees.push(TreeTypeDef {
                map_char: single_char(&t.map_char, &t.id)?,
                resource: resource_id(&t.resource)?,
                skill: skill_id(&t.skill)?,
                level_required: t.level_required.max(1),
                logs: t.logs.max(1),
                regrow_ticks: ticks(t.regrow_seconds),
                xp_per_log: t.xp_per_log,
                chop_ticks: ticks(t.chop_seconds) as u32,
                chop_ticks_per_level_x10: tenth_ticks(t.chop_seconds_per_level),
                min_chop_ticks: ticks(t.min_chop_seconds) as u32,
                id: t.id,
                name: t.name,
            });
        }

        let sites: Vec<SiteDef> = raw
            .sites
            .into_iter()
            .map(|s| SiteDef {
                id: s.id,
                name: s.name,
                area: s.area,
                regrow_pct: s.regrow_pct.max(1),
            })
            .collect();
        let gathering = GatheringRules {
            window_ticks: ticks(raw.gathering.measure_window_seconds).max(1),
        };

        let mut roles = Vec::new();
        for r in raw.roles {
            if r.jobs.is_empty() {
                return Err(format!("role {:?} has no jobs", r.id));
            }
            let mut jobs = r.jobs;
            // Stable sort keeps file order for equal priorities.
            jobs.sort_by_key(|j| std::cmp::Reverse(j.priority));
            roles.push(RoleDef {
                skill: skill_id(&r.skill)?,
                from_soul_separation: r.from_soul_separation,
                move_ticks: (ticks(r.move_seconds_per_tile) as u32).max(1),
                carry_capacity: r.carry_capacity.max(1),
                capacity_per_level: r.capacity_per_level,
                deliver_to: r.deliver_to,
                haul_min: r.haul_min.max(1),
                color: r.color,
                jobs,
                id: r.id,
                name: r.name,
            });
        }

        let mut buildings = Vec::new();
        for b in raw.buildings {
            let mut cost = Vec::new();
            for c in &b.cost {
                cost.push((resource_id(&c.resource)?, c.amount));
            }
            buildings.push(BuildingDef {
                map_char: b
                    .map_char
                    .as_deref()
                    .map(|c| single_char(c, &b.id))
                    .transpose()?,
                is_main: b.is_main,
                capacity: b.capacity,
                accepts_gatherer_deposits: b.accepts_gatherer_deposits,
                autonomous: b.autonomous,
                max_count: b.max_count,
                cost,
                build_ticks: ticks(b.build_seconds) as u32,
                build_ticks_per_level_x10: tenth_ticks(b.build_seconds_per_level),
                min_build_ticks: ticks(b.min_build_seconds) as u32,
                color: b.color,
                is_station: false,
                input_buffer: b.input_buffer,
                capacity_bonus: b.capacity_bonus,
                manual: b.manual,
                id: b.id,
                name: b.name,
            });
        }
        if buildings.iter().filter(|b| b.is_main).count() != 1 {
            return Err("exactly one building must have is_main = true".into());
        }

        let mut recipes = Vec::new();
        for r in raw.recipes {
            let station = buildings
                .iter()
                .position(|b| b.id == r.station)
                .ok_or_else(|| format!("recipe {:?}: unknown station {:?}", r.id, r.station))?;
            buildings[station].is_station = true;
            if r.inputs.is_empty() {
                return Err(format!("recipe {:?} has no inputs", r.id));
            }
            let mut inputs = Vec::new();
            for c in &r.inputs {
                inputs.push((resource_id(&c.resource)?, c.amount.max(1)));
            }
            recipes.push(RecipeDef {
                skill: skill_id(&r.skill)?,
                station: BuildingKindId(station as u16),
                level_required: r.level_required.max(1),
                inputs,
                output: (resource_id(&r.output.resource)?, r.output.amount.max(1)),
                craft_ticks: ticks(r.craft_seconds) as u32,
                craft_ticks_per_level_x10: tenth_ticks(r.craft_seconds_per_level),
                min_craft_ticks: ticks(r.min_craft_seconds) as u32,
                xp: r.xp,
                id: r.id,
                name: r.name,
            });
        }
        let production = ProductionRules {
            output: resource_id(&raw.production.output)?,
            upstream_cap: raw.production.upstream_cap,
        };

        let objective = ObjectiveDef {
            resource: resource_id(&raw.objective.resource)?,
            target: raw.objective.target,
        };
        let p = raw.planner;
        let planner = PlannerRules {
            eval_interval_ticks: ticks(p.eval_interval_seconds).max(1),
            delivery_window: p.delivery_window.max(1),
            max_sample_age_ticks: ticks(p.max_sample_age_seconds),
            travel_share_threshold_pct: p.travel_share_threshold_pct,
            min_leg_tiles: p.min_leg_tiles,
            min_savings_pct: p.min_savings_pct,
            site_search_radius: p.site_search_radius.max(1),
            reject_cooldown_ticks: ticks(p.reject_cooldown_seconds),
        };

        Ok(GameDefs {
            ticks_per_second: tps,
            idle_rethink_ticks: ticks(raw.simulation.idle_rethink_seconds).max(1),
            event_log_capacity: raw.simulation.event_log_capacity.max(10),
            xp_table,
            player: PlayerRules {
                move_ticks: (ticks(raw.player.move_seconds_per_tile) as u32).max(1),
                carry_capacity: raw.player.carry_capacity.max(1),
            },
            soul_min_xp: raw.soul_separation.min_xp.max(1),
            resources,
            skills,
            trees,
            recipes,
            production,
            sites,
            gathering,
            roles,
            buildings,
            objective,
            planner,
        })
    }

    pub fn resource_id(&self, id: &str) -> Option<ResourceId> {
        self.resources
            .iter()
            .position(|r| r.id == id)
            .map(|i| ResourceId(i as u16))
    }

    pub fn skill_id(&self, id: &str) -> Option<SkillId> {
        self.skills
            .iter()
            .position(|s| s.id == id)
            .map(|i| SkillId(i as u16))
    }

    pub fn role_id(&self, id: &str) -> Option<RoleId> {
        self.roles
            .iter()
            .position(|r| r.id == id)
            .map(|i| RoleId(i as u16))
    }

    pub fn building_kind_id(&self, id: &str) -> Option<BuildingKindId> {
        self.buildings
            .iter()
            .position(|b| b.id == id)
            .map(|i| BuildingKindId(i as u16))
    }

    pub fn resource(&self, id: ResourceId) -> &ResourceDef {
        &self.resources[id.index()]
    }

    pub fn skill(&self, id: SkillId) -> &SkillDef {
        &self.skills[id.index()]
    }

    pub fn role(&self, id: RoleId) -> &RoleDef {
        &self.roles[id.index()]
    }

    pub fn building(&self, id: BuildingKindId) -> &BuildingDef {
        &self.buildings[id.index()]
    }

    pub fn tree(&self, id: TreeKindId) -> &TreeTypeDef {
        &self.trees[id.index()]
    }

    pub fn recipe(&self, id: RecipeId) -> &RecipeDef {
        &self.recipes[id.index()]
    }

    /// Recipes of `skill`, ordered by level requirement.
    pub fn recipe_tiers(&self, skill: SkillId) -> Vec<RecipeId> {
        let mut ids: Vec<RecipeId> = (0..self.recipes.len())
            .map(|i| RecipeId(i as u16))
            .filter(|&r| self.recipe(r).skill == skill)
            .collect();
        ids.sort_by_key(|&r| self.recipe(r).level_required);
        ids
    }

    /// Recipes made at a station type, ordered by level requirement.
    pub fn station_recipes(&self, station: BuildingKindId) -> Vec<RecipeId> {
        let mut ids: Vec<RecipeId> = (0..self.recipes.len())
            .map(|i| RecipeId(i as u16))
            .filter(|&r| self.recipe(r).station == station)
            .collect();
        ids.sort_by_key(|&r| self.recipe(r).level_required);
        ids
    }

    /// The highest-level recipe at `station` that `level` allows and the
    /// materials reported by `have` can pay for.
    pub fn best_recipe(
        &self,
        station: BuildingKindId,
        level: u32,
        have: impl Fn(ResourceId) -> u32,
    ) -> Option<RecipeId> {
        self.station_recipes(station).into_iter().rev().find(|&r| {
            let def = self.recipe(r);
            level >= def.level_required && def.inputs.iter().all(|&(res, n)| have(res) >= n)
        })
    }

    pub fn main_building_kind(&self) -> BuildingKindId {
        BuildingKindId(self.buildings.iter().position(|b| b.is_main).unwrap() as u16)
    }

    /// The spirit role created when the player separates XP from `skill`.
    pub fn soul_role_for(&self, skill: SkillId) -> Option<RoleId> {
        self.roles
            .iter()
            .position(|r| r.from_soul_separation && r.skill == skill)
            .map(|i| RoleId(i as u16))
    }

    pub fn site_id(&self, id: &str) -> Option<SiteId> {
        self.sites
            .iter()
            .position(|s| s.id == id)
            .map(|i| SiteId(i as u16))
    }

    pub fn site(&self, id: SiteId) -> &SiteDef {
        &self.sites[id.index()]
    }

    /// The site whose area contains a tile, if any.
    pub fn site_at(&self, x: i32, y: i32) -> Option<SiteId> {
        self.sites
            .iter()
            .position(|s| s.contains(x, y))
            .map(|i| SiteId(i as u16))
    }

    /// Tree types of `skill`, ordered by level requirement.
    pub fn tree_tiers(&self, skill: SkillId) -> Vec<TreeKindId> {
        let mut kinds: Vec<TreeKindId> = (0..self.trees.len())
            .map(|i| TreeKindId(i as u16))
            .filter(|&k| self.tree(k).skill == skill)
            .collect();
        kinds.sort_by_key(|&k| self.tree(k).level_required);
        kinds
    }

    pub fn tree_kind_for_char(&self, c: char) -> Option<TreeKindId> {
        self.trees
            .iter()
            .position(|t| t.map_char == c)
            .map(|i| TreeKindId(i as u16))
    }

    pub fn building_kind_for_char(&self, c: char) -> Option<BuildingKindId> {
        self.buildings
            .iter()
            .position(|b| b.map_char == Some(c))
            .map(|i| BuildingKindId(i as u16))
    }

    /// Formats a resource bundle like "15 Logs".
    pub fn describe_amounts(&self, amounts: &BTreeMap<ResourceId, u32>) -> String {
        if amounts.is_empty() {
            return "nothing".into();
        }
        amounts
            .iter()
            .map(|(r, n)| format!("{n} {}", self.resource(*r).name))
            .collect::<Vec<_>>()
            .join(", ")
    }
}
