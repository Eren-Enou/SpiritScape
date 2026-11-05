//! Spirit decision making: evaluate prioritized job rules against the world
//! and expand the chosen job into a task queue.

use crate::defs::{Condition, DeliverPolicy, JobKind, ResourceId, SiteId, TreeKindId};
use crate::entities::{
    Activity, Building, BuildingId, Focus, ProjectState, Spirit, SpiritId, Tree, TreeId,
};
use crate::events::EventKind;
use crate::metrics::WaitKind;
use crate::tasks::{Goal, Task};
use crate::world::World;

/// A planned job, ready to be installed on a spirit.
#[derive(Default)]
pub(crate) struct JobPlan {
    pub(crate) description: String,
    pub(crate) tasks: Vec<Task>,
    pub(crate) claim_tree: Option<TreeId>,
    pub(crate) deliver_target: Option<BuildingId>,
    /// Site and tree type of a gathering job.
    pub(crate) gather: Option<(SiteId, TreeKindId)>,
    /// Stock set aside for this trip when the job starts.
    pub(crate) reserve: Option<(BuildingId, ResourceId, u32)>,
}

/// Why a job can't be planned right now.
pub(crate) struct Blocked {
    kind: WaitKind,
    reason: String,
}

impl Blocked {
    pub(crate) fn new(kind: WaitKind, reason: impl Into<String>) -> Self {
        Self {
            kind,
            reason: reason.into(),
        }
    }
}

impl From<&str> for Blocked {
    fn from(reason: &str) -> Self {
        Blocked::new(WaitKind::Other, reason)
    }
}

impl From<String> for Blocked {
    fn from(reason: String) -> Self {
        Blocked::new(WaitKind::Other, reason)
    }
}

impl World {
    pub(crate) fn condition_holds(&self, sid: SpiritId, cond: Condition) -> bool {
        let s = &self.spirits[sid];
        match cond {
            Condition::Always => true,
            Condition::InventoryFull => s.actor.inventory.is_full(),
            Condition::CarryingAny => s.actor.inventory.total() > 0,
            Condition::ObjectiveUnmet => {
                (self.objective_unmet() && self.basic_in_reach(sid)) || self.advanced_wanted(sid)
            }
            Condition::OutpostStockAvailable => self.haul_source(sid).is_some(),
            Condition::ProjectAssigned => self.project_for(sid).is_some(),
            Condition::ProductionWanted => {
                s.assigned
                    && self
                        .orders()
                        .iter()
                        .any(|o| self.deficit_of(o.output) > 0 && self.can_make(s, o.output))
            }
            Condition::WorkshopNeedsInput => self.input_wanted(sid).is_some(),
            Condition::WorkshopOutputReady => {
                self.output_ready(sid, self.defs.role(s.role).haul_min)
            }
            Condition::OutputUnattended => {
                !self.has_role_with(JobKind::Haul)
                    && self.output_ready(sid, s.actor.inventory.capacity)
            }
            Condition::TrainingPickupUrgent => {
                self.settlement.training.training_first && self.training_pickup_ready(sid)
            }
            Condition::TrainingPickupReady => self.training_pickup_ready(sid),
            Condition::TrainingSupplyUrgent => {
                self.settlement.training.training_first && self.training_supply_ready(sid)
            }
            Condition::TrainingSupplyWanted => self.training_supply_ready(sid),
        }
    }

    /// Why a condition currently holds, for the event log.
    fn condition_reason(&self, sid: SpiritId, cond: Condition) -> String {
        match cond {
            Condition::Always => "standing instruction".into(),
            Condition::InventoryFull => "inventory is full".into(),
            Condition::CarryingAny => "carrying items".into(),
            Condition::ObjectiveUnmet => {
                let o = &self.settlement.objective;
                format!(
                    "objective unmet ({}/{} {} counted toward Main Storage)",
                    self.projected_stock(o.resource),
                    o.target,
                    self.defs.resource(o.resource).name
                )
            }
            Condition::OutpostStockAvailable => {
                let parts: Vec<String> = self
                    .outposts()
                    .filter(|b| b.storage.total() > 0)
                    .map(|b| format!("{} holds {}", b.name, b.storage.total()))
                    .collect();
                parts.join(", ")
            }
            Condition::ProjectAssigned => match self.project_for(sid) {
                Some(p) => format!(
                    "assigned to the {} project",
                    self.defs.building(self.projects[p].kind).name
                ),
                None => "no project".into(),
            },
            Condition::ProductionWanted | Condition::WorkshopNeedsInput => {
                let order = &self.settlement.order;
                format!(
                    "production short ({}/{} {})",
                    self.projected_stock(order.output),
                    self.production_goal(),
                    self.defs.resource(order.output).name
                )
            }
            Condition::WorkshopOutputReady | Condition::OutputUnattended => {
                format!("{} finished at the workshop", self.output_waiting())
            }
            Condition::TrainingPickupUrgent | Condition::TrainingPickupReady => {
                "logs waiting in the Training Cache".into()
            }
            Condition::TrainingSupplyUrgent | Condition::TrainingSupplyWanted => {
                "the bench is short of training logs".into()
            }
        }
    }

    pub(crate) fn project_for(&self, sid: SpiritId) -> Option<usize> {
        self.projects
            .iter()
            .position(|p| p.is_active() && p.assigned == Some(sid))
    }

    /// Picks the highest-priority applicable job and installs its tasks.
    pub(crate) fn think(&mut self, sid: SpiritId) {
        for t in &mut self.trees {
            if t.claimed_by == Some(sid) {
                t.claimed_by = None;
            }
        }
        let role = self.defs.role(self.spirits[sid].role).clone();
        let previous = self.spirits[sid].activity.clone();
        let mut blocked: Option<Blocked> = None;

        let focus = self.spirits[sid].policy.focus;
        for rule in &role.jobs {
            if !focus.allows(rule.job) || !self.condition_holds(sid, rule.when) {
                continue;
            }
            match self.plan_job(sid, rule.job) {
                Ok(plan) => {
                    let why = self.condition_reason(sid, rule.when);
                    self.install(sid, rule.job, plan, &previous, &why);
                    return;
                }
                Err(reason) => {
                    blocked.get_or_insert(reason);
                }
            }
        }

        let s = &mut self.spirits[sid];
        s.job = None;
        s.next_think = self.tick + self.defs.idle_rethink_ticks;
        s.wait = blocked.as_ref().map(|b| b.kind);
        let activity = match blocked {
            Some(b) => Activity::Blocked(b.reason),
            None => Activity::Idle(self.idle_reason(sid)),
        };
        let changed = match (&previous, &activity) {
            (Activity::Idle(_), Activity::Idle(_)) => false,
            (a, b) => a != b,
        };
        if changed {
            let s = &self.spirits[sid];
            // Short of materials isn't stuck: it clears up by itself.
            let word = match s.wait {
                Some(WaitKind::Recovery | WaitKind::Inputs) => "waiting".to_string(),
                _ => activity.label().to_lowercase(),
            };
            let msg = format!("{} is {word}: {}", s.name, activity.text());
            let key = match &activity {
                // Counts in the text change without the reason changing.
                Activity::Blocked(r) => {
                    let reason: String = r.chars().filter(|c| !c.is_ascii_digit()).collect();
                    format!("blocked:{reason}")
                }
                _ => "idle".into(),
            };
            self.log_spirit(sid, key, msg);
        }
        self.spirits[sid].activity = activity;
    }

    /// Whether this spirit's level allows a recipe for `output`.
    fn can_make(&self, s: &Spirit, output: ResourceId) -> bool {
        self.defs
            .recipes
            .iter()
            .any(|d| d.output.0 == output && d.skill == s.skill() && d.level_required <= s.level())
    }

    /// Whether this kind of tree grows where this spirit has been told to work.
    fn grows_at_post(&self, s: &Spirit, kind: TreeKindId) -> bool {
        s.policy.site.is_none_or(|p| {
            self.trees
                .iter()
                .any(|t| t.kind == kind && self.within_post(t.site, p))
        })
    }

    /// Whether this spirit can gather the basic material where it works. A
    /// spirit posted to a site without it isn't blocked, just not needed.
    fn basic_in_reach(&self, sid: SpiritId) -> bool {
        let s = &self.spirits[sid];
        let basic = self.settlement.objective.resource;
        self.defs.tree_tiers(s.skill()).into_iter().any(|k| {
            self.defs.tree(k).resource == basic
                && self.spirit_can_cut(s, k)
                && self.grows_at_post(s, k)
        })
    }

    /// A material other than the settlement's basic one is short, and this
    /// spirit can cut it where it works.
    fn advanced_wanted(&self, sid: SpiritId) -> bool {
        let s = &self.spirits[sid];
        let basic = self.settlement.objective.resource;
        self.defs.tree_tiers(s.skill()).into_iter().any(|k| {
            let r = self.defs.tree(k).resource;
            r != basic
                && self.spirit_can_cut(s, k)
                && self.grows_at_post(s, k)
                && self.gather_short(r)
        })
    }

    fn idle_reason(&self, sid: SpiritId) -> String {
        if self.spirits[sid].policy.focus == Focus::Rest {
            return "resting, as instructed".into();
        }
        let role = self.defs.role(self.spirits[sid].role);
        if role.has_job(JobKind::Gather) {
            let o = &self.settlement.objective;
            return format!(
                "objective satisfied ({}/{} {})",
                self.projected_stock(o.resource),
                o.target,
                self.defs.resource(o.resource).name
            );
        }
        if role.has_job(JobKind::Haul) {
            return "nothing to carry".into();
        }
        if role.has_job(JobKind::Craft) {
            let s = &self.spirits[sid];
            let order = &self.settlement.order;
            let name = &self.defs.resource(order.output).name;
            return if !s.assigned {
                format!("not assigned to {name}")
            } else if !order.enabled {
                "production is disabled".into()
            } else if self.stations().next().is_none() {
                "no workshop available".into()
            } else {
                format!(
                    "target met ({}/{} {name})",
                    self.projected_stock(order.output),
                    self.production_goal()
                )
            };
        }
        if role.has_job(JobKind::Construct) {
            return "no construction project assigned".into();
        }
        "nothing to do".into()
    }

    fn install(
        &mut self,
        sid: SpiritId,
        job: JobKind,
        plan: JobPlan,
        previous: &Activity,
        why: &str,
    ) {
        if let Some(t) = plan.claim_tree {
            self.trees[t].claimed_by = Some(sid);
        }
        if let Some((b, r, n)) = plan.reserve {
            let reserved = self.buildings[b].storage.try_reserve(r, n);
            debug_assert!(reserved, "plans only reserve what is available");
        }
        let s = &self.spirits[sid];
        if !matches!(previous, Activity::Working(_)) {
            let msg = format!("{}: {} - {why}", s.name, plan.description);
            self.log_spirit(sid, format!("job:{job:?}"), msg);
        }
        let s = &self.spirits[sid];
        if let Some(target) = plan.deliver_target
            && let Some(old) = s.last_deliver_target
            && old != target
        {
            let dist = self.path_distance_to(sid, target).unwrap_or(0);
            let msg = format!(
                "{} now delivers to {} ({dist} tiles away) instead of {}",
                s.name, self.buildings[target].name, self.buildings[old].name
            );
            self.events.push(self.tick, EventKind::Spirit, msg);
        }
        if let Some((site, kind)) = plan.gather {
            let s = &self.spirits[sid];
            if let Some(old) = s.home_site
                && old != site
            {
                let msg = format!(
                    "{} relocates from {} to {}: {}",
                    s.name,
                    self.defs.site(old).name,
                    self.defs.site(site).name,
                    self.site_unavailable_reason(sid, old)
                );
                self.log_spirit(sid, format!("relocate:{}", site.0), msg);
            }
            let s = &mut self.spirits[sid];
            s.home_site = Some(site);
            s.last_tree_kind = Some(kind);
        }
        let s = &mut self.spirits[sid];
        s.wait = None;
        if plan.deliver_target.is_some() {
            s.last_deliver_target = plan.deliver_target;
        }
        s.job = Some(job);
        s.activity = Activity::Working(plan.description);
        s.actor.queue.extend(plan.tasks);
    }

    /// Logs a spirit decision unless the same kind was logged recently.
    pub(crate) fn log_spirit(&mut self, sid: SpiritId, key: String, msg: String) {
        let repeat_after = 60 * self.defs.ticks_per_second as u64;
        let last = self.spirits[sid].log_times.get(&key).copied();
        if last.is_some_and(|t| self.tick < t + repeat_after) {
            return;
        }
        // Waiting for regrowth is shown on the map and repeats often.
        let waiting = self.spirits[sid].wait == Some(WaitKind::Recovery);
        let detail = key == "idle" || key.starts_with("job:") || waiting;
        self.spirits[sid].log_times.insert(key, self.tick);
        if detail {
            self.events.push_detail(self.tick, EventKind::Spirit, msg);
        } else {
            self.events.push(self.tick, EventKind::Spirit, msg);
        }
    }

    fn path_distance_to(&self, sid: SpiritId, building: BuildingId) -> Option<u32> {
        let field = self.grid.distance_field(&[self.spirits[sid].actor.pos()]);
        self.grid
            .approach_distance(&field, self.buildings[building].pos)
    }

    /// Why a spirit left (or can't use) a site, for the event log.
    fn site_unavailable_reason(&self, sid: SpiritId, site: SiteId) -> String {
        if !self.settlement.site_open[site.index()] {
            return format!("{} is closed to spirits", self.defs.site(site).name);
        }
        let s = &self.spirits[sid];
        let free = self
            .trees
            .iter()
            .filter(|t| t.site == Some(site) && t.is_grown() && self.spirit_can_cut(s, t.kind))
            .filter(|t| t.claimed_by.is_none_or(|c| c == sid))
            .count();
        if free == 0 {
            "no free grown trees left there".into()
        } else {
            "a closer tree was available".into()
        }
    }

    /// Spirits obey the same level requirements as the player.
    pub(crate) fn spirit_can_cut(&self, s: &Spirit, kind: TreeKindId) -> bool {
        let def = self.defs.tree(kind);
        def.skill == s.skill() && s.level() >= def.level_required
    }

    fn plan_job(&self, sid: SpiritId, job: JobKind) -> Result<JobPlan, Blocked> {
        match job {
            JobKind::Gather => self.plan_gather(sid),
            JobKind::Deliver => self.plan_deliver(sid),
            JobKind::Haul => self.plan_haul(sid),
            JobKind::Construct => self.plan_construct(sid),
            JobKind::Craft => self.plan_craft(sid),
            JobKind::Supply => self.plan_supply(sid),
            JobKind::Collect => self.plan_collect(sid),
            JobKind::TrainingCollect => self.plan_training_collect(sid),
            JobKind::TrainingSupply => self.plan_training_supply(sid),
        }
    }

    pub(crate) fn has_role_with(&self, job: JobKind) -> bool {
        self.spirits
            .iter()
            .any(|s| self.defs.role(s.role).has_job(job))
    }

    /// Storages a spirit with `policy` may deliver to (complete, with room).
    fn deposit_targets(&self, policy: DeliverPolicy) -> Vec<BuildingId> {
        self.buildings
            .iter()
            .filter(|b| b.is_complete() && b.storage.free() > 0)
            .filter(|b| match policy {
                DeliverPolicy::Main => b.id == self.main_storage,
                DeliverPolicy::Nearest => self.defs.building(b.kind).accepts_gatherer_deposits,
            })
            .map(|b| b.id)
            .collect()
    }

    fn plan_gather(&self, sid: SpiritId) -> Result<JobPlan, Blocked> {
        let s = &self.spirits[sid];
        let role = self.defs.role(s.role);
        let from = self.grid.distance_field(&[s.actor.pos()]);
        let storage_fields: Vec<_> = self
            .deposit_targets(role.deliver_to)
            .into_iter()
            .map(|b| self.grid.approach_field(self.buildings[b].pos))
            .collect();
        if storage_fields.is_empty() {
            return Err(Blocked::new(
                WaitKind::Storage,
                "No storage has room for more logs",
            ));
        }

        // Which trees are this spirit's business: wanted materials it can
        // cut, at its assigned site, the top tier first if it's been told to.
        let short: Vec<bool> = (0..self.defs.resources.len())
            .map(|i| self.gather_short(ResourceId(i as u16)))
            .collect();
        let wanted =
            |k: TreeKindId| self.spirit_can_cut(s, k) && short[self.defs.tree(k).resource.index()];
        let first_choice = s
            .policy
            .prefer_advanced
            .then(|| {
                let mut tiers = self.defs.tree_tiers(s.skill()).into_iter();
                tiers
                    .rfind(|k| wanted(*k))
                    .map(|k| self.defs.tree(k).resource)
            })
            .flatten();
        let suits = |t: &Tree| {
            wanted(t.kind)
                && first_choice.is_none_or(|r| self.defs.tree(t.kind).resource == r)
                && s.policy.site.is_none_or(|p| self.within_post(t.site, p))
        };

        let mut any_cuttable = false;
        let mut any_permitted = false;
        let mut any_grown = false;
        // A spirit stays at its current site while that site has a free tree
        // it can cut. It only moves to another site that has at least a full
        // load of free logs; otherwise it waits for local regrowth. This
        // avoids crossing the map for a single regrown tree.
        let mut free_logs = vec![0u32; self.defs.sites.len()];
        for t in &self.trees {
            if let Some(site) = t.site
                && suits(t)
                && t.claimed_by.is_none_or(|c| c == sid)
            {
                free_logs[site.index()] += t.logs_left();
            }
        }
        let load = s.actor.inventory.capacity;
        let mut best_home: Option<(u32, TreeId)> = None;
        let mut best: Option<(u32, TreeId)> = None;
        for t in &self.trees {
            if !suits(t) {
                continue;
            }
            any_cuttable = true;
            if t.site
                .is_some_and(|site| !self.settlement.site_open[site.index()])
            {
                continue;
            }
            any_permitted = true;
            if !t.is_grown() {
                continue;
            }
            any_grown = true;
            if t.claimed_by.is_some_and(|c| c != sid) {
                continue;
            }
            let Some(walk) = self.grid.approach_distance(&from, t.pos) else {
                continue;
            };
            let Some(haul) = storage_fields
                .iter()
                .filter_map(|f| self.grid.approach_distance(f, t.pos))
                .min()
            else {
                continue;
            };
            // Prefer trees that are close to us and close to a drop-off point.
            let score = walk + haul;
            if s.home_site.is_some()
                && t.site == s.home_site
                && best_home.is_none_or(|(b, _)| score < b)
            {
                best_home = Some((score, t.id));
            }
            let worth_moving = s.home_site.is_none()
                || t.site == s.home_site
                || t.site.is_none_or(|site| free_logs[site.index()] >= load);
            if worth_moving && best.is_none_or(|(b, _)| score < b) {
                best = Some((score, t.id));
            }
        }
        let Some((_, tree)) = best_home.or(best) else {
            return Err(if !any_cuttable {
                "No trees within this spirit's skill level".into()
            } else if !any_permitted {
                "Every site with suitable trees is closed to spirits".into()
            } else if any_grown {
                Blocked::new(
                    WaitKind::Recovery,
                    "All grown trees are taken - waiting for regrowth",
                )
            } else {
                Blocked::new(WaitKind::Recovery, "No grown trees - waiting for regrowth")
            });
        };
        let t = &self.trees[tree];
        let name = self.defs.tree(t.kind).name.clone();
        let place = t
            .site
            .map(|site| format!(" in {}", self.defs.site(site).name))
            .unwrap_or_default();
        Ok(JobPlan {
            description: format!("Gathering at {name} {}{place}", t.pos),
            tasks: vec![
                Task::Move {
                    goal: Goal::Adjacent(t.pos),
                    label: name,
                },
                Task::Chop { tree },
            ],
            claim_tree: Some(tree),
            gather: t.site.map(|site| (site, t.kind)),
            ..Default::default()
        })
    }

    fn plan_deliver(&self, sid: SpiritId) -> Result<JobPlan, Blocked> {
        let s = &self.spirits[sid];
        if s.actor.inventory.total() == 0 {
            return Err("Nothing to deliver".into());
        }
        let role = self.defs.role(s.role);
        let field = self.grid.distance_field(&[s.actor.pos()]);
        let target = self
            .deposit_targets(role.deliver_to)
            .into_iter()
            .filter_map(|b| {
                self.grid
                    .approach_distance(&field, self.buildings[b].pos)
                    .map(|d| (d, b))
            })
            .min();
        let Some((_, b)) = target else {
            return Err(Blocked::new(
                WaitKind::Storage,
                "No reachable storage has free space",
            ));
        };
        let building = &self.buildings[b];
        Ok(JobPlan {
            description: format!(
                "Delivering {} to {}",
                self.defs.describe_amounts(s.actor.inventory.items()),
                building.name
            ),
            tasks: vec![
                Task::Move {
                    goal: Goal::Adjacent(building.pos),
                    label: building.name.clone(),
                },
                Task::Deposit { building: b },
            ],
            claim_tree: None,
            deliver_target: Some(b),
            ..Default::default()
        })
    }

    /// The store with the most to send home that's worth this spirit's trip.
    fn haul_source(&self, sid: SpiritId) -> Option<(BuildingId, ResourceId, u32)> {
        let s = &self.spirits[sid];
        let min = self.defs.role(s.role).haul_min;
        self.outposts()
            .filter(|b| self.serves(s, b))
            .filter_map(|b| {
                let (r, n) = self.surplus_at(b)?;
                (self.total_surplus(b) >= min).then_some((b, r, n))
            })
            .max_by_key(|(b, _, n)| (*n, std::cmp::Reverse(b.id)))
            .map(|(b, r, n)| (b.id, r, n))
    }

    fn plan_haul(&self, sid: SpiritId) -> Result<JobPlan, Blocked> {
        let s = &self.spirits[sid];
        let (source, resource, amount) = self
            .haul_source(sid)
            .ok_or_else(|| "Nothing to carry home".to_string())?;
        let source = &self.buildings[source];
        let main = self.main();
        if main.storage.free() == 0 {
            return Err(Blocked::new(WaitKind::Storage, "Main Storage is full"));
        }
        let amount = amount
            .min(s.actor.inventory.free())
            .min(main.storage.free());
        Ok(JobPlan {
            description: format!(
                "Hauling {amount} {} from {} to {}",
                self.defs.resource(resource).name,
                source.name,
                main.name
            ),
            tasks: vec![
                Task::Move {
                    goal: Goal::Adjacent(source.pos),
                    label: source.name.clone(),
                },
                Task::Withdraw {
                    building: source.id,
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

    /// Whether someone will do `job` for the settlement.
    pub(crate) fn has_worker_for(&self, job: JobKind) -> bool {
        self.spirits
            .iter()
            .any(|s| self.defs.role(s.role).has_job(job) && s.policy.focus.allows(job))
    }

    /// Work at a station: the most urgent order this spirit can serve, at the
    /// nearest permitted station that has a free place and materials. A free
    /// worker only crosses the map for a bench with a real stretch of work.
    fn plan_craft(&self, sid: SpiritId) -> Result<JobPlan, Blocked> {
        let s = &self.spirits[sid];
        let mut blocked: Option<Blocked> = None;
        let field = self.grid.distance_field(&[s.actor.pos()]);
        let long = self.defs.logistics.long_trip_tiles;
        for output in self.orders_for(s) {
            let mut stations: Vec<(u32, &Building)> = self
                .stations_for(output)
                .into_iter()
                .filter(|b| self.may_work_at(s, b.id))
                .filter_map(|b| Some((self.grid.approach_distance(&field, b.pos)?, b)))
                .collect();
            stations.sort_by_key(|(walk, b)| (*walk, b.id));
            for (walk, b) in stations {
                if walk >= long && s.policy.station.is_none() && !self.worth_the_walk(s, b, output)
                {
                    blocked.get_or_insert(Blocked::new(
                        WaitKind::Inputs,
                        format!("{} is too far for what's there", b.name),
                    ));
                    continue;
                }
                match self.plan_craft_at(sid, b, output) {
                    Ok(plan) => return Ok(plan),
                    // A full bench says most about why this spirit is idle.
                    Err(e) if e.kind == WaitKind::Station => blocked = Some(e),
                    Err(e) => {
                        blocked.get_or_insert(e);
                    }
                }
            }
        }
        Err(blocked.unwrap_or_else(|| "No workshop available".into()))
    }

    /// Whether a far bench holds enough material to be worth walking to.
    fn worth_the_walk(&self, s: &Spirit, b: &Building, output: ResourceId) -> bool {
        let batches = self.defs.logistics.long_trip_batches;
        let have = |r| b.storage.available(r);
        self.defs.recipes.iter().any(|d| {
            d.output.0 == output
                && d.station == b.kind
                && d.level_required <= s.level()
                && d.inputs.iter().all(|&(r, n)| have(r) >= n * batches)
        })
    }

    /// What this spirit could make, in the order it should try: urgent orders
    /// first and, between equals, the hardest work it's qualified for, because
    /// fewer spirits can do that.
    fn orders_for(&self, s: &Spirit) -> Vec<ResourceId> {
        let mut list: Vec<_> = self
            .orders()
            .into_iter()
            .filter(|o| self.deficit_of(o.output) > 0 && self.can_make(s, o.output))
            .map(|o| {
                let tier = self
                    .defs
                    .recipes
                    .iter()
                    .filter(|d| d.output.0 == o.output && d.level_required <= s.level())
                    .map(|d| d.level_required)
                    .max()
                    .unwrap_or(0);
                (
                    std::cmp::Reverse(o.priority),
                    std::cmp::Reverse(tier),
                    o.output,
                )
            })
            .collect();
        list.sort();
        list.into_iter().map(|x| x.2).collect()
    }

    /// Between batches, a worker leaves its bench job if something more
    /// urgent has become possible right where it stands.
    pub(crate) fn switch_craft_if_needed(&mut self, sid: SpiritId) {
        let s = &self.spirits[sid];
        let Some(&Task::Craft {
            building,
            output: Some(current),
        }) = s.actor.queue.front()
        else {
            return;
        };
        if s.actor.action_ticks > 0 {
            return;
        }
        let b = &self.buildings[building];
        let have = |r| s.actor.inventory.get(r) + b.storage.available(r);
        let better = self
            .orders_for(s)
            .into_iter()
            .take_while(|o| *o != current)
            .any(|o| {
                self.defs
                    .best_recipe_for(b.kind, s.level(), o, have)
                    .is_some()
            });
        if better {
            let s = &mut self.spirits[sid];
            s.actor.clear_tasks();
            s.next_think = self.tick;
        }
    }

    /// Work at one station. With no supplier in the settlement, the worker
    /// first fetches its own materials from Main Storage.
    fn plan_craft_at(
        &self,
        sid: SpiritId,
        station: &Building,
        output: ResourceId,
    ) -> Result<JobPlan, Blocked> {
        let s = &self.spirits[sid];
        let def = self.defs.building(station.kind);
        let limit = def.capacity.saturating_sub(def.input_buffer);
        if station.storage.get(output) >= limit || station.storage.free() == 0 {
            return Err(Blocked::new(
                WaitKind::Storage,
                format!("{} output is full", station.name),
            ));
        }
        if !self.slot_free_for_spirit(station) {
            return Err(Blocked::new(
                WaitKind::Station,
                format!(
                    "{} waiting: {} is occupied",
                    self.defs.role(s.role).name,
                    station.name
                ),
            ));
        }
        let to_station = Task::Move {
            goal: Goal::Adjacent(station.pos),
            label: station.name.clone(),
        };
        let craft = Task::Craft {
            building: station.id,
            output: Some(output),
        };
        let have = |r| s.actor.inventory.get(r) + station.storage.available(r);
        let best = self
            .defs
            .best_recipe_for(station.kind, s.level(), output, have);
        if let Some(r) = best {
            return Ok(JobPlan {
                description: format!("Making {} at {}", self.defs.recipe(r).name, station.name),
                tasks: vec![to_station, craft],
                ..Default::default()
            });
        }
        let lowest = self
            .defs
            .station_recipes(station.kind)
            .into_iter()
            .map(|r| self.defs.recipe(r))
            .find(|r| r.output.0 == output);
        let input = lowest.map_or(self.defs.objective.resource, |r| r.inputs[0].0);
        let short = || {
            Blocked::new(
                WaitKind::Inputs,
                format!(
                    "{} waiting: insufficient {}",
                    station.name,
                    self.defs.resource(input).name.to_lowercase()
                ),
            )
        };
        if self.has_worker_for(JobKind::Supply) {
            return Err(short());
        }
        let (_, resource, want) = self.input_wanted(sid).ok_or_else(short)?;
        let (source, stock, _) = self
            .supply_source(station.id, resource, want)
            .ok_or_else(short)?;
        let amount = want.min(stock).min(s.actor.inventory.free());
        let recipe = self.recipe_for(output).ok_or_else(short)?;
        let needed = lowest
            .and_then(|r| r.inputs.iter().find(|i| i.0 == resource).map(|i| i.1))
            .unwrap_or(1);
        if amount + have(resource) < needed {
            return Err(short());
        }
        let source = &self.buildings[source];
        Ok(JobPlan {
            description: format!(
                "Fetching {amount} {} for {}",
                self.defs.resource(resource).name,
                self.defs.recipe(recipe).name
            ),
            tasks: vec![
                Task::Move {
                    goal: Goal::Adjacent(source.pos),
                    label: source.name.clone(),
                },
                Task::Withdraw {
                    building: source.id,
                    resource,
                    amount,
                    reserved: true,
                },
                to_station,
                craft,
            ],
            reserve: Some((source.id, resource, amount)),
            ..Default::default()
        })
    }

    /// Carry raw materials to a station from the nearest storage that has
    /// them, leaving Main Storage's reserve. A long trip waits for a real load.
    fn plan_supply(&self, sid: SpiritId) -> Result<JobPlan, Blocked> {
        let s = &self.spirits[sid];
        let (station, resource, want) = self
            .input_wanted(sid)
            .ok_or_else(|| "The workshop has enough materials".to_string())?;
        let station = &self.buildings[station];
        let name = &self.defs.resource(resource).name;
        let short = || {
            Blocked::new(
                WaitKind::Inputs,
                format!("No spare {name} in storage for the workshop"),
            )
        };
        let (source, stock, walk) = self
            .supply_source(station.id, resource, want)
            .ok_or_else(short)?;
        let amount = want.min(stock).min(s.actor.inventory.free());
        if amount == 0 {
            return Err(short());
        }
        if walk >= self.defs.logistics.long_trip_tiles && amount < self.long_trip_load(s, want) {
            return Err(Blocked::new(
                WaitKind::Inputs,
                format!("Waiting for a load of {name} worth the walk"),
            ));
        }
        let source = &self.buildings[source];
        Ok(JobPlan {
            description: format!(
                "Supplying {amount} {name} from {} to {}",
                source.name, station.name
            ),
            tasks: vec![
                Task::Move {
                    goal: Goal::Adjacent(source.pos),
                    label: source.name.clone(),
                },
                Task::Withdraw {
                    building: source.id,
                    resource,
                    amount,
                    reserved: true,
                },
                Task::Move {
                    goal: Goal::Adjacent(station.pos),
                    label: station.name.clone(),
                },
                Task::Deposit {
                    building: station.id,
                },
            ],
            reserve: Some((source.id, resource, amount)),
            ..Default::default()
        })
    }

    /// Carry finished goods from a station to Main Storage.
    fn plan_collect(&self, sid: SpiritId) -> Result<JobPlan, Blocked> {
        let s = &self.spirits[sid];
        let main = self.main();
        if main.storage.free() == 0 {
            return Err(Blocked::new(WaitKind::Storage, "Main Storage is full"));
        }
        let (station, resource, waiting) = self
            .output_pile(sid)
            .ok_or_else(|| "Nothing finished to collect".to_string())?;
        let station = &self.buildings[station];
        let amount = waiting
            .min(s.actor.inventory.free())
            .min(main.storage.free());
        if amount == 0 {
            return Err("Nothing finished to collect".into());
        }
        Ok(JobPlan {
            description: format!(
                "Carrying {amount} {} from {} to {}",
                self.defs.resource(resource).name,
                station.name,
                main.name
            ),
            tasks: vec![
                Task::Move {
                    goal: Goal::Adjacent(station.pos),
                    label: station.name.clone(),
                },
                Task::Withdraw {
                    building: station.id,
                    resource,
                    amount,
                    reserved: true,
                },
                Task::Move {
                    goal: Goal::Adjacent(main.pos),
                    label: main.name.clone(),
                },
                Task::Deposit { building: main.id },
            ],
            reserve: Some((station.id, resource, amount)),
            ..Default::default()
        })
    }

    fn plan_construct(&self, sid: SpiritId) -> Result<JobPlan, Blocked> {
        let pid = self
            .project_for(sid)
            .ok_or_else(|| "No construction project assigned".to_string())?;
        let p = &self.projects[pid];
        let def = self.defs.building(p.kind);
        let site = self.buildings[p.building].pos;
        let site_label = format!("{} site", def.name);
        let s = &self.spirits[sid];
        let carrying_materials = def.cost.iter().any(|&(r, _)| s.actor.inventory.get(r) > 0);
        let missing: Vec<_> = def
            .cost
            .iter()
            .map(|&(r, n)| {
                (
                    r,
                    n.saturating_sub(p.delivered.get(&r).copied().unwrap_or(0)),
                )
            })
            .filter(|&(_, n)| n > 0)
            .collect();

        let to_site = Task::Move {
            goal: Goal::Adjacent(site),
            label: site_label.clone(),
        };
        if carrying_materials {
            return Ok(JobPlan {
                description: format!("Carrying materials to the {site_label}"),
                tasks: vec![to_site, Task::DeliverToSite { project: pid }],
                ..Default::default()
            });
        }
        if missing.is_empty() {
            return Ok(JobPlan {
                description: format!("Building {} at {site}", def.name),
                tasks: vec![to_site, Task::Build { project: pid }],
                ..Default::default()
            });
        }
        if !p.reserved || p.state == ProjectState::AwaitingMaterials {
            return Err(p
                .blocked
                .clone()
                .unwrap_or_else(|| "Waiting for materials".into())
                .into());
        }
        let (resource, need) = missing[0];
        let amount = need.min(s.actor.inventory.free());
        let main = self.main();
        Ok(JobPlan {
            description: format!(
                "Fetching {amount} {} from {} for the {}",
                self.defs.resource(resource).name,
                main.name,
                def.name
            ),
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
                to_site,
                Task::DeliverToSite { project: pid },
            ],
            ..Default::default()
        })
    }
}
