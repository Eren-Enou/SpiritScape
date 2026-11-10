//! Data-driven game definitions loaded from `data/defs.toml`.
//!
//! The TOML is parsed into `Raw*` structs that use string ids, then resolved
//! into [`GameDefs`], which uses compact index ids and simulation ticks.

use crate::skills::XpTable;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const BUILTIN_DEFS: &str = include_str!("../../../data/defs.toml");

macro_rules! id_type {
    ($($name:ident),*) => {$(
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
    /// Empty the player's Training Cache into Main Storage.
    TrainingCollect,
    /// Carry raw materials to a station for the player's own training.
    TrainingSupply,
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
    /// The Training Cache has a load waiting and training comes first.
    TrainingPickupUrgent,
    /// The Training Cache has a load waiting.
    TrainingPickupReady,
    /// The bench is short of training materials and training comes first.
    TrainingSupplyUrgent,
    /// The bench is short of training materials.
    TrainingSupplyWanted,
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
    /// Follows the seasons: grows faster in spring, and stops in winter.
    pub seasonal: bool,
    /// No spirit will cut this.
    pub spirits_avoid: bool,
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
    /// Why this tree matters, for the moment it's unlocked.
    pub why: String,
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
    /// Why this recipe matters, for the moment it's unlocked.
    pub why: String,
    /// Made only when someone asks for it by name.
    pub chosen_only: bool,
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

/// When a long walk is worth it.
#[derive(Clone, Debug)]
pub struct LogisticsRules {
    /// A trip this many tiles or longer counts as a long one.
    pub long_trip_tiles: u32,
    /// Batches of material a far bench must hold before a free worker walks there.
    pub long_trip_batches: u32,
    /// Share of a carrier's load, in percent, worth a long trip.
    pub long_trip_load_pct: u32,
    /// Goods this close to a pickup can join the same trip.
    pub batch_tiles: u32,
    /// Carriers weigh jobs by load and distance. Off, they take the job rules
    /// strictly in order, which is how the difference is measured.
    pub scored_carrying: bool,
}

/// Limits on the construction plan.
#[derive(Clone, Debug)]
pub struct ConstructionRules {
    /// Buildings that can wait on the plan at once.
    pub queue_limit: u32,
    /// An annex works with a bench this many tiles away.
    pub annex_reach: u32,
    /// Room in Main Storage that surplus finished goods can't use.
    pub raw_room: u32,
    /// How long a road tile takes, as a percentage of open ground.
    pub road_pct: u32,
    /// How long a visitor waits for an answer.
    pub visitor_patience_minutes: u64,
    /// The least time between one visitor arriving and the next.
    pub visitor_gap_minutes: u64,
    /// How long spirits gather when the Hearth is lit.
    pub hearth_gathering_ticks: u64,
    /// Time between one spirit boarding and the next.
    pub homecoming_gap_ticks: u64,
    /// How long anyone gets to reach the boat.
    pub homecoming_limit_ticks: u64,
    /// Time between one spirit doing something small and the next.
    pub moment_ticks: u64,
    /// How long that stays on the map.
    pub moment_shown_ticks: u64,
    /// The water that freezes in winter, as [x0, y0, x1, y1].
    pub ice_area: [i32; 4],
    /// How long before spring the ice starts to warn the player.
    pub thaw_warning_ticks: u64,
    /// How often to check that nobody's been walled in.
    pub stranded_check_ticks: u64,
}

/// Settlement production settings that aren't player-adjustable.
#[derive(Clone, Debug)]
pub struct ProductionRules {
    pub output: ResourceId,
    /// Further products with an order of their own.
    pub more_outputs: Vec<ResourceId>,
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
    /// Can be run as a remote production site.
    pub outpost: bool,
    /// The site this one is reached through. Spirits posted there work here too.
    pub region: Option<SiteId>,
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
    /// Farthest walk, in tiles, to the next tree of a gathering session.
    pub gather_reach_tiles: u32,
    /// Longest a gathering session waits for a tree to regrow.
    pub gather_wait_ticks: u64,
    /// Longest a crafting session waits for spirits to bring what it needs.
    pub craft_wait_ticks: u64,
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
    /// More that every spirit can carry once this stands.
    pub carry_bonus: u32,
    /// Leaves when it's finished, so it can be built again and again.
    pub repeatable: bool,
    /// Finishing this ends the story, though the game carries on.
    pub ending: bool,
    /// Trade Caravans that must have gone before this can be planned.
    pub needs_caravans: u32,
    /// What the caravan that opens this brings back, in words.
    pub caravan_find: String,
    /// Trees or outcrops added to the map when this is finished.
    pub plants: Vec<(TreeKindId, [i32; 2])>,
    /// Built on the player's request.
    pub manual: bool,
    /// Room for materials set aside for the player's training at a station.
    pub training_buffer: u32,
    /// The player's drop-off point in their training area.
    pub training_cache: bool,
    /// Storage beyond `capacity` for further products at a station.
    pub extra_capacity: u32,
    /// Workers that fit at a station at once.
    pub slots: u32,
    /// Room added at stations for logs set aside for the player.
    pub training_bonus: u32,
    /// Keeps seasonal things recovering through winter, at half speed.
    pub winter_growth: bool,
    /// What the closing story says if this stands.
    pub ending_line: Option<String>,
    /// Spirits can be called home to this.
    pub calls_home: bool,
    /// Where this must be built, when it's tied to one place.
    pub access_site: Option<[i32; 2]>,
    /// Tiles that become a crossing when this is finished.
    pub opens: Vec<[i32; 2]>,
    /// Tiles that become road when this is finished, in order from its start.
    pub paves: Vec<[i32; 2]>,
    /// The site that crossing reaches.
    pub unlocks_site: Option<String>,
    /// Buildings that must be finished first.
    pub requires: Vec<String>,
    /// Places added at the nearest bench.
    pub slot_bonus: u32,
    /// What changes when this is finished, in a sentence.
    pub benefit: String,
    /// The site it goes up in when the player doesn't choose a tile.
    pub home_site: Option<String>,
}

impl BuildingDef {
    /// Storage a new building of this kind starts with.
    pub fn storage_capacity(&self) -> u32 {
        self.capacity + self.training_buffer + self.extra_capacity
    }

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

/// A selectable pace of personal progression.
#[derive(Clone, Debug, Deserialize)]
pub struct ProfileInfo {
    pub id: String,
    pub title: String,
    pub description: String,
}

/// When the new-game guidance moves on.
#[derive(Clone, Copy, Debug, Deserialize)]
pub struct JourneyRules {
    pub first_spirit_level: u32,
    pub woodworking_level: u32,
}

#[derive(Clone, Debug)]
pub struct GameDefs {
    /// Long-term goals, in the order they're suggested.
    pub ambitions: Vec<crate::ambitions::AmbitionDef>,
    /// What to say as each system first matters.
    pub tips: Vec<crate::tips::TipDef>,
    /// People who pass through, in the order they come.
    pub visitors: Vec<crate::visitors::VisitorDef>,
    /// The year, in order.
    pub seasons: Vec<crate::stories::SeasonDef>,
    /// Old stones to find around the map.
    pub markers: Vec<crate::stories::MarkerDef>,
    /// What spirits made by Soul Separation can be like.
    pub traits: Vec<crate::stories::TraitDef>,
    /// Milestones in a spirit's working life.
    pub deeds: Vec<crate::stories::DeedDef>,
    pub moments: Vec<crate::moments::MomentDef>,
    /// The progression profile these definitions were resolved with.
    pub profile: ProfileInfo,
    /// Every profile in the data file.
    pub profiles: Vec<ProfileInfo>,
    pub journey: JourneyRules,
    /// Real seconds between autosaves; 0 turns autosave off.
    pub autosave_seconds: f32,
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
    pub logistics: LogisticsRules,
    pub construction: ConstructionRules,
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
    saves: RawSaves,
    progression: RawProgression,
    journey: JourneyRules,
    #[serde(default)]
    ambitions: Vec<crate::ambitions::AmbitionDef>,
    #[serde(default)]
    tips: Vec<crate::tips::TipDef>,
    #[serde(default)]
    visitors: Vec<crate::visitors::VisitorDef>,
    #[serde(default)]
    seasons: Vec<crate::stories::SeasonDef>,
    #[serde(default)]
    markers: Vec<crate::stories::MarkerDef>,
    #[serde(default)]
    traits: Vec<crate::stories::TraitDef>,
    #[serde(default)]
    deeds: Vec<crate::stories::DeedDef>,
    #[serde(default)]
    moments: Vec<crate::moments::MomentDef>,
    resources: Vec<ResourceDef>,
    skills: Vec<SkillDef>,
    tree_types: Vec<RawTreeType>,
    #[serde(default)]
    recipes: Vec<RawRecipe>,
    production: RawProduction,
    #[serde(default)]
    sites: Vec<RawSite>,
    gathering: RawGathering,
    logistics: RawLogistics,
    construction: RawConstruction,
    player: RawPlayer,
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
struct RawSaves {
    autosave_seconds: f32,
}

#[derive(Deserialize)]
struct RawProgression {
    default_profile: String,
    profiles: Vec<RawProfile>,
}

#[derive(Clone, Deserialize)]
struct RawProfile {
    id: String,
    title: String,
    description: String,
    soul_min_xp: u64,
    /// Cost of buildings that expand the settlement, as a percentage.
    #[serde(default = "hundred")]
    improvement_cost_pct: u32,
    xp_table: Vec<u64>,
}

#[derive(Deserialize)]
struct RawTreeType {
    id: String,
    name: String,
    map_char: String,
    #[serde(default)]
    seasonal: bool,
    #[serde(default)]
    spirits_avoid: bool,
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
    #[serde(default)]
    why: String,
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
    #[serde(default)]
    why: String,
    #[serde(default)]
    chosen_only: bool,
}

#[derive(Deserialize)]
struct RawProduction {
    output: String,
    #[serde(default)]
    more_outputs: Vec<String>,
    upstream_cap: u32,
}

#[derive(Deserialize)]
struct RawLogistics {
    long_trip_tiles: u32,
    long_trip_batches: u32,
    long_trip_load_pct: u32,
    batch_tiles: u32,
    scored_carrying: bool,
}

#[derive(Deserialize)]
struct RawPlant {
    tree: String,
    at: [i32; 2],
}

/// Every tile along a road's corners.
fn road_tiles(corners: &[[i32; 2]]) -> Result<Vec<[i32; 2]>, String> {
    let mut tiles: Vec<[i32; 2]> = corners.iter().take(1).copied().collect();
    for pair in corners.windows(2) {
        let ([ax, ay], [bx, by]) = (pair[0], pair[1]);
        if ax != bx && ay != by {
            return Err("a road's corners must line up".into());
        }
        let (dx, dy) = ((bx - ax).signum(), (by - ay).signum());
        let (mut x, mut y) = (ax, ay);
        while [x, y] != [bx, by] {
            x += dx;
            y += dy;
            tiles.push([x, y]);
        }
    }
    Ok(tiles)
}

#[derive(Deserialize)]
struct RawConstruction {
    queue_limit: u32,
    annex_reach: u32,
    raw_room: u32,
    road_pct: u32,
    visitor_gap_minutes: u64,
    visitor_patience_minutes: u64,
    hearth_gathering_seconds: f32,
    homecoming_gap_seconds: f32,
    homecoming_limit_seconds: f32,
    moment_seconds: f32,
    moment_shown_seconds: f32,
    ice_area: [i32; 4],
    thaw_warning_seconds: f32,
    stranded_check_seconds: f32,
}

#[derive(Deserialize)]
struct RawSite {
    id: String,
    name: String,
    area: [i32; 4],
    #[serde(default = "hundred")]
    regrow_pct: u32,
    #[serde(default)]
    outpost: bool,
    #[serde(default)]
    region: Option<String>,
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
    gather_reach_tiles: u32,
    gather_wait_seconds: f32,
    craft_wait_seconds: f32,
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
    #[serde(default)]
    training_buffer: u32,
    #[serde(default)]
    training_cache: bool,
    #[serde(default)]
    extra_capacity: u32,
    #[serde(default = "one")]
    slots: u32,
    #[serde(default)]
    training_bonus: u32,
    #[serde(default)]
    winter_growth: bool,
    #[serde(default)]
    ending_line: Option<String>,
    #[serde(default)]
    calls_home: bool,
    #[serde(default)]
    carry_bonus: u32,
    #[serde(default)]
    repeatable: bool,
    #[serde(default)]
    ending: bool,
    #[serde(default)]
    needs_caravans: u32,
    #[serde(default)]
    caravan_find: String,
    #[serde(default)]
    plants: Vec<RawPlant>,
    #[serde(default)]
    access_site: Option<[i32; 2]>,
    #[serde(default)]
    opens: Vec<[i32; 2]>,
    /// Corners of a road, joined by straight lines.
    #[serde(default)]
    paves: Vec<[i32; 2]>,
    #[serde(default)]
    unlocks_site: Option<String>,
    #[serde(default)]
    requires: Vec<String>,
    #[serde(default)]
    slot_bonus: u32,
    #[serde(default)]
    benefit: String,
    #[serde(default)]
    home_site: Option<String>,
    /// Finished goods in the cost follow the profile's improvement cost.
    #[serde(default)]
    scales_goods: bool,
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

/// Production planning looks one step upstream and never recurses, which is
/// only sound while no recipe's output leads back to its own input.
fn check_no_cycles(recipes: &[RecipeDef], resources: usize) -> Result<(), String> {
    fn visit(r: usize, recipes: &[RecipeDef], state: &mut [u8]) -> Result<(), String> {
        match state[r] {
            1 => return Err("recipes form a loop".into()),
            2 => return Ok(()),
            _ => state[r] = 1,
        }
        for d in recipes.iter().filter(|d| d.output.0.index() == r) {
            for (input, _) in &d.inputs {
                visit(input.index(), recipes, state)?;
            }
        }
        state[r] = 2;
        Ok(())
    }
    let mut state = vec![0u8; resources];
    (0..resources).try_for_each(|r| visit(r, recipes, &mut state))
}

impl GameDefs {
    /// The definitions shipped in `data/defs.toml`, embedded at compile time.
    pub fn builtin() -> Self {
        Self::from_toml(BUILTIN_DEFS).expect("built-in data/defs.toml must be valid")
    }

    /// The built-in definitions under a named progression profile.
    pub fn builtin_profile(profile: &str) -> Result<Self, String> {
        Self::from_toml_profile(BUILTIN_DEFS, Some(profile))
    }

    pub fn from_toml(text: &str) -> Result<Self, String> {
        Self::from_toml_profile(text, None)
    }

    /// Resolves the definitions under `profile`, or the file's default.
    pub fn from_toml_profile(text: &str, profile: Option<&str>) -> Result<Self, String> {
        let raw: RawDefs = toml::from_str(text).map_err(|e| format!("defs.toml: {e}"))?;
        let tps = raw.simulation.ticks_per_second.max(1);
        let ticks = |secs: f32| -> u64 { (secs * tps as f32).round().max(0.0) as u64 };
        // Per-level reductions are kept in tenths of a tick to avoid floats in the sim.
        let tenth_ticks = |secs: f32| -> u32 { (secs * tps as f32 * 10.0).round().max(0.0) as u32 };

        let wanted = profile.unwrap_or(&raw.progression.default_profile);
        let chosen = raw
            .progression
            .profiles
            .iter()
            .find(|p| p.id == wanted)
            .ok_or_else(|| format!("unknown progression profile {wanted:?}"))?
            .clone();
        let profiles: Vec<ProfileInfo> = raw
            .progression
            .profiles
            .iter()
            .map(|p| ProfileInfo {
                id: p.id.clone(),
                title: p.title.clone(),
                description: p.description.clone(),
            })
            .collect();
        let xp_table = XpTable::new(chosen.xp_table)?;
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
                seasonal: t.seasonal,
                spirits_avoid: t.spirits_avoid,
                resource: resource_id(&t.resource)?,
                skill: skill_id(&t.skill)?,
                level_required: t.level_required.max(1),
                logs: t.logs.max(1),
                regrow_ticks: ticks(t.regrow_seconds),
                xp_per_log: t.xp_per_log,
                chop_ticks: ticks(t.chop_seconds) as u32,
                chop_ticks_per_level_x10: tenth_ticks(t.chop_seconds_per_level),
                min_chop_ticks: ticks(t.min_chop_seconds) as u32,
                why: t.why,
                id: t.id,
                name: t.name,
            });
        }

        let site_keys: Vec<String> = raw.sites.iter().map(|s| s.id.clone()).collect();
        let sites: Vec<SiteDef> = raw
            .sites
            .into_iter()
            .map(|s| SiteDef {
                region: s
                    .region
                    .as_ref()
                    .and_then(|r| site_keys.iter().position(|k| k == r))
                    .map(|i| SiteId(i as u16)),
                id: s.id,
                name: s.name,
                area: s.area,
                regrow_pct: s.regrow_pct.max(1),
                outpost: s.outpost,
            })
            .collect();
        let gathering = GatheringRules {
            window_ticks: ticks(raw.gathering.measure_window_seconds).max(1),
        };
        let logistics = LogisticsRules {
            long_trip_tiles: raw.logistics.long_trip_tiles.max(1),
            long_trip_batches: raw.logistics.long_trip_batches,
            long_trip_load_pct: raw.logistics.long_trip_load_pct.min(100),
            batch_tiles: raw.logistics.batch_tiles,
            scored_carrying: raw.logistics.scored_carrying,
        };
        let construction = ConstructionRules {
            queue_limit: raw.construction.queue_limit.max(1),
            annex_reach: raw.construction.annex_reach,
            raw_room: raw.construction.raw_room,
            road_pct: raw.construction.road_pct.clamp(1, 100),
            visitor_gap_minutes: raw.construction.visitor_gap_minutes,
            visitor_patience_minutes: raw.construction.visitor_patience_minutes,
            hearth_gathering_ticks: ticks(raw.construction.hearth_gathering_seconds),
            homecoming_gap_ticks: ticks(raw.construction.homecoming_gap_seconds),
            homecoming_limit_ticks: ticks(raw.construction.homecoming_limit_seconds),
            moment_ticks: ticks(raw.construction.moment_seconds),
            moment_shown_ticks: ticks(raw.construction.moment_shown_seconds),
            ice_area: raw.construction.ice_area,
            thaw_warning_ticks: ticks(raw.construction.thaw_warning_seconds),
            stranded_check_ticks: ticks(raw.construction.stranded_check_seconds).max(1),
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
            let pct = if b.capacity_bonus > 0 {
                chosen.improvement_cost_pct
            } else {
                100
            };
            let mut cost = Vec::new();
            for c in &b.cost {
                let made = raw.recipes.iter().any(|r| r.output.resource == c.resource);
                let pct = if b.scales_goods && made {
                    chosen.improvement_cost_pct
                } else {
                    pct
                };
                cost.push((resource_id(&c.resource)?, c.amount * pct / 100));
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
                carry_bonus: b.carry_bonus,
                repeatable: b.repeatable,
                ending: b.ending,
                needs_caravans: b.needs_caravans,
                caravan_find: b.caravan_find,
                plants: b
                    .plants
                    .iter()
                    .map(|p| {
                        let at = trees.iter().position(|t| t.id == p.tree);
                        at.map(|i| (TreeKindId(i as u16), p.at))
                            .ok_or_else(|| format!("{:?} plants unknown {:?}", b.id, p.tree))
                    })
                    .collect::<Result<Vec<_>, _>>()?,
                manual: b.manual,
                training_buffer: b.training_buffer,
                training_cache: b.training_cache,
                extra_capacity: b.extra_capacity,
                slots: b.slots.max(1),
                training_bonus: b.training_bonus,
                winter_growth: b.winter_growth,
                ending_line: b.ending_line,
                calls_home: b.calls_home,
                access_site: b.access_site,
                opens: b.opens,
                paves: road_tiles(&b.paves).map_err(|e| format!("{:?}: {e}", b.id))?,
                unlocks_site: b.unlocks_site,
                requires: b.requires,
                slot_bonus: b.slot_bonus,
                benefit: b.benefit,
                home_site: b.home_site,
                id: b.id,
                name: b.name,
            });
        }
        for b in &buildings {
            for need in &b.requires {
                if !buildings.iter().any(|x| x.id == *need) {
                    return Err(format!("{:?} requires unknown building {need:?}", b.id));
                }
            }
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
                why: r.why,
                chosen_only: r.chosen_only,
                id: r.id,
                name: r.name,
            });
        }
        check_no_cycles(&recipes, resources.len())?;
        let more_outputs = raw
            .production
            .more_outputs
            .iter()
            .map(|r| resource_id(r))
            .collect::<Result<Vec<_>, _>>()?;
        let production = ProductionRules {
            output: resource_id(&raw.production.output)?,
            more_outputs,
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

        for a in &raw.ambitions {
            let known_building = |b: &String| buildings.iter().any(|d| d.id == *b);
            let known_skill = a.skill.iter().all(|s| skills.iter().any(|d| d.id == *s));
            let known_resource = a.made.iter().all(|r| resources.iter().any(|d| d.id == *r));
            if !a.built.iter().all(known_building) || !known_skill || !known_resource {
                return Err(format!("ambition {:?} refers to something unknown", a.id));
            }
        }

        Ok(GameDefs {
            ambitions: raw.ambitions,
            tips: raw.tips,
            visitors: raw.visitors,
            seasons: raw.seasons,
            markers: raw.markers,
            traits: raw.traits,
            deeds: raw.deeds,
            moments: raw.moments,
            profile: ProfileInfo {
                id: chosen.id,
                title: chosen.title,
                description: chosen.description,
            },
            profiles,
            journey: raw.journey,
            autosave_seconds: raw.saves.autosave_seconds.max(0.0),
            ticks_per_second: tps,
            idle_rethink_ticks: ticks(raw.simulation.idle_rethink_seconds).max(1),
            event_log_capacity: raw.simulation.event_log_capacity.max(10),
            xp_table,
            player: PlayerRules {
                move_ticks: (ticks(raw.player.move_seconds_per_tile) as u32).max(1),
                carry_capacity: raw.player.carry_capacity.max(1),
                gather_reach_tiles: raw.player.gather_reach_tiles.max(1),
                gather_wait_ticks: ticks(raw.player.gather_wait_seconds),
                craft_wait_ticks: ticks(raw.player.craft_wait_seconds),
            },
            soul_min_xp: chosen.soul_min_xp.max(1),
            resources,
            skills,
            trees,
            recipes,
            production,
            sites,
            gathering,
            logistics,
            construction,
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
            !def.chosen_only
                && level >= def.level_required
                && def.inputs.iter().all(|&(res, n)| have(res) >= n)
        })
    }

    /// Like `best_recipe`, for a recipe making `output`.
    pub fn best_recipe_for(
        &self,
        station: BuildingKindId,
        level: u32,
        output: ResourceId,
        have: impl Fn(ResourceId) -> u32,
    ) -> Option<RecipeId> {
        self.station_recipes(station).into_iter().rev().find(|&r| {
            let def = self.recipe(r);
            def.output.0 == output
                && level >= def.level_required
                && def.inputs.iter().all(|&(res, n)| have(res) >= n)
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
