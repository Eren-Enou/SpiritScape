//! Gathering and logistics measurements: how workers spend their time, how
//! much each site produces, and what currently limits output.
//!
//! Everything recorded here is integer ticks and counts, so it is part of the
//! deterministic state. Rates and shares are derived only when reporting.

use crate::defs::{JobKind, SiteId};
use crate::entities::{Activity, Spirit};
use crate::world::World;
use std::collections::VecDeque;

/// Why a worker is not working right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaitKind {
    /// No grown tree it may cut is free: waiting for trees to recover.
    Recovery,
    /// Nowhere to put what it gathers or hauls.
    Storage,
    /// A station or its supplier is short of raw materials.
    Inputs,
    Other,
}

/// Ticks spent in each kind of activity.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct TimeUse {
    pub gathering: u64,
    pub traveling: u64,
    /// Depositing, collecting and supplying sites.
    pub handling: u64,
    pub building: u64,
    pub crafting: u64,
    pub waiting_recovery: u64,
    pub waiting_input: u64,
    pub blocked_storage: u64,
    pub blocked_other: u64,
    pub idle: u64,
}

impl TimeUse {
    pub fn total(&self) -> u64 {
        self.working() + self.idle
    }

    /// Everything except idling because there was nothing to do.
    pub fn working(&self) -> u64 {
        self.gathering
            + self.traveling
            + self.handling
            + self.building
            + self.crafting
            + self.waiting_recovery
            + self.waiting_input
            + self.blocked_storage
            + self.blocked_other
    }

    pub fn since(&self, earlier: &TimeUse) -> TimeUse {
        TimeUse {
            gathering: self.gathering - earlier.gathering,
            traveling: self.traveling - earlier.traveling,
            handling: self.handling - earlier.handling,
            building: self.building - earlier.building,
            crafting: self.crafting - earlier.crafting,
            waiting_recovery: self.waiting_recovery - earlier.waiting_recovery,
            waiting_input: self.waiting_input - earlier.waiting_input,
            blocked_storage: self.blocked_storage - earlier.blocked_storage,
            blocked_other: self.blocked_other - earlier.blocked_other,
            idle: self.idle - earlier.idle,
        }
    }

    pub fn add(&mut self, other: &TimeUse) {
        self.gathering += other.gathering;
        self.traveling += other.traveling;
        self.handling += other.handling;
        self.building += other.building;
        self.crafting += other.crafting;
        self.waiting_recovery += other.waiting_recovery;
        self.waiting_input += other.waiting_input;
        self.blocked_storage += other.blocked_storage;
        self.blocked_other += other.blocked_other;
        self.idle += other.idle;
    }

    /// `part` as a whole-number percentage of working time.
    pub fn pct_of_working(&self, part: u64) -> u32 {
        (part * 100).checked_div(self.working()).unwrap_or(0) as u32
    }
}

/// Snapshot interval for rolling windows.
pub(crate) const SNAPSHOT_TICKS: u64 = 100;

/// A cumulative counter with periodic snapshots, so the last `window` ticks
/// can be read without storing every tick.
#[derive(Clone, Debug, Default)]
pub struct TimeLog {
    pub total: TimeUse,
    snapshots: VecDeque<(u64, TimeUse)>,
}

impl TimeLog {
    pub(crate) fn snapshot(&mut self, tick: u64, window: u64) {
        self.snapshots.push_back((tick, self.total));
        while self
            .snapshots
            .front()
            .is_some_and(|(t, _)| *t + window < tick)
        {
            self.snapshots.pop_front();
        }
    }

    /// Time use over (roughly) the last window, or since the start.
    pub fn recent(&self) -> TimeUse {
        match self.snapshots.front() {
            Some((_, old)) => self.total.since(old),
            None => self.total,
        }
    }
}

/// Harvest record for one site.
#[derive(Clone, Debug, Default)]
pub struct SiteStats {
    pub harvested_total: u64,
    /// Tick of each unit harvested within the measurement window.
    recent: VecDeque<u64>,
}

impl SiteStats {
    pub(crate) fn record(&mut self, tick: u64, window: u64) {
        self.harvested_total += 1;
        self.recent.push_back(tick);
        self.trim(tick, window);
    }

    pub(crate) fn trim(&mut self, tick: u64, window: u64) {
        while self.recent.front().is_some_and(|t| *t + window <= tick) {
            self.recent.pop_front();
        }
    }

    pub fn recent_count(&self) -> usize {
        self.recent.len()
    }
}

/// What limits the gathering operation right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bottleneck {
    NoWorkers,
    /// Workers are mostly idle because the stock target is met.
    DemandMet,
    ResourceRecovery,
    HaulingCapacity,
    StorageCapacity,
    TravelDistance,
    GatheringSpeed,
}

impl Bottleneck {
    pub fn label(self) -> &'static str {
        match self {
            Bottleneck::NoWorkers => "No gatherers",
            Bottleneck::DemandMet => "None (demand met)",
            Bottleneck::ResourceRecovery => "Resource recovery",
            Bottleneck::HaulingCapacity => "Hauling capacity",
            Bottleneck::StorageCapacity => "Storage capacity",
            Bottleneck::TravelDistance => "Travel distance",
            Bottleneck::GatheringSpeed => "Gathering speed",
        }
    }
}

#[derive(Clone, Debug)]
pub struct SiteReport {
    pub site: SiteId,
    pub name: String,
    pub spirits_allowed: bool,
    pub trees_total: u32,
    pub trees_grown: u32,
    pub logs_available: u32,
    pub logs_max: u32,
    /// Measured over the window, in units per minute.
    pub measured_per_min: f32,
    /// Long-run regrowth limit: logs per tree / regrow time, summed.
    pub sustainable_per_min: f32,
    /// Gathering spirits working (or waiting) at this site.
    pub workers: u32,
}

#[derive(Clone, Debug)]
pub struct GatheringReport {
    pub sites: Vec<SiteReport>,
    /// Combined recent time use of gathering spirits.
    pub gatherers: TimeUse,
    pub gatherer_count: u32,
    /// What the gatherers could cut if they did nothing but chop.
    pub chop_capacity_per_min: f32,
    pub bottleneck: Bottleneck,
    /// One or two readable sentences.
    pub summary: String,
}

impl World {
    /// Measured gathering rate, sustainable output, time use and bottleneck.
    pub fn gathering_report(&self) -> GatheringReport {
        let tpm = self.defs.ticks_per_second as f32 * 60.0;
        let window = self.defs.gathering.window_ticks.min(self.tick.max(1));
        let mut sites = Vec::new();
        for (i, def) in self.defs.sites.iter().enumerate() {
            let site = SiteId(i as u16);
            let trees: Vec<_> = self.trees.iter().filter(|t| t.site == Some(site)).collect();
            let sustainable: f32 = trees
                .iter()
                .map(|t| self.defs.tree(t.kind).logs as f32 / t.regrow_ticks.max(1) as f32 * tpm)
                .sum();
            sites.push(SiteReport {
                site,
                name: def.name.clone(),
                spirits_allowed: self.settlement.site_open[i],
                trees_total: trees.len() as u32,
                trees_grown: trees.iter().filter(|t| t.is_grown()).count() as u32,
                logs_available: trees.iter().map(|t| t.logs_left()).sum(),
                logs_max: trees.iter().map(|t| self.defs.tree(t.kind).logs).sum(),
                measured_per_min: self.site_stats[i].recent_count() as f32 / window as f32 * tpm,
                sustainable_per_min: sustainable,
                workers: self
                    .gatherers()
                    .filter(|s| {
                        s.home_site == Some(site) && !matches!(s.activity, Activity::Idle(_))
                    })
                    .count() as u32,
            });
        }

        let mut gatherers = TimeUse::default();
        let mut count = 0;
        let mut capacity = 0.0;
        for s in self.gatherers() {
            gatherers.add(&s.time.recent());
            count += 1;
            let kind = s
                .last_tree_kind
                .unwrap_or_else(|| self.defs.tree_tiers(s.skill())[0]);
            capacity += tpm / self.defs.tree(kind).chop_ticks_at(s.level()) as f32;
        }
        let bottleneck = self.bottleneck(&gatherers, count);
        let summary = self.summarize(&sites, &gatherers, bottleneck);
        GatheringReport {
            sites,
            gatherers,
            gatherer_count: count,
            chop_capacity_per_min: capacity,
            bottleneck,
            summary,
        }
    }

    pub(crate) fn gatherers(&self) -> impl Iterator<Item = &Spirit> {
        self.spirits
            .iter()
            .filter(|s| self.defs.role(s.role).has_job(JobKind::Gather))
    }

    fn bottleneck(&self, t: &TimeUse, count: u32) -> Bottleneck {
        if count == 0 {
            return Bottleneck::NoWorkers;
        }
        if t.working() * 2 < t.total() {
            return Bottleneck::DemandMet;
        }
        if t.pct_of_working(t.waiting_recovery) >= 25 {
            return Bottleneck::ResourceRecovery;
        }
        if t.pct_of_working(t.blocked_storage) >= 10 || self.outpost_nearly_full() {
            let main_has_room = self.main().storage.free() > 0;
            return if main_has_room && self.outpost_nearly_full() {
                Bottleneck::HaulingCapacity
            } else {
                Bottleneck::StorageCapacity
            };
        }
        if t.pct_of_working(t.traveling) >= 40 {
            return Bottleneck::TravelDistance;
        }
        Bottleneck::GatheringSpeed
    }

    /// A completed outpost storage is at least 80% full.
    fn outpost_nearly_full(&self) -> bool {
        self.outposts()
            .any(|b| b.storage.capacity > 0 && b.storage.total() * 5 >= b.storage.capacity * 4)
    }

    fn summarize(&self, sites: &[SiteReport], t: &TimeUse, b: Bottleneck) -> String {
        let busiest = sites
            .iter()
            .max_by(|a, b| a.measured_per_min.total_cmp(&b.measured_per_min));
        let production = match busiest {
            Some(s) if s.measured_per_min > 0.0 => format!(
                "{} is producing {:.0} logs/min (sustainable ~{:.0}). ",
                s.name, s.measured_per_min, s.sustainable_per_min
            ),
            _ => String::new(),
        };
        let travel = t.pct_of_working(t.traveling);
        let reason = match b {
            Bottleneck::NoWorkers => "No gathering spirits yet.".to_string(),
            Bottleneck::DemandMet => {
                "Gatherers are mostly idle: the stock target is met.".to_string()
            }
            Bottleneck::ResourceRecovery => format!(
                "Workers could gather faster, but available trees are recovering ({}% of working time spent waiting).",
                t.pct_of_working(t.waiting_recovery)
            ),
            Bottleneck::HaulingCapacity => {
                "Field storage is filling up faster than haulers can empty it.".to_string()
            }
            Bottleneck::StorageCapacity => {
                "Storage is full: there is nowhere to put logs.".to_string()
            }
            Bottleneck::TravelDistance => {
                format!("Gatherers spend {travel}% of their working time walking.")
            }
            Bottleneck::GatheringSpeed => format!(
                "Gatherers spend most of their time chopping ({}%); more or higher-level spirits would raise output.",
                t.pct_of_working(t.gathering)
            ),
        };
        format!("{production}{reason}")
    }
}

impl Spirit {
    /// Adds this tick to the matching time-use bucket.
    pub(crate) fn account_tick(&mut self) {
        use crate::tasks::Task;
        let t = &mut self.time.total;
        match self.actor.queue.front() {
            Some(Task::Chop { .. }) => t.gathering += 1,
            Some(Task::Move { .. }) => t.traveling += 1,
            Some(Task::Build { .. }) => t.building += 1,
            Some(Task::Craft { .. }) => t.crafting += 1,
            Some(_) => t.handling += 1,
            None => match (&self.activity, self.wait) {
                (Activity::Blocked(_), Some(WaitKind::Recovery)) => t.waiting_recovery += 1,
                (Activity::Blocked(_), Some(WaitKind::Storage)) => t.blocked_storage += 1,
                (Activity::Blocked(_), Some(WaitKind::Inputs)) => t.waiting_input += 1,
                (Activity::Blocked(_), _) => t.blocked_other += 1,
                _ => t.idle += 1,
            },
        }
    }
}
