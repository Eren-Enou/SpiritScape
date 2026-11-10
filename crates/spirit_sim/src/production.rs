//! The settlement's production orders: what it wants made, what that demands
//! upstream, and why output is or isn't flowing.
//!
//! Demand runs backwards one step at a time and is always bounded: a shortfall
//! in a finished good becomes a request for inputs at a station (for haulers)
//! and a capped addition to the stock goal of each input (for gatherers).
//! Recipes can't form a loop (`GameDefs` refuses that when it loads), so there
//! is nothing to resolve recursively. Nothing here creates or moves resources;
//! workers do that through tasks.

use crate::defs::{JobKind, RecipeId, ResourceId};
use crate::entities::{Building, BuildingId, Spirit, SpiritId};
use crate::metrics::{TimeUse, WaitKind};
use crate::session::SessionKind;
use crate::tasks::Task;
use crate::world::World;

/// A standing instruction: keep `target` units of `output` in Main Storage
/// without taking raw materials below `input_reserve`.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProductionOrder {
    pub output: ResourceId,
    pub target: u32,
    pub input_reserve: u32,
    pub enabled: bool,
    /// Higher goes first when workers, haulers or benches are short.
    #[serde(default)]
    pub priority: u32,
}

/// What the station is doing right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkshopStatus {
    NoStation,
    Disabled,
    NoWorker,
    TargetMet,
    Working,
    WaitingForInputs,
    OutputFull,
}

impl WorkshopStatus {
    pub fn label(self) -> &'static str {
        match self {
            WorkshopStatus::NoStation => "No workshop",
            WorkshopStatus::Disabled => "Production disabled",
            WorkshopStatus::NoWorker => "Workshop unstaffed",
            WorkshopStatus::TargetMet => "Workshop idle: target met",
            WorkshopStatus::Working => "Workshop operating",
            WorkshopStatus::WaitingForInputs => "Workshop waiting: insufficient logs",
            WorkshopStatus::OutputFull => "Workshop stopped: output full",
        }
    }
}

/// What limits the production chain right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProductionBottleneck {
    None,
    Disabled,
    NoWorker,
    LogSupply,
    TreeRecovery,
    InputReserve,
    Hauling,
    Storage,
    ProcessingSpeed,
    WorkshopCapacity,
}

impl ProductionBottleneck {
    pub fn label(self) -> &'static str {
        match self {
            ProductionBottleneck::None => "None (target met)",
            ProductionBottleneck::Disabled => "Production disabled",
            ProductionBottleneck::NoWorker => "No Woodworking Spirit",
            ProductionBottleneck::LogSupply => "Log supply",
            ProductionBottleneck::TreeRecovery => "Tree recovery",
            ProductionBottleneck::InputReserve => "Log reserve",
            ProductionBottleneck::Hauling => "Hauling capacity",
            ProductionBottleneck::Storage => "Storage capacity",
            ProductionBottleneck::ProcessingSpeed => "Processing speed",
            ProductionBottleneck::WorkshopCapacity => "Workshop capacity",
        }
    }
}

#[derive(Clone, Debug)]
pub struct ProductionReport {
    /// Raw material and finished stock in Main Storage.
    pub input_in_main: u32,
    pub output_in_main: u32,
    pub input_at_station: u32,
    pub output_at_station: u32,
    /// Stock the settlement is aiming for, including what projects still need.
    pub goal: u32,
    pub status: WorkshopStatus,
    /// Finished units per minute over the measurement window.
    pub output_per_min: f32,
    pub workers: u32,
    pub gathering: TimeUse,
    pub hauling: TimeUse,
    pub processing: TimeUse,
    pub bottleneck: ProductionBottleneck,
    /// A readable sentence on why, and what would help.
    pub summary: String,
}

/// How one production order stands.
#[derive(Clone, Debug)]
pub struct OrderReport {
    pub output: ResourceId,
    pub name: String,
    pub in_main: u32,
    pub target: u32,
    pub goal: u32,
    pub enabled: bool,
    pub priority: u32,
    pub per_min: f32,
    /// A readable sentence on what's happening.
    pub status: String,
    /// Nothing is being made although more is wanted.
    pub stalled: bool,
}

/// How one workstation stands.
#[derive(Clone, Debug)]
pub struct StationReport {
    pub id: BuildingId,
    pub name: String,
    pub occupied: u32,
    pub slots: u32,
    /// Everything made here since it was built.
    pub crafted: u32,
    /// Spirit names working or walking here.
    pub workers: Vec<String>,
}

impl World {
    /// Every order, most urgent first.
    pub fn orders(&self) -> Vec<&ProductionOrder> {
        let mut list: Vec<&ProductionOrder> = std::iter::once(&self.settlement.order)
            .chain(&self.settlement.extra_orders)
            .collect();
        list.sort_by_key(|o| std::cmp::Reverse(o.priority));
        list
    }

    pub fn order_for(&self, output: ResourceId) -> Option<&ProductionOrder> {
        self.orders().into_iter().find(|o| o.output == output)
    }

    pub(crate) fn order_for_mut(&mut self, output: ResourceId) -> Option<&mut ProductionOrder> {
        let s = &mut self.settlement;
        std::iter::once(&mut s.order)
            .chain(&mut s.extra_orders)
            .find(|o| o.output == output)
    }

    /// Finished buildings that can make `output`.
    pub fn stations_for(&self, output: ResourceId) -> Vec<&Building> {
        self.buildings
            .iter()
            .filter(|b| {
                b.is_complete()
                    && self.defs.building(b.kind).is_station
                    && self
                        .defs
                        .station_recipes(b.kind)
                        .iter()
                        .any(|r| self.defs.recipe(*r).output.0 == output)
            })
            .collect()
    }

    /// Every finished workstation.
    pub fn stations(&self) -> impl Iterator<Item = &Building> {
        self.buildings
            .iter()
            .filter(|b| b.is_complete() && self.defs.building(b.kind).is_station)
    }

    /// The first building producing the main order's output.
    pub fn station(&self) -> Option<&Building> {
        self.stations_for(self.settlement.order.output)
            .into_iter()
            .next()
    }

    /// Spirits that work at stations.
    pub fn crafters(&self) -> impl Iterator<Item = &Spirit> {
        self.spirits
            .iter()
            .filter(|s| self.defs.role(s.role).has_job(JobKind::Craft))
    }

    /// Crafters available to production orders.
    fn staff(&self) -> impl Iterator<Item = &Spirit> {
        self.crafters()
            .filter(|s| s.assigned && s.policy.focus.allows(JobKind::Craft))
    }

    /// Tiles between two buildings by the shortest route.
    pub(crate) fn walk_between(&self, a: BuildingId, b: BuildingId) -> Option<u32> {
        let field = self.grid.approach_field(self.buildings[a].pos);
        self.grid.approach_distance(&field, self.buildings[b].pos)
    }

    /// Whether a carrier's instructions let it serve a building.
    pub(crate) fn serves(&self, s: &Spirit, b: &Building) -> bool {
        s.policy.site.is_none_or(|site| self.in_site(b, site))
    }

    /// What a storage can give to a station. Main Storage keeps its reserve;
    /// an outpost store gives what it has, since local use is what it's for.
    fn source_stock(&self, b: &Building, r: ResourceId) -> u32 {
        if b.id == self.main_storage {
            self.input_spare(r)
        } else {
            b.storage.available(r)
        }
    }

    /// Everything the settlement could give to stations, wherever it is.
    pub(crate) fn any_spare(&self, r: ResourceId) -> u32 {
        self.input_spare(r) + self.outposts().map(|b| b.storage.available(r)).sum::<u32>()
    }

    /// The storage to fetch `r` from for a station: the nearest that can fill
    /// the request, or else the nearest with any. Returns where, how much it
    /// can give and how far it is.
    pub(crate) fn supply_source(
        &self,
        station: BuildingId,
        r: ResourceId,
        want: u32,
    ) -> Option<(BuildingId, u32, u32)> {
        let main = self.main();
        let stocked: Vec<&Building> = std::iter::once(main)
            .chain(self.outposts())
            .filter(|b| self.source_stock(b, r) > 0)
            .collect();
        let field = self.grid.approach_field(self.buildings[station].pos);
        stocked
            .into_iter()
            .filter_map(|b| {
                let walk = self.grid.approach_distance(&field, b.pos)?;
                let stock = self.source_stock(b, r);
                Some(((stock < want, walk, b.id), stock))
            })
            .min()
            .map(|((_, walk, id), stock)| (id, stock, walk))
    }

    /// A long trip is worth making for this much, given what's wanted.
    pub(crate) fn long_trip_load(&self, s: &Spirit, wanted: u32) -> u32 {
        let pct = self.defs.logistics.long_trip_load_pct;
        (s.actor.inventory.capacity * pct / 100).min(wanted).max(1)
    }

    /// Stock a store holds back: what it's told to keep, or a bench's worth
    /// while a staffed bench at the same site uses this material.
    fn held_at(&self, b: &Building, r: ResourceId) -> u32 {
        let told = b.keep.get(&r).copied().unwrap_or(0);
        let Some(site) = self.defs.site_at(b.pos.x, b.pos.y) else {
            return told;
        };
        let local = self
            .orders()
            .iter()
            .filter(|o| o.enabled && self.uses_input(o.output, r))
            .filter(|o| !self.only_when_chosen(o.output) || self.deficit_of(o.output) > 0)
            .flat_map(|o| self.staffed_stations(o.output))
            .filter(|station| self.in_site(station, site))
            .map(|station| self.defs.building(station.kind).input_buffer)
            .max()
            .unwrap_or(0);
        told.max(local)
    }

    /// What a store can send home: the most of any one material above what it
    /// holds back.
    pub(crate) fn surplus_at(&self, b: &Building) -> Option<(ResourceId, u32)> {
        b.storage
            .contents()
            .keys()
            .map(|&r| {
                let spare = b.storage.available(r).saturating_sub(self.held_at(b, r));
                (r, spare)
            })
            .filter(|(_, n)| *n > 0)
            .max_by_key(|(r, n)| (*n, std::cmp::Reverse(*r)))
    }

    /// Everything a store could send home.
    pub(crate) fn total_surplus(&self, b: &Building) -> u32 {
        b.storage
            .contents()
            .keys()
            .map(|&r| b.storage.available(r).saturating_sub(self.held_at(b, r)))
            .sum()
    }

    /// Stock all stores have been told to keep.
    fn local_keep(&self, r: ResourceId) -> u32 {
        self.outposts()
            .map(|b| b.keep.get(&r).copied().unwrap_or(0))
            .sum()
    }

    /// Whether a spirit's instructions let it work at a station.
    pub(crate) fn may_work_at(&self, s: &Spirit, station: BuildingId) -> bool {
        s.policy.station.is_none_or(|b| b == station)
    }

    /// The best recipe for `output` any available worker knows, used to
    /// estimate demand. None while nobody could make it.
    pub fn recipe_for(&self, output: ResourceId) -> Option<RecipeId> {
        self.stations_for(output)
            .iter()
            .filter_map(|b| {
                let level = self
                    .staff()
                    .filter(|s| self.may_work_at(s, b.id) && self.can_make(s, output))
                    .map(|s| s.level())
                    .max()?;
                self.defs
                    .best_recipe_for(b.kind, level, output, |_| u32::MAX)
            })
            .max_by_key(|r| self.defs.recipe(*r).level_required)
    }

    /// The recipe the main order would use.
    pub fn production_recipe(&self) -> Option<RecipeId> {
        self.recipe_for(self.settlement.order.output)
    }

    /// What the plan still has to claim: projects not yet reserved, and
    /// everything queued. Reserved stock is already out of `available`, so
    /// nothing is counted twice. The queue's limit keeps this bounded.
    pub(crate) fn project_demand(&self, r: ResourceId) -> u32 {
        let cost_of = |k: crate::defs::BuildingKindId| -> u32 {
            let cost = self.defs.building(k).cost.iter();
            cost.filter(|(res, _)| *res == r).map(|(_, n)| *n).sum()
        };
        let waiting: u32 = self
            .projects
            .iter()
            .filter(|p| p.is_active() && !p.reserved)
            .map(|p| {
                let need = cost_of(p.kind);
                need.saturating_sub(p.claimed_of(r, need))
            })
            .sum();
        let queued: u32 = self.settlement.queue.iter().map(|q| cost_of(q.kind)).sum();
        waiting + queued
    }

    /// Room in Main Storage for `r`. Finished goods nobody wants leave some
    /// free for raw materials, so surplus can't stop the settlement getting
    /// what it needs to build with.
    pub(crate) fn main_room_for(&self, r: ResourceId) -> u32 {
        let free = self.main().storage.free();
        let finished = self.defs.recipes.iter().any(|d| d.output.0 == r);
        let wanted = self.main().storage.available(r) < self.goal_of(r);
        if finished && !wanted {
            free.saturating_sub(self.defs.construction.raw_room)
        } else {
            free
        }
    }

    /// Room for what the player brings. While a project is waiting on
    /// materials, their spare logs can't take the room it needs. Otherwise
    /// it's all theirs.
    pub(crate) fn player_room_for(&self, r: ResourceId) -> u32 {
        let waiting = self.active_project().is_some_and(|p| !p.reserved);
        if waiting {
            self.haul_room_for(r)
        } else {
            self.main_room_for(r)
        }
    }

    /// Room for hauling a store's stock home. Raw materials nothing needs yet
    /// can wait where they are, rather than take the last of the room.
    pub(crate) fn haul_room_for(&self, r: ResourceId) -> u32 {
        let main = &self.main().storage;
        let surplus = main.available(r) >= self.gather_goal_of(r);
        if surplus && self.order_for(r).is_none() {
            main.free().saturating_sub(self.defs.construction.raw_room)
        } else {
            self.main_room_for(r)
        }
    }

    /// Workers that fit at a bench, counting annexes.
    pub fn slots_of(&self, b: &Building) -> u32 {
        self.defs.building(b.kind).slots + b.extra_slots
    }

    /// The stock an order aims for: its target plus unreserved project needs.
    pub fn goal_of(&self, output: ResourceId) -> u32 {
        self.order_for(output).map_or(0, |o| o.target)
            + self.project_demand(output)
            + self.demand_from_orders(output)
    }

    /// What other orders need of `output` as a material, capped like any other
    /// upstream demand. Recipes can't form a loop, so this always ends.
    fn demand_from_orders(&self, output: ResourceId) -> u32 {
        self.orders()
            .iter()
            .filter(|o| o.output != output && self.uses_input(o.output, output))
            .filter_map(|o| {
                let def = self.defs.recipe(self.recipe_for(o.output)?);
                let per_batch = def.inputs.iter().find(|i| i.0 == output)?.1;
                Some(self.deficit_of(o.output).div_ceil(def.output.1) * per_batch)
            })
            .sum::<u32>()
            .min(self.defs.production.upstream_cap)
    }

    pub fn production_goal(&self) -> u32 {
        self.goal_of(self.settlement.order.output)
    }

    /// Units an order still wants made. Zero while nobody could make them.
    pub fn deficit_of(&self, output: ResourceId) -> u32 {
        let enabled = self.order_for(output).is_some_and(|o| o.enabled);
        if !enabled || self.recipe_for(output).is_none() {
            return 0;
        }
        self.goal_of(output)
            .saturating_sub(self.projected_stock(output))
    }

    pub fn production_deficit(&self) -> u32 {
        self.deficit_of(self.settlement.order.output)
    }

    /// Units of `r` that spirits are bringing to `building` right now.
    fn incoming(&self, building: BuildingId, r: ResourceId) -> u32 {
        self.spirits
            .iter()
            .filter(|s| {
                s.actor
                    .queue
                    .iter()
                    .any(|t| matches!(t, Task::Deposit { building: b } if *b == building))
            })
            .map(|s| {
                let queued: u32 = s
                    .actor
                    .queue
                    .iter()
                    .map(|t| match t {
                        Task::Withdraw {
                            resource, amount, ..
                        } if *resource == r => *amount,
                        _ => 0,
                    })
                    .sum();
                s.actor.inventory.get(r) + queued
            })
            .sum()
    }

    /// Stations that have someone who could work there for `output`.
    fn staffed_stations(&self, output: ResourceId) -> Vec<&Building> {
        self.stations_for(output)
            .into_iter()
            .filter(|b| {
                self.staff()
                    .any(|s| self.may_work_at(s, b.id) && self.can_make(s, output))
            })
            .collect()
    }

    /// Units of each input the shortfall of `output` would consume, and how
    /// many of them are already at or on their way to a station.
    fn input_need(&self, output: ResourceId) -> Vec<(ResourceId, u32, u32)> {
        let Some(recipe) = self.recipe_for(output) else {
            return Vec::new();
        };
        let def = self.defs.recipe(recipe);
        let batches = self.deficit_of(output).div_ceil(def.output.1);
        def.inputs
            .iter()
            .map(|&(r, n)| {
                let have = self
                    .staffed_stations(output)
                    .iter()
                    .map(|b| b.storage.available(r) + self.incoming(b.id, r))
                    .sum();
                (r, batches * n, have)
            })
            .collect()
    }

    /// Staffed benches that should get a material, nearest to it first. A bench
    /// a long trip from the material gets none while a closer one can use it,
    /// so nothing is carried across the map to be made there instead.
    fn benches_for_input(&self, output: ResourceId, r: ResourceId) -> Vec<&Building> {
        let stations = self.staffed_stations(output);
        if stations.len() < 2 {
            return stations;
        }
        let mut near: Vec<(u32, &Building)> = stations
            .into_iter()
            .map(|b| {
                let source = self.supply_source(b.id, r, 1);
                (source.map_or(u32::MAX, |s| s.2), b)
            })
            .collect();
        near.sort_by_key(|(walk, b)| (*walk, b.id));
        let closest = near[0].0;
        let long = self.defs.logistics.long_trip_tiles;
        near.into_iter()
            .filter(|(walk, _)| *walk < long || *walk == closest)
            .map(|(_, b)| b)
            .collect()
    }

    /// Every delivery stations would take now, most urgent first: where, what
    /// and how much. Bounded by each input buffer and the current shortfall.
    fn input_requests(&self) -> Vec<(BuildingId, ResourceId, u32)> {
        let mut requests = Vec::new();
        for order in self.orders() {
            let Some(recipe) = self.recipe_for(order.output) else {
                continue;
            };
            let def = self.defs.recipe(recipe);
            let batches = self.deficit_of(order.output).div_ceil(def.output.1);
            for &(r, n) in &def.inputs {
                let mut need = batches * n;
                for b in self.benches_for_input(order.output, r) {
                    let buffer = self.defs.building(b.kind).input_buffer;
                    let have = b.storage.available(r) + self.incoming(b.id, r);
                    let want = need.min(buffer).saturating_sub(have).min(b.storage.free());
                    if want > 0 {
                        requests.push((b.id, r, want));
                    }
                    need = need.saturating_sub(have.max(want));
                }
            }
        }
        requests
    }

    /// The delivery this spirit should make next: the most urgent one it
    /// serves that some storage can fill, or else the most urgent it serves.
    pub(crate) fn input_wanted(&self, sid: SpiritId) -> Option<(BuildingId, ResourceId, u32)> {
        let s = &self.spirits[sid];
        let requests: Vec<_> = self
            .input_requests()
            .into_iter()
            .filter(|(b, ..)| self.serves(s, &self.buildings[*b]))
            .collect();
        requests
            .iter()
            .find(|(_, r, _)| self.any_spare(*r) > 0)
            .or(requests.first())
            .copied()
    }

    /// Extra stock of `r` that production asks gatherers for, on top of the
    /// settlement's own goal. Capped, so a large target can't demand everything.
    pub fn upstream_demand(&self, r: ResourceId) -> u32 {
        self.orders()
            .iter()
            .flat_map(|o| self.input_need(o.output))
            .filter(|(res, ..)| *res == r)
            .map(|(_, need, have)| need.saturating_sub(have))
            .sum::<u32>()
            .min(self.defs.production.upstream_cap)
    }

    /// Whether `output` is only made on request, so holding materials for it
    /// makes no sense until someone wants some.
    fn only_when_chosen(&self, output: ResourceId) -> bool {
        let mut made = self.defs.recipes.iter().filter(|d| d.output.0 == output);
        made.all(|d| d.chosen_only)
    }

    /// Whether any recipe for `output` uses `input`.
    fn uses_input(&self, output: ResourceId, input: ResourceId) -> bool {
        self.defs
            .recipes
            .iter()
            .any(|d| d.output.0 == output && d.inputs.iter().any(|i| i.0 == input))
    }

    /// Stock of `r` the orders using it have told production to leave alone.
    pub(crate) fn reserve_of(&self, r: ResourceId) -> u32 {
        self.orders()
            .iter()
            .filter(|o| self.uses_input(o.output, r))
            .map(|o| o.input_reserve)
            .max()
            .unwrap_or(0)
    }

    /// What Main Storage can give to production without breaking the reserve
    /// or taking what a project is waiting for.
    pub(crate) fn input_spare(&self, r: ResourceId) -> u32 {
        self.main()
            .storage
            .available(r)
            .saturating_sub(self.reserve_of(r) + self.project_demand(r))
    }

    /// The stock level of `r` gatherers work toward.
    pub fn gather_goal_of(&self, r: ResourceId) -> u32 {
        let o = &self.settlement.objective;
        let base = if r == o.resource { o.target } else { 0 };
        let standing = self.settlement.standing.get(&r).copied().unwrap_or(0);
        let base = base.max(standing);
        let wanted = self.orders().iter().any(|o| {
            o.enabled && self.uses_input(o.output, r) && self.recipe_for(o.output).is_some()
        });
        let goal = if wanted {
            base.max(self.reserve_of(r)) + self.upstream_demand(r)
        } else {
            base
        };
        // Raw materials a project needs come from gatherers directly.
        let project = if self.order_for(r).is_none() {
            self.project_demand(r)
        } else {
            0
        };
        goal + project + self.local_keep(r) + self.training_gather_demand(r)
    }

    /// The log level gatherers work toward.
    pub fn gather_goal(&self) -> u32 {
        self.gather_goal_of(self.settlement.objective.resource)
    }

    /// Whether gatherers should bring in more `r`.
    pub(crate) fn gather_short(&self, r: ResourceId) -> bool {
        self.projected_stock(r) < self.gather_goal_of(r)
    }

    /// Finished goods of an order waiting at a station, unclaimed.
    fn waiting_at(&self, b: &Building) -> u32 {
        self.orders()
            .iter()
            .map(|o| b.storage.available(o.output))
            .sum()
    }

    /// Finished goods at stations that nobody has claimed yet.
    pub(crate) fn output_waiting(&self) -> u32 {
        self.stations().map(|b| self.waiting_at(b)).sum()
    }

    /// The fullest pile of finished goods this spirit serves.
    /// Stock of `r` a bench is holding as an input for another order.
    fn used_at_bench(&self, b: &Building, r: ResourceId) -> u32 {
        let wanted = self.orders().into_iter().any(|o| {
            o.output != r
                && self.deficit_of(o.output) > 0
                && self
                    .recipe_for(o.output)
                    .is_some_and(|id| self.defs.recipe(id).inputs.iter().any(|i| i.0 == r))
        });
        if wanted {
            self.defs.building(b.kind).input_buffer
        } else {
            0
        }
    }

    pub(crate) fn output_pile(&self, sid: SpiritId) -> Option<(BuildingId, ResourceId, u32)> {
        let s = &self.spirits[sid];
        let mut best: Option<(BuildingId, ResourceId, u32)> = None;
        for b in self.stations().filter(|b| self.serves(s, b)) {
            for o in self.orders() {
                // What the bench is about to use isn't there to be taken away.
                let n = b
                    .storage
                    .available(o.output)
                    .saturating_sub(self.used_at_bench(b, o.output));
                let room = self.main_room_for(o.output) > 0;
                if n > 0 && room && best.is_none_or(|x| n > x.2) {
                    best = Some((b.id, o.output, n));
                }
            }
        }
        best
    }

    /// Someone is working at this station right now.
    fn being_worked(&self, id: BuildingId) -> bool {
        let at_bench =
            |q: Option<&Task>| matches!(q, Some(Task::Craft { building, .. }) if *building == id);
        self.crafters().any(|s| at_bench(s.actor.queue.front()))
            || at_bench(self.player.actor.queue.front())
    }

    /// Whether finished goods are worth a trip: a load, or anything at all
    /// once nobody is adding to them. A long trip wants a bigger load.
    pub(crate) fn output_ready(&self, sid: SpiritId, load: u32) -> bool {
        let s = &self.spirits[sid];
        let main = self.main_storage;
        let long = self.defs.logistics.long_trip_tiles;
        self.stations().filter(|b| self.serves(s, b)).any(|b| {
            let waiting = self.waiting_at(b);
            if waiting == 0 {
                return false;
            }
            if !self.being_worked(b.id) {
                return true;
            }
            let far = self.walk_between(b.id, main).is_some_and(|d| d >= long);
            let need = if far {
                load.max(self.long_trip_load(s, u32::MAX))
            } else {
                load
            };
            // A shelf that can't take another batch is as full as it gets, so
            // a big enough hauler must not wait for more than it can ever hold.
            waiting >= need.min(self.shelf_can_reach(b)).max(1)
        })
    }

    /// The most finished goods a bench can pile up before its worker stops.
    fn shelf_can_reach(&self, b: &Building) -> u32 {
        let def = self.defs.building(b.kind);
        let shelf = def.capacity.saturating_sub(def.input_buffer);
        let batch = self
            .defs
            .station_recipes(b.kind)
            .iter()
            .map(|r| self.defs.recipe(*r).output.1)
            .max()
            .unwrap_or(1);
        shelf.saturating_sub(batch - 1)
    }

    /// Spirits working at or headed for a station.
    fn spirits_at(&self, id: BuildingId) -> impl Iterator<Item = &Spirit> {
        self.spirits.iter().filter(move |s| {
            s.actor
                .queue
                .iter()
                .any(|t| matches!(t, Task::Craft { building, .. } if *building == id))
        })
    }

    /// The player has a job at this station.
    pub(crate) fn player_at_station(&self, id: BuildingId) -> bool {
        let session = self.player.session.as_ref().map(|s| &s.kind);
        matches!(session, Some(SessionKind::Crafting { building }) if *building == id)
            || self
                .player
                .actor
                .queue
                .iter()
                .any(|t| matches!(t, Task::Craft { building, .. } if *building == id))
    }

    /// Places at a station in use, counting the player's.
    pub fn slots_used(&self, id: BuildingId) -> u32 {
        self.spirits_at(id).count() as u32 + self.player_at_station(id) as u32
    }

    /// Whether a spirit may take a place here. One place stays free for the
    /// player when they've asked for that.
    pub(crate) fn slot_free_for_spirit(&self, b: &Building) -> bool {
        let slots = self.slots_of(b);
        let promised = self.settlement.training.keep_bench_place
            && self.training_bench().is_some_and(|t| t.id == b.id);
        let held = promised || self.player_at_station(b.id);
        (self.spirits_at(b.id).count() as u32 + held as u32) < slots
    }

    /// Makes room for the player: with a place promised to them, the spirit
    /// that arrived last steps away. Otherwise a full station says so.
    pub(crate) fn claim_player_slot(&mut self, id: BuildingId) -> Result<(), String> {
        let b = &self.buildings[id];
        let slots = self.slots_of(b);
        if self.player_at_station(id) || (self.spirits_at(id).count() as u32) < slots {
            return Ok(());
        }
        if !self.settlement.training.keep_bench_place {
            return Err(format!(
                "All {slots} places at the {} are taken. Keep a place for yourself in the Training tab.",
                b.name
            ));
        }
        let working = self
            .spirits_at(id)
            .filter(|s| matches!(s.actor.queue.front(), Some(Task::Craft { .. })))
            .map(|s| s.id)
            .last();
        if let Some(sid) = working {
            let next = self.tick + self.defs.idle_rethink_ticks;
            let s = &mut self.spirits[sid];
            s.actor.clear_tasks();
            s.next_think = next;
        }
        Ok(())
    }

    fn output_full(&self) -> bool {
        let Some(b) = self.station() else {
            return false;
        };
        let def = self.defs.building(b.kind);
        let limit = def.capacity.saturating_sub(def.input_buffer);
        b.storage.available(self.settlement.order.output) >= limit || b.storage.free() == 0
    }

    pub fn workshop_status(&self) -> WorkshopStatus {
        let order = &self.settlement.order;
        if self.station().is_none() {
            WorkshopStatus::NoStation
        } else if !order.enabled {
            WorkshopStatus::Disabled
        } else if self.production_recipe().is_none() {
            WorkshopStatus::NoWorker
        } else if self.production_deficit() == 0 {
            WorkshopStatus::TargetMet
        } else if self.output_full() {
            WorkshopStatus::OutputFull
        } else if self
            .crafters()
            .any(|s| matches!(s.actor.queue.front(), Some(Task::Craft { .. })))
        {
            WorkshopStatus::Working
        } else {
            WorkshopStatus::WaitingForInputs
        }
    }

    fn sum_time(&self, job: JobKind) -> TimeUse {
        let mut t = TimeUse::default();
        for s in self
            .spirits
            .iter()
            .filter(|s| self.defs.role(s.role).has_job(job))
        {
            t.add(&s.time.recent());
        }
        t
    }

    pub fn production_report(&self) -> ProductionReport {
        let order = &self.settlement.order;
        let input = self.defs.objective.resource;
        let tpm = self.defs.ticks_per_second as f32 * 60.0;
        let window = self.defs.gathering.window_ticks.min(self.tick.max(1));
        let main = &self.main().storage;
        let status = self.workshop_status();
        let gathering = self.sum_time(JobKind::Gather);
        let hauling = self.sum_time(JobKind::Supply);
        let processing = self.sum_time(JobKind::Craft);
        let bottleneck = self.production_bottleneck(status, &gathering);
        let summary = self.production_summary(bottleneck);
        ProductionReport {
            input_in_main: main.get(input),
            output_in_main: main.get(order.output),
            input_at_station: self.station().map_or(0, |b| b.storage.get(input)),
            output_at_station: self.station().map_or(0, |b| b.storage.get(order.output)),
            goal: self.production_goal(),
            status,
            output_per_min: self.output_stats.recent_count() as f32 / window as f32 * tpm,
            workers: self.crafters().filter(|s| s.assigned).count() as u32,
            gathering,
            hauling,
            processing,
            bottleneck,
            summary,
        }
    }

    /// Crafters that have no bench to go to.
    fn waiting_for_bench(&self) -> u32 {
        self.crafters()
            .filter(|s| s.wait == Some(WaitKind::Station))
            .count() as u32
    }

    /// Follows the chain backwards from the station to the first thing short.
    fn production_bottleneck(
        &self,
        status: WorkshopStatus,
        gathering: &TimeUse,
    ) -> ProductionBottleneck {
        if self.waiting_for_bench() > 0 {
            return ProductionBottleneck::WorkshopCapacity;
        }
        match status {
            WorkshopStatus::NoStation | WorkshopStatus::NoWorker => {
                return ProductionBottleneck::NoWorker;
            }
            WorkshopStatus::Disabled => return ProductionBottleneck::Disabled,
            WorkshopStatus::TargetMet => return ProductionBottleneck::None,
            WorkshopStatus::OutputFull => {
                return if self.main().storage.free() == 0 {
                    ProductionBottleneck::Storage
                } else {
                    ProductionBottleneck::Hauling
                };
            }
            WorkshopStatus::Working => return ProductionBottleneck::ProcessingSpeed,
            WorkshopStatus::WaitingForInputs => {}
        }
        let input = self.defs.objective.resource;
        if self.input_spare(input) > 0 {
            return ProductionBottleneck::Hauling;
        }
        if self.main().storage.available(input) > 0 {
            return ProductionBottleneck::InputReserve;
        }
        if gathering.pct_of_working(gathering.waiting_recovery) >= 25 {
            return ProductionBottleneck::TreeRecovery;
        }
        ProductionBottleneck::LogSupply
    }

    fn production_summary(&self, b: ProductionBottleneck) -> String {
        let order = &self.settlement.order;
        let output = &self.defs.resource(order.output).name;
        let input = self.defs.objective.resource;
        let input_name = &self.defs.resource(input).name;
        let stock = self.main().storage.available(input);
        match b {
            ProductionBottleneck::None => {
                format!("{output} target met: production has paused.")
            }
            ProductionBottleneck::Disabled => "Production is switched off.".to_string(),
            ProductionBottleneck::NoWorker => {
                "Train Woodworking, then use Soul Separation to create a Woodworking Spirit."
                    .to_string()
            }
            ProductionBottleneck::LogSupply => format!(
                "Main Storage is out of {input_name}: more gatherers would raise {output} output."
            ),
            ProductionBottleneck::TreeRecovery => format!(
                "Out of {input_name} and gatherers are waiting for trees: open another site."
            ),
            ProductionBottleneck::InputReserve => format!(
                "Only {stock} {input_name} in Main Storage, all held by the reserve of {}. Lower it or add gatherers.",
                order.input_reserve
            ),
            ProductionBottleneck::Hauling => format!(
                "{output} production limited by hauling capacity: another Hauling Spirit would help."
            ),
            ProductionBottleneck::Storage => {
                "Main Storage is full: build a Storehouse or use up stock.".to_string()
            }
            ProductionBottleneck::ProcessingSpeed => {
                "The bench is busy: a higher-level or second Woodworking Spirit would raise output."
                    .to_string()
            }
            ProductionBottleneck::WorkshopCapacity => {
                let n = self.waiting_for_bench();
                let (count, verb, noun) = match n {
                    1 => ("One".to_string(), "is", "Spirit"),
                    2 => ("Two".to_string(), "are", "Spirits"),
                    3 => ("Three".to_string(), "are", "Spirits"),
                    n => (n.to_string(), "are", "Spirits"),
                };
                format!(
                    "{count} Woodworking {noun} {verb} idle. Additional workshop capacity would increase output."
                )
            }
        }
    }

    /// Why an order is or isn't moving, in one sentence.
    fn order_status(&self, o: &ProductionOrder) -> (String, bool) {
        let name = &self.defs.resource(o.output).name;
        if !o.enabled {
            return (format!("{name}: switched off."), false);
        }
        if self.stations_for(o.output).is_empty() {
            return (format!("{name} paused: no workshop."), true);
        }
        let Some(recipe) = self.recipe_for(o.output) else {
            if self.goal_of(o.output) == 0 {
                return (format!("{name}: no target set."), false);
            }
            let tiers = self
                .defs
                .recipes
                .iter()
                .filter(|d| d.output.0 == o.output)
                .min_by_key(|d| d.level_required);
            let text = match tiers {
                Some(d) => format!(
                    "{name} paused: needs a {} Spirit of level {} on production.",
                    self.defs.skill(d.skill).name,
                    d.level_required
                ),
                None => format!("{name} paused: nobody can make it."),
            };
            return (text, true);
        };
        if self.deficit_of(o.output) == 0 {
            return (format!("{name} target met."), false);
        }
        let working = self.crafters().any(|s| {
            matches!(s.actor.queue.front(), Some(Task::Craft { output: Some(x), .. }) if *x == o.output)
        });
        if working {
            return (format!("{name} in production."), false);
        }
        for &(r, n) in &self.defs.recipe(recipe).inputs {
            let stocked = self
                .stations_for(o.output)
                .iter()
                .any(|b| b.storage.available(r) >= n);
            if !stocked {
                let input = &self.defs.resource(r).name;
                return if self.any_spare(r) > 0 {
                    (
                        format!("{name} waiting: hauling capacity is limiting {input} delivery."),
                        true,
                    )
                } else {
                    (
                        format!("{name} production paused: insufficient {input}."),
                        true,
                    )
                };
            }
        }
        if self.waiting_for_bench() > 0 {
            return (format!("{name} waiting for a free bench."), true);
        }
        (format!("{name} waiting for a worker."), true)
    }

    /// One line per production order, most urgent first.
    pub fn order_reports(&self) -> Vec<OrderReport> {
        let tpm = self.defs.ticks_per_second as f32 * 60.0;
        let window = self.defs.gathering.window_ticks.min(self.tick.max(1));
        self.orders()
            .into_iter()
            .map(|o| {
                let (status, stalled) = self.order_status(o);
                let made = self
                    .product_stats
                    .get(&o.output)
                    .map_or(0, |s| s.recent_count());
                OrderReport {
                    output: o.output,
                    name: self.defs.resource(o.output).name.clone(),
                    in_main: self.main().storage.get(o.output),
                    target: o.target,
                    goal: self.goal_of(o.output),
                    enabled: o.enabled,
                    priority: o.priority,
                    per_min: made as f32 / window as f32 * tpm,
                    status,
                    stalled,
                }
            })
            .collect()
    }

    /// One line per workstation.
    pub fn station_reports(&self) -> Vec<StationReport> {
        self.stations()
            .map(|b| StationReport {
                id: b.id,
                name: b.name.clone(),
                occupied: self.slots_used(b.id),
                slots: self.slots_of(b),
                crafted: b.crafted,
                workers: self.spirits_at(b.id).map(|s| s.name.clone()).collect(),
            })
            .collect()
    }
}
