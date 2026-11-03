//! The settlement's production order: what it wants made, what that demands
//! upstream, and why output is or isn't flowing.
//!
//! Demand runs backwards one step at a time and is always bounded: a plank
//! shortfall becomes a request for logs at the station (for haulers) and a
//! capped addition to the log goal (for gatherers). Nothing here creates or
//! moves resources; workers do that through tasks.

use crate::defs::{JobKind, RecipeId, ResourceId};
use crate::entities::{Building, BuildingId, Spirit};
use crate::metrics::TimeUse;
use crate::tasks::Task;
use crate::world::World;

/// A standing instruction: keep `target` units of `output` in Main Storage
/// without taking raw materials below `input_reserve`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProductionOrder {
    pub output: ResourceId,
    pub target: u32,
    pub input_reserve: u32,
    pub enabled: bool,
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

impl World {
    /// The building producing the order's output.
    pub fn station(&self) -> Option<&Building> {
        let order = &self.settlement.order;
        self.buildings.iter().find(|b| {
            b.is_complete()
                && self.defs.building(b.kind).is_station
                && self
                    .defs
                    .station_recipes(b.kind)
                    .iter()
                    .any(|r| self.defs.recipe(*r).output.0 == order.output)
        })
    }

    /// Spirits that work at stations.
    pub fn crafters(&self) -> impl Iterator<Item = &Spirit> {
        self.spirits
            .iter()
            .filter(|s| self.defs.role(s.role).has_job(JobKind::Craft))
    }

    /// The best recipe any assigned worker knows, used to estimate demand.
    pub fn production_recipe(&self) -> Option<RecipeId> {
        let station = self.station()?;
        let level = self
            .crafters()
            .filter(|s| s.assigned)
            .map(|s| s.level())
            .max()?;
        self.defs.best_recipe(station.kind, level, |_| u32::MAX)
    }

    /// Output still to be reserved by active projects.
    fn project_demand(&self, r: ResourceId) -> u32 {
        self.projects
            .iter()
            .filter(|p| p.is_active() && !p.reserved)
            .flat_map(|p| self.defs.building(p.kind).cost.iter())
            .filter(|(res, _)| *res == r)
            .map(|(_, n)| *n)
            .sum()
    }

    /// The stock the order aims for: its target plus unreserved project needs.
    pub fn production_goal(&self) -> u32 {
        let order = &self.settlement.order;
        order.target + self.project_demand(order.output)
    }

    /// Units the order still wants made. Zero while nobody could make them.
    pub fn production_deficit(&self) -> u32 {
        let order = &self.settlement.order;
        if !order.enabled || self.production_recipe().is_none() {
            return 0;
        }
        self.production_goal()
            .saturating_sub(self.projected_stock(order.output))
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

    /// Units of each input that the current shortfall would consume, and how
    /// many of them are already at or on their way to the station.
    fn input_need(&self) -> Vec<(ResourceId, u32, u32)> {
        let (Some(recipe), Some(station)) = (self.production_recipe(), self.station()) else {
            return Vec::new();
        };
        let def = self.defs.recipe(recipe);
        let deficit = self.production_deficit();
        let batches = deficit.div_ceil(def.output.1);
        def.inputs
            .iter()
            .map(|&(r, n)| {
                let have = station.storage.available(r) + self.incoming(station.id, r);
                (r, batches * n, have)
            })
            .collect()
    }

    /// The raw material a hauler should bring to the station, and how much.
    /// Bounded by the station's buffer and the current shortfall.
    pub(crate) fn input_wanted(&self) -> Option<(ResourceId, u32)> {
        let station = self.station()?;
        let def = self.defs.building(station.kind);
        self.input_need().into_iter().find_map(|(r, need, have)| {
            let cap = need.min(def.input_buffer);
            let want = cap.saturating_sub(have).min(station.storage.free());
            (want > 0).then_some((r, want))
        })
    }

    /// Extra stock of `r` that production asks gatherers for, on top of the
    /// settlement's own goal. Capped, so a large target can't demand everything.
    pub fn upstream_demand(&self, r: ResourceId) -> u32 {
        self.input_need()
            .into_iter()
            .filter(|(res, ..)| *res == r)
            .map(|(_, need, have)| need.saturating_sub(have))
            .sum::<u32>()
            .min(self.defs.production.upstream_cap)
    }

    /// What Main Storage can give to production without breaking the reserve.
    pub(crate) fn input_spare(&self, r: ResourceId) -> u32 {
        self.main()
            .storage
            .available(r)
            .saturating_sub(self.settlement.order.input_reserve)
    }

    /// The stock level gatherers work toward.
    pub fn gather_goal(&self) -> u32 {
        let o = &self.settlement.objective;
        let order = &self.settlement.order;
        if !order.enabled || self.production_recipe().is_none() {
            return o.target;
        }
        o.target.max(order.input_reserve) + self.upstream_demand(o.resource)
    }

    /// Finished goods at the station that nobody has claimed yet.
    pub(crate) fn output_waiting(&self) -> u32 {
        self.station()
            .map_or(0, |b| b.storage.available(self.settlement.order.output))
    }

    /// Whether finished goods at the station are worth a trip: a full load,
    /// or anything at all once nobody is adding to them.
    pub(crate) fn output_ready(&self, load: u32) -> bool {
        let waiting = self.output_waiting();
        let crafting = self
            .crafters()
            .any(|s| matches!(s.actor.queue.front(), Some(Task::Craft { .. })));
        waiting >= load.max(1) || (waiting > 0 && !crafting)
    }

    fn output_full(&self) -> bool {
        let Some(b) = self.station() else {
            return false;
        };
        let def = self.defs.building(b.kind);
        let limit = def.capacity.saturating_sub(def.input_buffer);
        b.storage.get(self.settlement.order.output) >= limit || b.storage.free() == 0
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

    /// Follows the chain backwards from the station to the first thing short.
    fn production_bottleneck(
        &self,
        status: WorkshopStatus,
        gathering: &TimeUse,
    ) -> ProductionBottleneck {
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
        }
    }
}
