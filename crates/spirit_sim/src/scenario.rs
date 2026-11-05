//! Starting scenarios (`data/scenarios.toml`). Normal gameplay and the
//! development demonstration share all rules and differ only in setup.

use crate::defs::{GameDefs, ResourceId, TreeKindId};
use crate::entities::{Actor, Building, BuildingState, Player, Tree, TreeState};
use crate::events::{EventKind, EventLog, Notices};
use crate::grid::{BUILTIN_MAP, Grid, MapLayout, TilePos};
use crate::inventory::Storage;
use crate::journey::Journey;
use crate::metrics::SiteStats;
use crate::planner::PlannerState;
use crate::production::ProductionOrder;
use crate::training::TrainingSupport;
use crate::world::{Ledger, Objective, Settlement, World};
use serde::Deserialize;
use std::collections::BTreeMap;

pub const BUILTIN_SCENARIOS: &str = include_str!("../../../data/scenarios.toml");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScenarioKind {
    NewGame,
    Demo,
    /// Train to the Oak unlock, sacrifice it into a spirit, then retrain.
    Regrind,
    /// One spirit at the Starting Grove: extraction stays below regrowth.
    GroveSustainable,
    /// Four spirits at the Starting Grove: extraction outpaces regrowth.
    GroveOversubscribed,
    /// Train Woodworking by hand, then sacrifice it into a spirit.
    WorkshopManual,
    /// Spirits maintain a plank target with no player help.
    WorkshopAutomated,
    /// The chain supplies planks to a construction project.
    WorkshopConstruction,
    /// Train toward the top tiers, then sacrifice them into spirits.
    AdvancedSpecialization,
    /// More workers than a workshop has places.
    ProductionBottleneck,
    /// The same line with a second workshop on order.
    ExpandedProduction,
    /// Retrain with spirits and infrastructure behind you.
    SupportedRetraining,
    /// A few minutes of continuous gathering, with Oak already open.
    SkillingSession,
    /// Every recipe unlocked and materials for each, to compare them.
    WorkshopRecipes,
    /// Main Storage almost full, to see what the game says about it.
    FullStorage,
    /// Hardwood travels home to be made into beams.
    CentralProduction,
    /// The same spirits make beams beside the trees and send them home.
    RemoteProduction,
    /// Nothing built at Old Growth yet: the player sets the outpost up.
    OutpostEstablishment,
    /// The crossing is built, with beams still made at home.
    ExpandedCentral,
    /// The crossing is built, with beams made at the outpost.
    ExpandedRemote,
    /// An outpost that can make what the crossing needs, but hasn't yet.
    ForestAccess,
    /// Stone to cut and a causeway to spend it on.
    QuarryStart,
    /// A builder shut in by racks, as older saves can have.
    WalledIn,
    /// A skilled player with planks to spare and things to make from them.
    PanelWorkshop,
    /// Main Storage clogged with planks and the roads still to be laid.
    PlankSurplus,
    /// A small plan to set going and leave alone.
    ProjectQueue,
    /// One hauler shared between home and the outpost.
    SharedHauler,
}

impl ScenarioKind {
    pub const ALL: [ScenarioKind; 27] = [
        ScenarioKind::NewGame,
        ScenarioKind::Demo,
        ScenarioKind::Regrind,
        ScenarioKind::GroveSustainable,
        ScenarioKind::GroveOversubscribed,
        ScenarioKind::WorkshopManual,
        ScenarioKind::WorkshopAutomated,
        ScenarioKind::WorkshopConstruction,
        ScenarioKind::AdvancedSpecialization,
        ScenarioKind::ProductionBottleneck,
        ScenarioKind::ExpandedProduction,
        ScenarioKind::SupportedRetraining,
        ScenarioKind::SkillingSession,
        ScenarioKind::WorkshopRecipes,
        ScenarioKind::FullStorage,
        ScenarioKind::CentralProduction,
        ScenarioKind::RemoteProduction,
        ScenarioKind::OutpostEstablishment,
        ScenarioKind::ExpandedCentral,
        ScenarioKind::ExpandedRemote,
        ScenarioKind::ForestAccess,
        ScenarioKind::QuarryStart,
        ScenarioKind::WalledIn,
        ScenarioKind::PanelWorkshop,
        ScenarioKind::PlankSurplus,
        ScenarioKind::ProjectQueue,
        ScenarioKind::SharedHauler,
    ];

    pub fn key(self) -> &'static str {
        match self {
            ScenarioKind::NewGame => "new_game",
            ScenarioKind::Demo => "demo",
            ScenarioKind::Regrind => "regrind",
            ScenarioKind::GroveSustainable => "grove_sustainable",
            ScenarioKind::GroveOversubscribed => "grove_oversubscribed",
            ScenarioKind::WorkshopManual => "workshop_manual",
            ScenarioKind::WorkshopAutomated => "workshop_automated",
            ScenarioKind::WorkshopConstruction => "workshop_construction",
            ScenarioKind::AdvancedSpecialization => "advanced_specialization",
            ScenarioKind::ProductionBottleneck => "production_bottleneck",
            ScenarioKind::ExpandedProduction => "expanded_production",
            ScenarioKind::SupportedRetraining => "supported_retraining",
            ScenarioKind::SkillingSession => "skilling_session",
            ScenarioKind::WorkshopRecipes => "workshop_recipes",
            ScenarioKind::FullStorage => "full_storage",
            ScenarioKind::CentralProduction => "central_production",
            ScenarioKind::RemoteProduction => "remote_production",
            ScenarioKind::OutpostEstablishment => "outpost_establishment",
            ScenarioKind::ExpandedCentral => "expanded_central",
            ScenarioKind::ExpandedRemote => "expanded_remote",
            ScenarioKind::ForestAccess => "forest_access",
            ScenarioKind::QuarryStart => "quarry_start",
            ScenarioKind::WalledIn => "walled_in",
            ScenarioKind::PanelWorkshop => "panel_workshop",
            ScenarioKind::PlankSurplus => "plank_surplus",
            ScenarioKind::ProjectQueue => "project_queue",
            ScenarioKind::SharedHauler => "shared_hauler",
        }
    }

    pub fn from_key(key: &str) -> Option<ScenarioKind> {
        Self::ALL.into_iter().find(|k| k.key() == key)
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct SpiritSeed {
    pub role: String,
    /// Fixed XP, or use `level` for the XP of a level in the current profile.
    #[serde(default)]
    pub xp: u64,
    #[serde(default)]
    pub level: Option<u32>,
    pub spawn: String,
    /// Starts with the instruction to gather the top tier first.
    #[serde(default)]
    pub prefer_advanced: bool,
    /// Starts with the instruction to gather only at this site.
    #[serde(default)]
    pub site: Option<String>,
    /// Starts with the instruction to work only at the bench on this tile.
    #[serde(default)]
    pub works_at: Option<[i32; 2]>,
}

/// A building that's already standing when the scenario starts.
#[derive(Clone, Debug, Deserialize)]
pub struct PrebuiltSeed {
    pub building: String,
    pub at: [i32; 2],
    /// Stock to leave here, as resource and amount.
    #[serde(default)]
    pub keep: BTreeMap<String, u32>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ScenarioDef {
    pub title: String,
    pub description: String,
    pub objective_target: u32,
    #[serde(default)]
    pub player_xp: BTreeMap<String, u64>,
    #[serde(default)]
    pub main_storage: BTreeMap<String, u32>,
    #[serde(default)]
    pub spirits: Vec<SpiritSeed>,
    /// Sites where spirits may not gather (reserved for the player).
    #[serde(default)]
    pub closed_sites: Vec<String>,
    /// Stock of the production output to maintain.
    #[serde(default)]
    pub plank_target: u32,
    /// Raw materials production must leave in Main Storage.
    #[serde(default)]
    pub log_reserve: u32,
    /// Buildings already ordered at start.
    #[serde(default)]
    pub requested_buildings: Vec<String>,
    /// Stock of each further product to maintain.
    #[serde(default)]
    pub extra_targets: BTreeMap<String, u32>,
    /// Gathering moves on to the next tree by itself. Real games start with
    /// this on. The older test scenarios keep one click per tree.
    #[serde(default)]
    pub auto_gather: bool,
    /// Buildings already standing.
    #[serde(default)]
    pub prebuilt: Vec<PrebuiltSeed>,
}

/// A full-grown tree of `kind`.
pub(crate) fn new_tree(defs: &GameDefs, kind: TreeKindId, pos: TilePos, id: usize) -> Tree {
    let def = defs.tree(kind);
    let site = defs.site_at(pos.x, pos.y);
    let pct = site.map_or(100, |s| defs.site(s).regrow_pct) as u64;
    Tree {
        id,
        kind,
        pos,
        state: TreeState::Grown { logs: def.logs },
        site,
        regrow_ticks: (def.regrow_ticks * pct / 100).max(1),
        claimed_by: None,
    }
}

/// Orders for further products: none wanted until the player sets a target.
pub(crate) fn extra_orders(outputs: &[ResourceId]) -> Vec<ProductionOrder> {
    outputs
        .iter()
        .map(|&output| ProductionOrder {
            output,
            target: 0,
            input_reserve: 0,
            enabled: true,
            priority: 0,
        })
        .collect()
}

pub fn load_scenarios(text: &str) -> Result<BTreeMap<String, ScenarioDef>, String> {
    toml::from_str(text).map_err(|e| format!("scenarios.toml: {e}"))
}

impl World {
    /// Builds a world from the built-in definitions, map and scenario.
    pub fn from_scenario(kind: ScenarioKind) -> World {
        Self::scenario_with(kind, GameDefs::builtin()).expect("built-in scenario must be valid")
    }

    /// A real playthrough: the new game under a progression profile, with
    /// guidance through the first progression cycle.
    pub fn new_journey(profile: &str) -> Result<World, String> {
        let defs = GameDefs::builtin_profile(profile)?;
        let mut world = Self::scenario_with(ScenarioKind::NewGame, defs)?;
        world.journey = Some(Journey::default());
        world.update_journey();
        Ok(world)
    }

    fn scenario_with(kind: ScenarioKind, defs: GameDefs) -> Result<World, String> {
        let scenarios = load_scenarios(BUILTIN_SCENARIOS)?;
        let scenario = scenarios
            .get(kind.key())
            .ok_or_else(|| format!("scenario {:?} missing", kind.key()))?;
        let map = MapLayout::parse(BUILTIN_MAP)?;
        World::build(defs, &map, scenario)
    }

    pub fn build(defs: GameDefs, map: &MapLayout, scenario: &ScenarioDef) -> Result<World, String> {
        let mut grid = Grid::new(map);
        let mut trees = Vec::new();
        let mut buildings = Vec::new();
        for (&c, positions) in &map.markers {
            for &pos in positions {
                if let Some(kind) = defs.tree_kind_for_char(c) {
                    trees.push(new_tree(&defs, kind, pos, 0));
                    grid.set_occupied(pos, true);
                } else if let Some(kind) = defs.building_kind_for_char(c) {
                    let def = defs.building(kind);
                    buildings.push(Building {
                        id: 0,
                        kind,
                        pos,
                        name: def.name.clone(),
                        state: BuildingState::Complete,
                        storage: Storage::new(def.storage_capacity()),
                        crafted: 0,
                        keep: BTreeMap::new(),
                        extra_slots: 0,
                    });
                    grid.set_occupied(pos, true);
                }
            }
        }
        // Deterministic ids in reading order.
        trees.sort_by_key(|t| (t.pos.y, t.pos.x));
        for (i, t) in trees.iter_mut().enumerate() {
            t.id = i;
        }
        buildings.sort_by_key(|b| (b.pos.y, b.pos.x));
        for (i, b) in buildings.iter_mut().enumerate() {
            b.id = i;
        }
        let main_kind = defs.main_building_kind();
        let main_storage = buildings
            .iter()
            .position(|b| b.kind == main_kind)
            .ok_or("map has no main storage marker")?;

        let player_pos = map.marker('P').ok_or("map has no player start 'P'")?;
        let player = Player::new(Actor::new(
            player_pos,
            defs.player.move_ticks,
            defs.player.carry_capacity,
        ));

        let objective = Objective {
            resource: defs.objective.resource,
            target: scenario.objective_target,
        };
        let event_cap = defs.event_log_capacity;
        let mut site_open = vec![true; defs.sites.len()];
        for key in &scenario.closed_sites {
            let id = defs
                .site_id(key)
                .ok_or_else(|| format!("unknown site {key:?}"))?;
            site_open[id.index()] = false;
        }
        let site_stats = vec![SiteStats::default(); defs.sites.len()];
        let order = ProductionOrder {
            output: defs.production.output,
            target: scenario.plank_target,
            input_reserve: scenario.log_reserve,
            enabled: true,
            priority: 0,
        };
        let defs_orders = defs.production.more_outputs.clone();
        let mut world = World {
            defs,
            tick: 0,
            grid,
            trees,
            buildings,
            spirits: Vec::new(),
            player,
            projects: Vec::new(),
            settlement: Settlement {
                objective,
                allow_autonomous_construction: true,
                planner: PlannerState::default(),
                site_open,
                order,
                training: TrainingSupport::default(),
                extra_orders: extra_orders(&defs_orders),
                carry_bonus: 0,
                bench_bonus: 0,
                queue: Vec::new(),
                next_queue_id: 0,
            },
            site_stats,
            output_stats: SiteStats::default(),
            events: EventLog::new(event_cap),
            ledger: Ledger::default(),
            main_storage,
            notices: Notices::default(),
            journey: None,
            product_stats: BTreeMap::new(),
        };

        world.player.auto_gather = scenario.auto_gather;
        for (skill, &xp) in &scenario.player_xp {
            let id = world
                .defs
                .skill_id(skill)
                .ok_or_else(|| format!("unknown skill {skill:?}"))?;
            world.player.add_xp(id, xp, &world.defs.xp_table);
        }
        for (res, &n) in &scenario.main_storage {
            let id: ResourceId = world
                .defs
                .resource_id(res)
                .ok_or_else(|| format!("unknown resource {res:?}"))?;
            let stored = world.buildings[main_storage].storage.deposit(id, n);
            *world.ledger.seeded.entry(id).or_default() += stored as u64;
        }
        for seed in &scenario.prebuilt {
            let kind = world
                .defs
                .building_kind_id(&seed.building)
                .ok_or_else(|| format!("unknown building {:?}", seed.building))?;
            let def = world.defs.building(kind);
            let pos = TilePos::new(seed.at[0], seed.at[1]);
            let mut keep = BTreeMap::new();
            for (res, &n) in &seed.keep {
                let id = world
                    .defs
                    .resource_id(res)
                    .ok_or_else(|| format!("unknown resource {res:?}"))?;
                keep.insert(id, n);
            }
            let building = Building {
                id: world.buildings.len(),
                kind,
                pos,
                name: def.name.clone(),
                state: BuildingState::Complete,
                storage: Storage::new(def.storage_capacity()),
                crafted: 0,
                keep,
                extra_slots: 0,
            };
            let opens = def.opens.clone();
            let paves = def.paves.clone();
            let road_pct = world.defs.construction.road_pct;
            world.buildings.push(building);
            world.grid.set_occupied(pos, paves.is_empty());
            for [x, y] in opens {
                world.grid.open_crossing(TilePos::new(x, y));
            }
            for [x, y] in paves {
                world.grid.pave(TilePos::new(x, y), road_pct);
            }
        }
        for seed in &scenario.spirits {
            let role = world
                .defs
                .role_id(&seed.role)
                .ok_or_else(|| format!("unknown role {:?}", seed.role))?;
            let c = seed.spawn.chars().next().ok_or("empty spawn marker")?;
            let pos = map
                .marker(c)
                .ok_or_else(|| format!("map has no spawn marker {c:?}"))?;
            let xp = seed
                .level
                .map_or(seed.xp, |l| world.defs.xp_table.xp_for_level(l));
            let id = world.add_spirit(role, xp, pos);
            world.spirits[id].policy.prefer_advanced = seed.prefer_advanced;
            if let Some(key) = &seed.site {
                let site = world
                    .defs
                    .site_id(key)
                    .ok_or_else(|| format!("unknown site {key:?}"))?;
                world.spirits[id].policy.site = Some(site);
            }
            if let Some([x, y]) = seed.works_at {
                let bench = world.building_at(TilePos::new(x, y)).map(|b| b.id);
                world.spirits[id].policy.station =
                    Some(bench.ok_or_else(|| format!("no building at ({x}, {y})"))?);
            }
        }

        for (key, &target) in &scenario.extra_targets {
            let id = world
                .defs
                .resource_id(key)
                .ok_or_else(|| format!("unknown resource {key:?}"))?;
            world
                .order_for_mut(id)
                .ok_or_else(|| format!("no order for {key:?}"))?
                .target = target;
        }

        let o = &world.settlement.objective;
        let msg = format!(
            "{}: settlement objective is to maintain {} {} in Main Storage.",
            scenario.title,
            o.target,
            world.defs.resource(o.resource).name
        );
        world.events.push(0, EventKind::Planner, msg);
        if !scenario.description.is_empty() {
            world
                .events
                .push(0, EventKind::Player, scenario.description.clone());
        }
        for key in &scenario.requested_buildings {
            let kind = world
                .defs
                .building_kind_id(key)
                .ok_or_else(|| format!("unknown building {key:?}"))?;
            world.request_building(kind)?;
        }
        Ok(world)
    }
}
