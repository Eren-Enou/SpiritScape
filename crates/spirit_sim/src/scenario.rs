//! Starting scenarios (`data/scenarios.toml`). Normal gameplay and the
//! development demonstration share all rules and differ only in setup.

use crate::defs::{GameDefs, ResourceId};
use crate::entities::{Actor, Building, BuildingState, Player, Tree, TreeState};
use crate::events::{EventKind, EventLog};
use crate::grid::{Grid, MapLayout, BUILTIN_MAP};
use crate::inventory::Storage;
use crate::planner::PlannerState;
use crate::world::{Ledger, Objective, Settlement, World};
use serde::Deserialize;
use std::collections::BTreeMap;

pub const BUILTIN_SCENARIOS: &str = include_str!("../../../data/scenarios.toml");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScenarioKind {
    NewGame,
    Demo,
}

impl ScenarioKind {
    pub fn key(self) -> &'static str {
        match self {
            ScenarioKind::NewGame => "new_game",
            ScenarioKind::Demo => "demo",
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct SpiritSeed {
    pub role: String,
    pub xp: u64,
    pub spawn: String,
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
}

pub fn load_scenarios(text: &str) -> Result<BTreeMap<String, ScenarioDef>, String> {
    toml::from_str(text).map_err(|e| format!("scenarios.toml: {e}"))
}

impl World {
    /// Builds a world from the built-in definitions, map and scenario.
    pub fn from_scenario(kind: ScenarioKind) -> World {
        let defs = GameDefs::builtin();
        let scenarios = load_scenarios(BUILTIN_SCENARIOS).expect("built-in scenarios must be valid");
        let scenario = scenarios.get(kind.key()).expect("scenario missing from scenarios.toml");
        let map = MapLayout::parse(BUILTIN_MAP).expect("built-in map must be valid");
        World::build(defs, &map, scenario).expect("built-in scenario must be valid")
    }

    pub fn build(defs: GameDefs, map: &MapLayout, scenario: &ScenarioDef) -> Result<World, String> {
        let mut grid = Grid::new(map);
        let mut trees = Vec::new();
        let mut buildings = Vec::new();
        for (&c, positions) in &map.markers {
            for &pos in positions {
                if let Some(kind) = defs.tree_kind_for_char(c) {
                    let logs = defs.tree(kind).logs;
                    trees.push(Tree { id: 0, kind, pos, state: TreeState::Grown { logs }, claimed_by: None });
                    grid.set_occupied(pos, true);
                } else if let Some(kind) = defs.building_kind_for_char(c) {
                    let def = defs.building(kind);
                    buildings.push(Building {
                        id: 0,
                        kind,
                        pos,
                        name: def.name.clone(),
                        state: BuildingState::Complete,
                        storage: Storage::new(def.capacity),
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
        let player = Player::new(Actor::new(player_pos, defs.player.move_ticks, defs.player.carry_capacity));

        let objective = Objective { resource: defs.objective.resource, target: scenario.objective_target };
        let event_cap = defs.event_log_capacity;
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
            },
            events: EventLog::new(event_cap),
            ledger: Ledger::default(),
            main_storage,
        };

        for (skill, &xp) in &scenario.player_xp {
            let id = world.defs.skill_id(skill).ok_or_else(|| format!("unknown skill {skill:?}"))?;
            world.player.add_xp(id, xp);
        }
        for (res, &n) in &scenario.main_storage {
            let id: ResourceId = world.defs.resource_id(res).ok_or_else(|| format!("unknown resource {res:?}"))?;
            let stored = world.buildings[main_storage].storage.deposit(id, n);
            *world.ledger.seeded.entry(id).or_default() += stored as u64;
        }
        for seed in &scenario.spirits {
            let role = world.defs.role_id(&seed.role).ok_or_else(|| format!("unknown role {:?}", seed.role))?;
            let c = seed.spawn.chars().next().ok_or("empty spawn marker")?;
            let pos = map.marker(c).ok_or_else(|| format!("map has no spawn marker {c:?}"))?;
            world.add_spirit(role, seed.xp, pos);
        }

        let o = &world.settlement.objective;
        let msg = format!(
            "{}: settlement objective is to maintain {} {} in Main Storage.",
            scenario.title,
            o.target,
            world.defs.resource(o.resource).name
        );
        world.events.push(0, EventKind::Planner, msg);
        Ok(world)
    }
}
