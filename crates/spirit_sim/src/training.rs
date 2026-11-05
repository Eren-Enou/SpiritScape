//! Spirit-supported training: the settlement makes the player's own skilling
//! more convenient without doing any of it for them.
//!
//! The player still performs every action that earns XP. Spirits only carry
//! things: a Hauling Spirit empties the Training Cache into Main Storage and
//! brings logs to a part of the bench set aside for the player. Those trips
//! take real time and compete with production for the same hauler.

use crate::behavior::{Blocked, JobPlan};
use crate::defs::{BuildingKindId, JobKind, ResourceId, SiteId};
use crate::entities::{Building, Focus, SpiritId};
use crate::events::EventKind;
use crate::metrics::WaitKind;
use crate::tasks::{Goal, Task};
use crate::world::World;
use serde::{Deserialize, Serialize};

/// The player's standing instructions for training support.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrainingSupport {
    /// The site reserved for the player. Spirits don't gather there.
    pub area: Option<SiteId>,
    /// A Hauling Spirit empties the Training Cache.
    pub collect: bool,
    /// A Hauling Spirit keeps logs at the bench for the player.
    pub supply: bool,
    /// Support comes before production, instead of after it.
    pub training_first: bool,
    /// Spirits leave one place free for the player at every bench.
    #[serde(default)]
    pub keep_bench_place: bool,
}

/// How training support stands right now.
#[derive(Clone, Debug)]
pub struct TrainingReport {
    pub area: Option<String>,
    /// Stock and capacity of the Training Cache, once it exists.
    pub cache: Option<(u32, u32)>,
    /// Logs set aside at the bench, and how many it can hold.
    pub bench: (u32, u32),
    /// A readable sentence on each kind of support.
    pub collect_status: String,
    pub supply_status: String,
}

impl World {
    /// The Training Cache building kind.
    pub fn training_cache_kind(&self) -> Option<BuildingKindId> {
        let i = self.defs.buildings.iter().position(|b| b.training_cache)?;
        Some(BuildingKindId(i as u16))
    }

    /// The finished Training Cache.
    pub fn training_cache(&self) -> Option<&Building> {
        let kind = self.training_cache_kind()?;
        self.buildings
            .iter()
            .find(|b| b.kind == kind && b.is_complete())
    }

    fn training_input(&self) -> ResourceId {
        self.defs.objective.resource
    }

    /// Logs set aside for the player at the bench.
    pub fn bench_stock(&self) -> u32 {
        self.station()
            .map_or(0, |b| b.storage.reserved(self.training_input()))
    }

    /// Whether the player's planks stay at the bench for a hauler to collect.
    pub(crate) fn player_output_at_station(&self) -> bool {
        self.settlement.training.supply && self.has_worker_for(JobKind::Collect)
    }

    /// Logs on their way to the bench for the player.
    fn bench_incoming(&self) -> u32 {
        let r = self.training_input();
        self.spirits
            .iter()
            .filter(|s| s.job == Some(JobKind::TrainingSupply))
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

    /// Logs the bench could still take for the player.
    fn bench_shortfall(&self) -> u32 {
        let Some(station) = self.station() else {
            return 0;
        };
        let buffer = self.bench_size(station);
        buffer
            .saturating_sub(self.bench_stock() + self.bench_incoming())
            .min(station.storage.free())
    }

    /// Logs a bench can hold for the player.
    fn bench_size(&self, station: &Building) -> u32 {
        self.defs.building(station.kind).training_buffer + self.settlement.bench_bonus
    }

    /// Extra stock gatherers should keep while a spirit is dedicated to the
    /// player's training: a bench's worth, so the next refill is ready.
    pub(crate) fn training_gather_demand(&self, r: ResourceId) -> u32 {
        let dedicated = self.settlement.training.supply
            && r == self.training_input()
            && self.gatherers().any(|s| s.policy.focus == Focus::Training);
        match self.station() {
            Some(b) if dedicated => self.bench_size(b),
            _ => 0,
        }
    }

    pub(crate) fn training_pickup_ready(&self, sid: SpiritId) -> bool {
        let min = self.defs.role(self.spirits[sid].role).haul_min;
        self.settlement.training.collect
            && self
                .training_cache()
                .is_some_and(|b| b.storage.total() >= min)
    }

    /// A trip is worth it: the bench can take a load and there are logs to spare.
    pub(crate) fn training_supply_ready(&self, sid: SpiritId) -> bool {
        let min = self.defs.role(self.spirits[sid].role).haul_min;
        self.settlement.training.supply
            && self.bench_shortfall() >= min
            && self.input_spare(self.training_input()) >= min
    }

    /// Empty the Training Cache into Main Storage.
    pub(crate) fn plan_training_collect(&self, sid: SpiritId) -> Result<JobPlan, Blocked> {
        let s = &self.spirits[sid];
        let cache = self
            .training_cache()
            .ok_or_else(|| "No Training Cache".to_string())?;
        let main = self.main();
        if main.storage.free() == 0 {
            return Err(Blocked::new(WaitKind::Storage, "Main Storage is full"));
        }
        let (&resource, &amount) = cache
            .storage
            .contents()
            .iter()
            .max_by_key(|(r, n)| (**n, std::cmp::Reverse(**r)))
            .ok_or_else(|| "The Training Cache is empty".to_string())?;
        let amount = amount
            .min(s.actor.inventory.free())
            .min(main.storage.free());
        Ok(JobPlan {
            description: format!(
                "Collecting {amount} {} from your {}",
                self.defs.resource(resource).name,
                cache.name
            ),
            tasks: vec![
                Task::Move {
                    goal: Goal::Adjacent(cache.pos),
                    label: cache.name.clone(),
                },
                Task::Withdraw {
                    building: cache.id,
                    resource,
                    amount,
                    reserved: false,
                },
                Task::Move {
                    goal: Goal::Adjacent(main.pos),
                    label: main.name.clone(),
                },
                Task::Deposit { building: main.id },
            ],
            ..Default::default()
        })
    }

    /// Carry logs above the reserve to the player's part of the bench.
    pub(crate) fn plan_training_supply(&self, sid: SpiritId) -> Result<JobPlan, Blocked> {
        let s = &self.spirits[sid];
        let station = self
            .station()
            .ok_or_else(|| "No workshop available".to_string())?;
        let resource = self.training_input();
        let name = &self.defs.resource(resource).name;
        let amount = self
            .bench_shortfall()
            .min(self.input_spare(resource))
            .min(s.actor.inventory.free());
        if amount == 0 {
            return Err(Blocked::new(
                WaitKind::Inputs,
                format!("No spare {name} for your training"),
            ));
        }
        let main = self.main();
        Ok(JobPlan {
            description: format!("Bringing {amount} {name} to the bench for your training"),
            tasks: vec![
                Task::Move {
                    goal: Goal::Adjacent(main.pos),
                    label: main.name.clone(),
                },
                Task::Withdraw {
                    building: main.id,
                    resource,
                    amount,
                    reserved: true,
                },
                Task::Move {
                    goal: Goal::Adjacent(station.pos),
                    label: station.name.clone(),
                },
                Task::DepositForPlayer {
                    building: station.id,
                },
            ],
            reserve: Some((main.id, resource, amount)),
            ..Default::default()
        })
    }

    pub(crate) fn set_training_area(&mut self, site: Option<SiteId>) -> Result<String, String> {
        if site.is_some_and(|s| s.index() >= self.defs.sites.len()) {
            return Err("Unknown site".into());
        }
        if let Some(old) = self.settlement.training.area {
            self.settlement.site_open[old.index()] = true;
        }
        self.settlement.training.area = site;
        let Some(site) = site else {
            let msg = "You no longer keep a training area".to_string();
            self.events.push(self.tick, EventKind::Player, msg.clone());
            return Ok(msg);
        };
        self.settlement.site_open[site.index()] = false;
        let mut msg = format!(
            "{} is your training area: spirits will not gather there",
            self.defs.site(site).name
        );
        let stranded: Vec<String> = self
            .gatherers()
            .filter(|s| !self.can_gather_somewhere(s.id))
            .map(|s| s.name.clone())
            .collect();
        if !stranded.is_empty() {
            msg += &format!(
                ". {} has no open site with trees it can cut",
                stranded.join(", ")
            );
        }
        self.events.push(self.tick, EventKind::Player, msg.clone());
        Ok(msg)
    }

    fn can_gather_somewhere(&self, sid: SpiritId) -> bool {
        let s = &self.spirits[sid];
        self.trees.iter().any(|t| {
            self.spirit_can_cut(s, t.kind)
                && t.site.is_none_or(|i| self.settlement.site_open[i.index()])
        })
    }

    pub(crate) fn set_training_collect(&mut self, on: bool) -> String {
        self.settlement.training.collect = on;
        let msg = if on {
            "Hauling Spirits will empty your Training Cache"
        } else {
            "Hauling Spirits will leave your Training Cache alone"
        };
        self.events.push(self.tick, EventKind::Player, msg);
        msg.into()
    }

    /// Turning supply off hands what's on the bench back to production.
    pub(crate) fn set_training_supply(&mut self, on: bool) -> String {
        self.settlement.training.supply = on;
        if !on {
            let r = self.training_input();
            if let Some(id) = self.station().map(|b| b.id) {
                let held = self.buildings[id].storage.reserved(r);
                self.buildings[id].storage.release_reserved(r, held);
            }
        }
        let msg = if on {
            "Hauling Spirits will keep logs at the bench for your training"
        } else {
            "The settlement no longer sets logs aside for your training"
        };
        self.events.push(self.tick, EventKind::Player, msg);
        msg.into()
    }

    pub(crate) fn set_training_first(&mut self, on: bool) -> String {
        self.settlement.training.training_first = on;
        let msg = if on {
            "Hauling Spirits now put your training ahead of production"
        } else {
            "Hauling Spirits now put production ahead of your training"
        };
        self.events.push(self.tick, EventKind::Player, msg);
        msg.into()
    }

    /// A hauler finished a Training Cache trip.
    pub(crate) fn on_training_collected(&mut self, sid: SpiritId, amount: u32) {
        let name = self.spirits[sid].name.clone();
        let msg = format!("{name} carried {amount} of your logs to Main Storage");
        self.log_spirit(sid, "training:collected".into(), msg);
        // Once is enough to show what's happening; the log has the rest.
        if !self.notices.contains("collected your training resources") {
            let role = self.defs.role(self.spirits[sid].role).name.clone();
            self.notices.push(
                self.tick,
                format!("Your {role} has collected your training resources."),
            );
        }
    }

    pub fn training_report(&self) -> TrainingReport {
        let t = &self.settlement.training;
        let hauler = self.has_role_with(JobKind::TrainingCollect);
        let r = self.training_input();
        let name = &self.defs.resource(r).name;
        let collect_status = if !t.collect {
            "Off: you carry your own logs.".to_string()
        } else if !hauler {
            "No Hauling Spirit to collect them.".to_string()
        } else if self.training_cache().is_none() {
            "Build a Training Cache to drop your logs nearby.".to_string()
        } else if self.main().storage.free() == 0 {
            "Main Storage is full: nothing can be collected.".to_string()
        } else {
            "A Hauling Spirit empties the cache when a load is waiting.".to_string()
        };
        let spare = self.input_spare(r);
        let supply_status = if !t.supply {
            "Off: fetch your own logs from Main Storage.".to_string()
        } else if !hauler {
            "No Hauling Spirit to bring them.".to_string()
        } else if self.station().is_none() {
            "No workshop.".to_string()
        } else if self.bench_shortfall() == 0 {
            "The bench is stocked.".to_string()
        } else if spare == 0 {
            format!(
                "No spare {name}: the settlement is keeping its reserve of {}.",
                self.settlement.order.input_reserve
            )
        } else {
            format!("{spare} spare {name} available to bring to the bench.")
        };
        let bench_size = self.station().map_or(0, |b| self.bench_size(b));
        TrainingReport {
            area: t.area.map(|s| self.defs.site(s).name.clone()),
            cache: self
                .training_cache()
                .map(|b| (b.storage.total(), b.storage.capacity)),
            bench: (self.bench_stock(), bench_size),
            collect_status,
            supply_status,
        }
    }
}
