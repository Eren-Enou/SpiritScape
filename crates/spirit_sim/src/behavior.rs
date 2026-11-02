//! Spirit decision making: evaluate prioritized job rules against the world
//! and expand the chosen job into a task queue.

use crate::defs::{Condition, DeliverPolicy, JobKind};
use crate::entities::{Activity, BuildingId, ProjectState, SpiritId, TreeId};
use crate::events::EventKind;
use crate::tasks::{Goal, Task};
use crate::world::World;

/// A planned job, ready to be installed on a spirit.
struct JobPlan {
    description: String,
    tasks: Vec<Task>,
    claim_tree: Option<TreeId>,
    deliver_target: Option<BuildingId>,
}

impl World {
    pub(crate) fn condition_holds(&self, sid: SpiritId, cond: Condition) -> bool {
        let s = &self.spirits[sid];
        match cond {
            Condition::Always => true,
            Condition::InventoryFull => s.actor.inventory.is_full(),
            Condition::CarryingAny => s.actor.inventory.total() > 0,
            Condition::ObjectiveUnmet => self.objective_unmet(),
            Condition::OutpostStockAvailable => {
                let min = self.defs.role(s.role).haul_min;
                self.outposts().any(|b| b.storage.total() >= min)
            }
            Condition::ProjectAssigned => self.project_for(sid).is_some(),
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
                Some(p) => format!("assigned to the {} project", self.defs.building(self.projects[p].kind).name),
                None => "no project".into(),
            },
        }
    }

    fn outposts(&self) -> impl Iterator<Item = &crate::entities::Building> {
        self.buildings.iter().filter(|b| b.id != self.main_storage && b.is_complete())
    }

    pub(crate) fn project_for(&self, sid: SpiritId) -> Option<usize> {
        self.projects.iter().position(|p| p.is_active() && p.assigned == Some(sid))
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
        let mut blocked: Option<String> = None;

        for rule in &role.jobs {
            if !self.condition_holds(sid, rule.when) {
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
        let activity = match blocked {
            Some(reason) => Activity::Blocked(reason),
            None => Activity::Idle(self.idle_reason(sid)),
        };
        let changed = match (&previous, &activity) {
            (Activity::Idle(_), Activity::Idle(_)) => false,
            (a, b) => a != b,
        };
        if changed {
            let s = &self.spirits[sid];
            let msg = format!("{} is {}: {}", s.name, activity.label().to_lowercase(), activity.text());
            let key = match &activity {
                Activity::Blocked(r) => format!("blocked:{r}"),
                _ => "idle".into(),
            };
            self.log_spirit(sid, key, msg);
        }
        self.spirits[sid].activity = activity;
    }

    fn idle_reason(&self, sid: SpiritId) -> String {
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
            return "no outpost stock to haul".into();
        }
        if role.has_job(JobKind::Construct) {
            return "no construction project assigned".into();
        }
        "nothing to do".into()
    }

    fn install(&mut self, sid: SpiritId, job: JobKind, plan: JobPlan, previous: &Activity, why: &str) {
        if let Some(t) = plan.claim_tree {
            self.trees[t].claimed_by = Some(sid);
        }
        let s = &self.spirits[sid];
        if !matches!(previous, Activity::Working(_)) {
            let msg = format!("{}: {} - {why}", s.name, plan.description);
            self.log_spirit(sid, format!("job:{job:?}"), msg);
        }
        let mut notes = Vec::new();
        let s = &self.spirits[sid];
        if let Some(target) = plan.deliver_target {
            if let Some(old) = s.last_deliver_target
                && old != target
            {
                let dist = self.path_distance_to(sid, target).unwrap_or(0);
                notes.push(format!(
                    "{} now delivers to {} ({dist} tiles away) instead of {}",
                    s.name, self.buildings[target].name, self.buildings[old].name
                ));
            }
        }
        for n in notes {
            self.events.push(self.tick, EventKind::Spirit, n);
        }
        let s = &mut self.spirits[sid];
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
        self.spirits[sid].log_times.insert(key, self.tick);
        self.events.push(self.tick, EventKind::Spirit, msg);
    }

    fn path_distance_to(&self, sid: SpiritId, building: BuildingId) -> Option<u32> {
        let field = self.grid.distance_field(&[self.spirits[sid].actor.pos()]);
        self.grid.approach_distance(&field, self.buildings[building].pos)
    }

    fn plan_job(&self, sid: SpiritId, job: JobKind) -> Result<JobPlan, String> {
        match job {
            JobKind::Gather => self.plan_gather(sid),
            JobKind::Deliver => self.plan_deliver(sid),
            JobKind::Haul => self.plan_haul(sid),
            JobKind::Construct => self.plan_construct(sid),
        }
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

    fn plan_gather(&self, sid: SpiritId) -> Result<JobPlan, String> {
        let s = &self.spirits[sid];
        let role = self.defs.role(s.role);
        let from = self.grid.distance_field(&[s.actor.pos()]);
        let storage_fields: Vec<_> = self
            .deposit_targets(role.deliver_to)
            .into_iter()
            .map(|b| self.grid.approach_field(self.buildings[b].pos))
            .collect();
        if storage_fields.is_empty() {
            return Err("No storage has room for more logs".into());
        }

        let mut any_grown = false;
        let mut best: Option<(u32, TreeId)> = None;
        for t in &self.trees {
            if self.defs.tree(t.kind).skill != s.skill() || !t.is_grown() {
                continue;
            }
            any_grown = true;
            if t.claimed_by.is_some_and(|c| c != sid) {
                continue;
            }
            let Some(walk) = self.grid.approach_distance(&from, t.pos) else { continue };
            let Some(haul) = storage_fields.iter().filter_map(|f| self.grid.approach_distance(f, t.pos)).min() else {
                continue;
            };
            // Prefer trees that are close to us and close to a drop-off point.
            let score = walk + haul;
            if best.is_none_or(|(b, _)| score < b) {
                best = Some((score, t.id));
            }
        }
        let Some((_, tree)) = best else {
            return Err(if any_grown {
                "All reachable trees are taken by other spirits".into()
            } else {
                "No grown trees - waiting for regrowth".into()
            });
        };
        let t = &self.trees[tree];
        let name = self.defs.tree(t.kind).name.clone();
        Ok(JobPlan {
            description: format!("Gathering at {name} {}", t.pos),
            tasks: vec![Task::Move { goal: Goal::Adjacent(t.pos), label: name }, Task::Chop { tree }],
            claim_tree: Some(tree),
            deliver_target: None,
        })
    }

    fn plan_deliver(&self, sid: SpiritId) -> Result<JobPlan, String> {
        let s = &self.spirits[sid];
        if s.actor.inventory.total() == 0 {
            return Err("Nothing to deliver".into());
        }
        let role = self.defs.role(s.role);
        let field = self.grid.distance_field(&[s.actor.pos()]);
        let target = self
            .deposit_targets(role.deliver_to)
            .into_iter()
            .filter_map(|b| self.grid.approach_distance(&field, self.buildings[b].pos).map(|d| (d, b)))
            .min();
        let Some((_, b)) = target else {
            return Err("No reachable storage has free space".into());
        };
        let building = &self.buildings[b];
        Ok(JobPlan {
            description: format!(
                "Delivering {} to {}",
                self.defs.describe_amounts(s.actor.inventory.items()),
                building.name
            ),
            tasks: vec![
                Task::Move { goal: Goal::Adjacent(building.pos), label: building.name.clone() },
                Task::Deposit { building: b },
            ],
            claim_tree: None,
            deliver_target: Some(b),
        })
    }

    fn plan_haul(&self, sid: SpiritId) -> Result<JobPlan, String> {
        let s = &self.spirits[sid];
        let min = self.defs.role(s.role).haul_min;
        let source = self
            .outposts()
            .filter(|b| b.storage.total() >= min)
            .max_by_key(|b| (b.storage.total(), std::cmp::Reverse(b.id)))
            .ok_or_else(|| "No outpost stock to haul".to_string())?;
        let main = self.main();
        if main.storage.free() == 0 {
            return Err("Main Storage is full".into());
        }
        let (&resource, &amount) = source
            .storage
            .contents()
            .iter()
            .max_by_key(|(r, n)| (**n, std::cmp::Reverse(**r)))
            .unwrap();
        let amount = amount.min(s.actor.inventory.free()).min(main.storage.free());
        Ok(JobPlan {
            description: format!(
                "Hauling {amount} {} from {} to {}",
                self.defs.resource(resource).name,
                source.name,
                main.name
            ),
            tasks: vec![
                Task::Move { goal: Goal::Adjacent(source.pos), label: source.name.clone() },
                Task::Withdraw { building: source.id, resource, amount, reserved: false },
                Task::Move { goal: Goal::Adjacent(main.pos), label: main.name.clone() },
                Task::Deposit { building: main.id },
            ],
            claim_tree: None,
            deliver_target: None,
        })
    }

    fn plan_construct(&self, sid: SpiritId) -> Result<JobPlan, String> {
        let pid = self.project_for(sid).ok_or_else(|| "No construction project assigned".to_string())?;
        let p = &self.projects[pid];
        let def = self.defs.building(p.kind);
        let site = self.buildings[p.building].pos;
        let site_label = format!("{} site", def.name);
        let s = &self.spirits[sid];
        let carrying_materials = def.cost.iter().any(|&(r, _)| s.actor.inventory.get(r) > 0);
        let missing: Vec<_> = def
            .cost
            .iter()
            .map(|&(r, n)| (r, n.saturating_sub(p.delivered.get(&r).copied().unwrap_or(0))))
            .filter(|&(_, n)| n > 0)
            .collect();

        let to_site = Task::Move { goal: Goal::Adjacent(site), label: site_label.clone() };
        if carrying_materials {
            return Ok(JobPlan {
                description: format!("Carrying materials to the {site_label}"),
                tasks: vec![to_site, Task::DeliverToSite { project: pid }],
                claim_tree: None,
                deliver_target: None,
            });
        }
        if missing.is_empty() {
            return Ok(JobPlan {
                description: format!("Building {} at {site}", def.name),
                tasks: vec![to_site, Task::Build { project: pid }],
                claim_tree: None,
                deliver_target: None,
            });
        }
        if !p.reserved || p.state == ProjectState::AwaitingMaterials {
            return Err(p.blocked.clone().unwrap_or_else(|| "Waiting for materials".into()));
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
                Task::Move { goal: Goal::Adjacent(main.pos), label: main.name.clone() },
                Task::Withdraw { building: main.id, resource, amount, reserved: true },
                to_site,
                Task::DeliverToSite { project: pid },
            ],
            claim_tree: None,
            deliver_target: None,
        })
    }
}
