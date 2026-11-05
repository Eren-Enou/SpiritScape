//! Remote production sites. An outpost isn't a new kind of thing: it's ordinary
//! buildings in a real place, with spirits told to work there. What this module
//! adds is the small set of instructions that makes that simple, and plain
//! readings of how it's going.
//!
//! Nothing here moves resources or changes rates. Any advantage an outpost has
//! comes from how far things have to be carried.

use crate::defs::{JobKind, ResourceId, SiteId};
use crate::entities::{Activity, Building, BuildingId, Focus, Spirit};
use crate::events::EventKind;
use crate::metrics::WaitKind;
use crate::world::World;

/// Hardwood kept at the outpost store when it's first staffed.
const DEFAULT_LOCAL_STOCK: u32 = 20;

/// What a spirit is doing, in terms of what the player should do about it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkerState {
    Working,
    /// Nothing to do because everything wanted is done.
    GoalsMet,
    /// Waiting for materials or regrowth.
    NoResources,
    /// Stuck on storage, a full bench or a failed trip.
    Logistics,
    /// Told to rest, off production, or has no project.
    Unassigned,
}

impl WorkerState {
    pub fn label(self) -> &'static str {
        match self {
            WorkerState::Working => "working",
            WorkerState::GoalsMet => "goals met",
            WorkerState::NoResources => "needs resources",
            WorkerState::Logistics => "blocked",
            WorkerState::Unassigned => "awaiting work",
        }
    }
}

/// How one remote site stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutpostReport {
    pub site: SiteId,
    pub name: String,
    pub store: Option<BuildingId>,
    pub workshop: Option<BuildingId>,
    /// The material this site gives.
    pub resource: Option<ResourceId>,
    /// In the store now, and how much it's told to keep.
    pub stock: u32,
    pub keep: u32,
    /// What the local bench has made.
    pub made: u32,
    pub gatherers: Vec<String>,
    pub workers: Vec<String>,
    pub haulers: Vec<String>,
    pub staffed: bool,
    /// The next thing to do, or how it's running.
    pub status: String,
}

/// One material a project needs and what's being done about it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DemandLine {
    pub name: String,
    pub have: u32,
    pub need: u32,
    pub status: String,
}

impl World {
    /// Reads a spirit's state for the player.
    pub fn worker_state(&self, s: &Spirit) -> WorkerState {
        if !s.actor.queue.is_empty() || matches!(s.activity, Activity::Working(_)) {
            return WorkerState::Working;
        }
        let role = self.defs.role(s.role);
        match s.activity {
            Activity::Blocked(_) => match s.wait {
                Some(WaitKind::Recovery | WaitKind::Inputs) => WorkerState::NoResources,
                _ => WorkerState::Logistics,
            },
            _ if s.policy.focus == Focus::Rest => WorkerState::Unassigned,
            _ if role.has_job(JobKind::Construct) => WorkerState::Unassigned,
            _ if role.has_job(JobKind::Craft) && !s.assigned => WorkerState::Unassigned,
            _ => WorkerState::GoalsMet,
        }
    }

    /// The site a place belongs to for instructions: a grove reached through
    /// a site counts as part of it.
    pub(crate) fn post_of(&self, site: SiteId) -> SiteId {
        self.defs.site(site).region.unwrap_or(site)
    }

    /// Whether a place falls under a posting to `post`.
    pub(crate) fn within_post(&self, place: Option<SiteId>, post: SiteId) -> bool {
        place.is_some_and(|s| self.post_of(s) == self.post_of(post))
    }

    /// Whether a building stands inside a site or its region.
    pub(crate) fn in_site(&self, b: &Building, site: SiteId) -> bool {
        self.within_post(self.defs.site_at(b.pos.x, b.pos.y), site)
    }

    /// Sites that can be run as outposts.
    pub fn outpost_sites(&self) -> Vec<SiteId> {
        (0..self.defs.sites.len())
            .map(|i| SiteId(i as u16))
            .filter(|s| self.defs.site(*s).outpost)
            .collect()
    }

    /// The material trees at a site give, the rarest kind first.
    fn site_resource(&self, site: SiteId) -> Option<ResourceId> {
        self.trees
            .iter()
            .filter(|t| t.site == Some(site))
            .max_by_key(|t| self.defs.tree(t.kind).level_required)
            .map(|t| self.defs.tree(t.kind).resource)
    }

    fn site_store(&self, site: SiteId) -> Option<&Building> {
        self.outposts().find(|b| self.in_site(b, site))
    }

    fn site_station(&self, site: SiteId) -> Option<&Building> {
        self.stations().find(|b| self.in_site(b, site))
    }

    /// Whether this spirit can use what the site gives.
    fn can_work_site(&self, s: &Spirit, site: SiteId) -> bool {
        self.trees
            .iter()
            .any(|t| t.site == Some(site) && self.spirit_can_cut(s, t.kind))
    }

    /// Whether this spirit can make something from what the site gives.
    fn can_process(&self, s: &Spirit, r: ResourceId) -> bool {
        self.defs.recipes.iter().any(|d| {
            d.skill == s.skill()
                && d.level_required <= s.level()
                && (!d.chosen_only || self.deficit_of(d.output.0) > 0)
                && d.inputs.iter().any(|i| i.0 == r)
        })
    }

    /// Tells a store how much of a material to keep for local use.
    pub(crate) fn set_local_stock(
        &mut self,
        building: BuildingId,
        resource: ResourceId,
        amount: u32,
    ) -> Result<String, String> {
        if resource.index() >= self.defs.resources.len() {
            return Err("Unknown material".into());
        }
        let Some(b) = self.buildings.get_mut(building) else {
            return Err("That building doesn't exist".into());
        };
        if amount == 0 {
            b.keep.remove(&resource);
        } else {
            b.keep.insert(resource, amount);
        }
        let msg = format!(
            "{} keeps {amount} {} for local use",
            b.name,
            self.defs.resource(resource).name
        );
        self.events.push(self.tick, EventKind::Player, msg.clone());
        Ok(msg)
    }

    /// One instruction for a whole site: everyone who can do its work goes
    /// there. With two or more haulers, one stays with it. Turning it off
    /// sends them back to working anywhere.
    pub(crate) fn set_outpost_staffed(&mut self, site: SiteId, on: bool) -> Result<String, String> {
        if site.index() >= self.defs.sites.len() {
            return Err("Unknown site".into());
        }
        let name = self.defs.site(site).name.clone();
        let station = self.site_station(site).map(|b| b.id);
        let store = self.site_store(site).map(|b| b.id);
        let resource = self.site_resource(site);
        let msg = if on {
            let gatherers: Vec<usize> = self
                .gatherers()
                .filter(|s| self.can_work_site(s, site))
                .map(|s| s.id)
                .collect();
            let crafters: Vec<usize> = self
                .crafters()
                .filter(|s| resource.is_some_and(|r| self.can_process(s, r)))
                .map(|s| s.id)
                .collect();
            if gatherers.is_empty() && crafters.is_empty() {
                return Err(format!("No spirit is skilled enough to work at {name}"));
            }
            for &id in &gatherers {
                let p = &mut self.spirits[id].policy;
                p.site = Some(site);
                p.prefer_advanced = true;
            }
            if station.is_some() {
                for &id in &crafters {
                    self.spirits[id].policy.station = station;
                }
            }
            let haulers: Vec<usize> = self
                .spirits
                .iter()
                .filter(|s| self.defs.role(s.role).has_job(JobKind::Haul))
                .map(|s| s.id)
                .collect();
            if haulers.len() >= 2
                && let Some(&id) = haulers.last()
            {
                self.spirits[id].policy.site = Some(site);
            }
            if let (Some(b), Some(r)) = (store, resource)
                && !self.buildings[b].keep.contains_key(&r)
            {
                self.buildings[b].keep.insert(r, DEFAULT_LOCAL_STOCK);
            }
            let mut msg = format!(
                "{name} staffed: {} gathering, {} at the bench",
                gatherers.len(),
                if station.is_some() { crafters.len() } else { 0 }
            );
            let basic = self.settlement.objective.resource;
            let left = self.gatherers().any(|s| {
                s.policy.site.is_none_or(|p| {
                    self.trees
                        .iter()
                        .any(|t| t.site == Some(p) && self.defs.tree(t.kind).resource == basic)
                })
            });
            if !left {
                msg += &format!(
                    ". Nobody is left gathering {}",
                    self.defs.resource(basic).name
                );
            }
            msg
        } else {
            for s in &mut self.spirits {
                if s.policy.site == Some(site) {
                    s.policy.site = None;
                }
                if station.is_some() && s.policy.station == station {
                    s.policy.station = None;
                }
            }
            format!("Spirits are no longer tied to {name}")
        };
        self.events.push(self.tick, EventKind::Player, msg.clone());
        Ok(msg)
    }

    pub fn outpost_report(&self, site: SiteId) -> OutpostReport {
        let name = self.defs.site(site).name.clone();
        let store = self.site_store(site);
        let station = self.site_station(site);
        let resource = self.site_resource(site);
        let by = |job: JobKind| -> Vec<String> {
            self.spirits
                .iter()
                .filter(|s| self.defs.role(s.role).has_job(job) && s.policy.site == Some(site))
                .map(|s| s.name.clone())
                .collect()
        };
        let workers: Vec<String> = self
            .crafters()
            .filter(|s| station.is_some_and(|b| s.policy.station == Some(b.id)))
            .map(|s| s.name.clone())
            .collect();
        let gatherers = by(JobKind::Gather);
        let haulers = by(JobKind::Haul);
        let staffed = !gatherers.is_empty() || !workers.is_empty();
        let building = |id: &str| {
            self.defs
                .building_kind_id(id)
                .map_or(String::new(), |k| self.defs.building(k).name.clone())
        };
        let under_way = self.active_project().is_some_and(|p| {
            self.defs
                .site_at(p.site.x, p.site.y)
                .is_some_and(|s| s == site)
        });
        let status = if under_way {
            "Under construction. The Project card shows what's still needed.".to_string()
        } else if store.is_none() {
            format!("Place an {} here to start.", building("outpost_storage"))
        } else if station.is_none() {
            format!("Place a {} here to process locally.", building("workshop"))
        } else if !staffed {
            "Built, with nobody assigned. Press Staff outpost.".to_string()
        } else if gatherers.is_empty() {
            "Nobody here can cut these trees.".to_string()
        } else if workers.is_empty() {
            "Nobody here can work the local bench.".to_string()
        } else {
            "Running on its own. Finished goods go home to Main Storage.".to_string()
        };
        OutpostReport {
            site,
            name,
            store: store.map(|b| b.id),
            workshop: station.map(|b| b.id),
            resource,
            stock: match (store, resource) {
                (Some(b), Some(r)) => b.storage.get(r),
                _ => 0,
            },
            keep: match (store, resource) {
                (Some(b), Some(r)) => b.keep.get(&r).copied().unwrap_or(0),
                _ => 0,
            },
            made: station.map_or(0, |b| b.crafted),
            gatherers,
            workers,
            haulers,
            staffed,
            status,
        }
    }

    /// What the current project still needs, and who's on it.
    pub fn project_needs(&self) -> Vec<DemandLine> {
        let Some(p) = self.active_project() else {
            return Vec::new();
        };
        if p.reserved {
            return Vec::new();
        }
        let main = &self.main().storage;
        self.defs
            .building(p.kind)
            .cost
            .iter()
            .map(|&(r, need)| {
                let have = main.available(r).min(need);
                let status = if have >= need {
                    "ready".to_string()
                } else if self.order_for(r).is_some() {
                    let report = self.order_reports().into_iter().find(|o| o.output == r);
                    report.map_or(String::new(), |o| o.status)
                } else if self.gatherers().next().is_some() {
                    "gatherers are bringing more".to_string()
                } else {
                    "nobody can gather this yet".to_string()
                };
                DemandLine {
                    name: self.defs.resource(r).name.clone(),
                    have,
                    need,
                    status,
                }
            })
            .collect()
    }
}

#[doc(hidden)]
impl World {
    pub fn input_spare_for_test(&self, r: ResourceId) -> u32 {
        self.input_spare(r)
    }

    pub fn gatherers_for_test(&self) -> Vec<&Spirit> {
        self.gatherers().collect()
    }

    pub fn slot_free_for_test(&self, b: &Building) -> bool {
        self.slot_free_for_spirit(b)
    }

    pub fn add_worker_for_test(&mut self, station: BuildingId) {
        let role = self.defs.role_id("woodworker").expect("woodworker");
        let pos = self.main().pos;
        let at = crate::grid::TilePos::new(pos.x, pos.y + 1);
        let id = self.add_spirit(role, 388, at);
        self.spirits[id].policy.station = Some(station);
    }

    pub fn project_demand_for_test(&self, r: ResourceId) -> u32 {
        self.project_demand(r)
    }

    pub fn main_room_for_test(&self, r: ResourceId) -> u32 {
        self.main_room_for(r)
    }

    pub fn player_add_xp_for_test(&mut self, skill: crate::defs::SkillId, xp: u64) {
        self.player.add_xp(skill, xp, &self.defs.xp_table);
    }
}
