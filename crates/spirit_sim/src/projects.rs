//! The construction queue: a short, ordered list of buildings the player wants
//! next. Only the front of the list ever becomes a project, so projects still
//! run one at a time and materials are only claimed when one starts.
//!
//! Until then a queued building holds nothing back except demand: its cost is
//! added to what production and gatherers aim for, which is what lets a player
//! set a plan and go back to skilling.

use crate::defs::{BuildingKindId, ResourceId};
use crate::entities::{BuildingState, ProjectId, ProjectState};
use crate::events::EventKind;
use crate::grid::TilePos;
use crate::outpost::DemandLine;
use crate::tasks::Task;
use crate::world::World;
use serde::{Deserialize, Serialize};

/// A building waiting its turn.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueuedProject {
    pub id: u32,
    pub kind: BuildingKindId,
    /// Where it goes. With none, a suitable place is found when it starts.
    pub site: Option<TilePos>,
    /// Why it can't start, when it's next and can't.
    #[serde(default)]
    pub blocked: Option<String>,
}

/// Where a project stands, for showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanStage {
    /// Waiting behind another project.
    Queued,
    /// Next, or under way, and short of something.
    GatheringMaterials,
    /// Materials claimed; the builder is carrying them.
    Supplying,
    Building,
    /// Can't go on until something else changes.
    Blocked,
    Finished,
}

impl PlanStage {
    pub fn label(self) -> &'static str {
        match self {
            PlanStage::Queued => "Queued",
            PlanStage::GatheringMaterials => "Gathering materials",
            PlanStage::Supplying => "Carrying materials",
            PlanStage::Building => "Building",
            PlanStage::Blocked => "Blocked",
            PlanStage::Finished => "Finished",
        }
    }
}

/// One line of the settlement's plans.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanView {
    pub name: String,
    pub stage: PlanStage,
    /// What changes when it's finished.
    pub benefit: String,
    pub cost: String,
    /// What's still short, and who's on it.
    pub needs: Vec<DemandLine>,
    /// Spirits working on this right now.
    pub workers: Vec<String>,
    /// The reason, when it isn't moving.
    pub reason: Option<String>,
    /// The queue id, for reordering and cancelling.
    pub queued: Option<u32>,
    /// The project, when one has started.
    pub project: Option<ProjectId>,
    pub can_cancel: bool,
}

impl World {
    /// Buildings of a kind standing, under way or on the list.
    fn planned_count(&self, kind: BuildingKindId) -> u32 {
        let standing = self
            .buildings
            .iter()
            .filter(|b| b.kind == kind && b.state != BuildingState::Removed)
            .count();
        let queued = self.settlement.queue.iter().filter(|q| q.kind == kind);
        (standing + queued.count()) as u32
    }

    /// Whether a prerequisite is finished, or will be before this position.
    fn prerequisite_met(&self, need: &str, position: usize) -> bool {
        let Some(kind) = self.defs.building_kind_id(need) else {
            return true;
        };
        let standing = self
            .buildings
            .iter()
            .any(|b| b.kind == kind && b.state != BuildingState::Removed);
        standing
            || self.settlement.queue[..position.min(self.settlement.queue.len())]
                .iter()
                .any(|q| q.kind == kind)
    }

    /// What would stop a building joining the queue.
    fn check_queue(&self, kind: BuildingKindId, site: Option<TilePos>) -> Result<(), String> {
        let def = self.defs.building(kind);
        let name = &def.name;
        if !def.manual {
            return Err(format!("A {name} can't be ordered"));
        }
        let limit = self.defs.construction.queue_limit as usize;
        if self.settlement.queue.len() >= limit {
            return Err(format!("The plan is full: {limit} projects at most"));
        }
        if def.max_count.is_some_and(|m| self.planned_count(kind) >= m) {
            return Err(format!("The settlement can't have more {name}s"));
        }
        let end = self.settlement.queue.len();
        if let Some(need) = def.requires.iter().find(|r| !self.prerequisite_met(r, end)) {
            let need = self.defs.building_kind_id(need).map(|k| self.defs.building(k));
            return Err(format!(
                "A {name} needs a {} first",
                need.map_or("?", |d| d.name.as_str())
            ));
        }
        if let Some(p) = site {
            let taken = self.settlement.queue.iter().any(|q| q.site == Some(p));
            if taken || !self.grid.is_buildable(p) {
                return Err(format!("You can't build at {p}"));
            }
        }
        Ok(())
    }

    /// Adds a building to the end of the plan.
    pub(crate) fn queue_building(
        &mut self,
        kind: BuildingKindId,
        site: Option<TilePos>,
    ) -> Result<String, String> {
        self.check_queue(kind, site)?;
        let id = self.settlement.next_queue_id;
        self.settlement.next_queue_id += 1;
        self.settlement.queue.push(QueuedProject {
            id,
            kind,
            site,
            blocked: None,
        });
        let name = self.defs.building(kind).name.clone();
        let msg = format!("{name} added to the plan");
        self.events.push(self.tick, EventKind::Player, msg.clone());
        self.update_queue();
        self.update_projects();
        Ok(msg)
    }

    /// Moves a queued building one place. Refused if that would put it ahead
    /// of something it depends on, or behind something depending on it.
    pub(crate) fn move_queued(&mut self, id: u32, up: bool) -> Result<String, String> {
        let queue = &self.settlement.queue;
        let at = queue
            .iter()
            .position(|q| q.id == id)
            .ok_or("That isn't on the plan")?;
        let other = if up {
            at.checked_sub(1)
        } else {
            (at + 1 < queue.len()).then_some(at + 1)
        };
        let Some(other) = other else {
            return Ok("It's already there".into());
        };
        let (first, second) = (at.min(other), at.max(other));
        let (a, b) = (queue[first].kind, queue[second].kind);
        let needed = self.defs.building(b).requires.iter().any(|r| {
            self.defs.building_kind_id(r) == Some(a)
                && !self.buildings.iter().any(|x| {
                    x.kind == a && x.state != BuildingState::Removed
                })
        });
        if needed {
            return Err(format!(
                "The {} has to come before the {}",
                self.defs.building(a).name,
                self.defs.building(b).name
            ));
        }
        self.settlement.queue.swap(first, second);
        Ok("Plan reordered".into())
    }

    /// Takes a building off the plan. It holds nothing, so nothing is lost.
    pub(crate) fn cancel_queued(&mut self, id: u32) -> Result<String, String> {
        let at = self
            .settlement
            .queue
            .iter()
            .position(|q| q.id == id)
            .ok_or("That isn't on the plan")?;
        let kind = self.settlement.queue[at].kind;
        let def = self.defs.building(kind);
        let dependent = self.settlement.queue[at + 1..].iter().find(|q| {
            self.defs.building(q.kind).requires.contains(&def.id)
                && self.planned_count(kind) < 2
                && !self.buildings.iter().any(|b| {
                    b.kind == kind && b.state != BuildingState::Removed
                })
        });
        if let Some(q) = dependent {
            return Err(format!(
                "The {} needs it: cancel that first",
                self.defs.building(q.kind).name
            ));
        }
        let name = def.name.clone();
        self.settlement.queue.remove(at);
        let msg = format!("{name} taken off the plan");
        self.events.push(self.tick, EventKind::Player, msg.clone());
        Ok(msg)
    }

    /// Calls off a project nobody has started on. Reserved materials go back
    /// to general use where they lie. Once anything has left storage it's
    /// under way, and it has to finish.
    pub(crate) fn cancel_project(&mut self, pid: ProjectId) -> Result<String, String> {
        let Some(p) = self.projects.get(pid) else {
            return Err("No such project".into());
        };
        if !p.is_active() {
            return Err("That project isn't under way".into());
        }
        let def = self.defs.building(p.kind).clone();
        let builder = p.assigned;
        let carrying = builder.is_some_and(|sid| {
            let bag = &self.spirits[sid].actor.inventory;
            def.cost.iter().any(|(r, _)| bag.get(*r) > 0)
        });
        if !p.delivered.is_empty() || carrying || p.state == ProjectState::Building {
            return Err(format!("Work on the {} has started", def.name));
        }
        let (reserved, site, building) = (p.reserved, p.site, p.building);
        if reserved {
            let main = self.main_storage;
            for &(r, n) in &def.cost {
                self.buildings[main].storage.release_reserved(r, n);
            }
        }
        if let Some(sid) = builder {
            let next = self.tick;
            let s = &mut self.spirits[sid];
            s.actor.clear_tasks();
            s.next_think = next;
        }
        let p = &mut self.projects[pid];
        p.state = ProjectState::Cancelled;
        p.reserved = false;
        p.assigned = None;
        p.blocked = None;
        self.buildings[building].state = BuildingState::Removed;
        self.grid.set_occupied(site, false);
        let msg = format!("The {} was called off", def.name);
        self.events.push(self.tick, EventKind::Player, msg.clone());
        Ok(msg)
    }

    /// Starts the next building when the builders are free.
    pub(crate) fn update_queue(&mut self) {
        if self.active_project().is_some() || self.settlement.queue.is_empty() {
            return;
        }
        let head = self.settlement.queue[0].clone();
        let def = self.defs.building(head.kind).clone();
        let missing = def.requires.iter().find(|need| {
            let kind = self.defs.building_kind_id(need);
            !self
                .buildings
                .iter()
                .any(|b| Some(b.kind) == kind && b.is_complete())
        });
        let blocked = if let Some(need) = missing {
            let name = self.defs.building_kind_id(need).map(|k| self.defs.building(k));
            Some(format!(
                "Needs a finished {}",
                name.map_or("?", |d| d.name.as_str())
            ))
        } else {
            let site = match head.site {
                Some(p) => self.site_ready(p).then_some(p).ok_or("the site is occupied".to_string()),
                None => self.default_site(head.kind),
            };
            match site {
                Ok(p) => {
                    self.settlement.queue.remove(0);
                    self.create_project(head.kind, p, "planned by the player".into());
                    let msg = format!("Work begins on the {} at {p}", def.name);
                    self.events.push(self.tick, EventKind::Construction, msg);
                    return;
                }
                Err(e) => Some(e),
            }
        };
        if self.settlement.queue[0].blocked != blocked {
            if let Some(reason) = &blocked {
                let msg = format!("The {} can't start: {reason}", def.name);
                self.events.push(self.tick, EventKind::Construction, msg);
            }
            self.settlement.queue[0].blocked = blocked;
        }
    }

    /// What a planned building still needs, with stock counted once: what
    /// earlier projects will claim isn't available to later ones.
    fn plan_needs(&self, kind: BuildingKindId, spoken_for: &mut Vec<(ResourceId, u32)>) -> Vec<DemandLine> {
        let main = &self.main().storage;
        self.defs
            .building(kind)
            .cost
            .iter()
            .map(|&(r, need)| {
                let claimed: u32 = spoken_for.iter().filter(|c| c.0 == r).map(|c| c.1).sum();
                let have = main.available(r).saturating_sub(claimed).min(need);
                spoken_for.push((r, need));
                DemandLine {
                    name: self.defs.resource(r).name.clone(),
                    have,
                    need,
                    status: if have >= need {
                        "ready".into()
                    } else {
                        self.supply_status(r)
                    },
                }
            })
            .collect()
    }

    /// Who is doing something about a shortage of `r`.
    pub(crate) fn supply_status(&self, r: ResourceId) -> String {
        if self.order_for(r).is_some() {
            let makers: Vec<&str> = self
                .crafters()
                .filter(|s| {
                    matches!(s.actor.queue.front(), Some(Task::Craft { output: Some(o), .. }) if *o == r)
                })
                .map(|s| s.name.as_str())
                .collect();
            if makers.is_empty() {
                let report = self.order_reports().into_iter().find(|o| o.output == r);
                report.map_or(String::new(), |o| o.status)
            } else {
                format!("{} making it", makers.join(", "))
            }
        } else {
            let able = self.gatherers().any(|s| {
                self.defs
                    .tree_tiers(s.skill())
                    .into_iter()
                    .any(|k| self.defs.tree(k).resource == r && self.spirit_can_cut(s, k))
            });
            if able {
                "gatherers are bringing more".into()
            } else {
                "nobody can gather this yet".into()
            }
        }
    }

    fn describe_cost(&self, kind: BuildingKindId) -> String {
        let cost = self.defs.building(kind).cost.iter().copied().collect();
        self.defs.describe_amounts(&cost)
    }

    /// The plan, as a player would read it: what's under way, what's next and
    /// what finished last.
    pub fn plan_views(&self) -> Vec<PlanView> {
        let mut out = Vec::new();
        let mut spoken_for = Vec::new();
        if let Some(p) = self.active_project() {
            let def = self.defs.building(p.kind);
            let stage = match p.state {
                ProjectState::Building => PlanStage::Building,
                ProjectState::Supplying => PlanStage::Supplying,
                _ if p.blocked.as_ref().is_some_and(|b| !b.starts_with("Waiting for materials")) => {
                    PlanStage::Blocked
                }
                _ => PlanStage::GatheringMaterials,
            };
            let needs = if p.reserved {
                Vec::new()
            } else {
                self.plan_needs(p.kind, &mut spoken_for)
            };
            let workers = p.assigned.iter().map(|s| self.spirits[*s].name.clone()).collect();
            let carrying = p.assigned.is_some_and(|sid| {
                let bag = &self.spirits[sid].actor.inventory;
                def.cost.iter().any(|(r, _)| bag.get(*r) > 0)
            });
            let can_cancel = p.delivered.is_empty() && !carrying && p.state != ProjectState::Building;
            out.push(PlanView {
                name: def.name.clone(),
                stage,
                benefit: def.benefit.clone(),
                cost: self.describe_cost(p.kind),
                needs,
                workers,
                reason: p.blocked.clone(),
                queued: None,
                project: Some(p.id),
                can_cancel,
            });
        }
        for (i, q) in self.settlement.queue.iter().enumerate() {
            let def = self.defs.building(q.kind);
            let stage = if q.blocked.is_some() {
                PlanStage::Blocked
            } else if i == 0 && self.active_project().is_none() {
                PlanStage::GatheringMaterials
            } else {
                PlanStage::Queued
            };
            out.push(PlanView {
                name: def.name.clone(),
                stage,
                benefit: def.benefit.clone(),
                cost: self.describe_cost(q.kind),
                needs: self.plan_needs(q.kind, &mut spoken_for),
                workers: Vec::new(),
                reason: q.blocked.clone(),
                queued: Some(q.id),
                project: None,
                can_cancel: true,
            });
        }
        let finished = self
            .projects
            .iter()
            .rev()
            .find(|p| p.state == ProjectState::Complete);
        if let Some(p) = finished {
            let def = self.defs.building(p.kind);
            out.push(PlanView {
                name: def.name.clone(),
                stage: PlanStage::Finished,
                benefit: def.benefit.clone(),
                cost: self.describe_cost(p.kind),
                needs: Vec::new(),
                workers: Vec::new(),
                reason: None,
                queued: None,
                project: Some(p.id),
                can_cancel: false,
            });
        }
        out
    }

    /// The buildings a player can plan right now, in data order.
    pub fn plannable(&self) -> Vec<BuildingKindId> {
        (0..self.defs.buildings.len())
            .map(|i| BuildingKindId(i as u16))
            .filter(|k| {
                let d = self.defs.building(*k);
                d.manual && !d.benefit.is_empty() && self.check_queue(*k, None).is_ok()
            })
            .collect()
    }

    /// The building that opens a site, if it's still shut.
    pub fn site_locked_by(&self, site: crate::defs::SiteId) -> Option<BuildingKindId> {
        let id = &self.defs.site(site).id;
        let kind = self
            .defs
            .buildings
            .iter()
            .position(|b| b.unlocks_site.as_ref() == Some(id))
            .map(|i| BuildingKindId(i as u16))?;
        let open = self.buildings.iter().any(|b| b.kind == kind && b.is_complete());
        (!open).then_some(kind)
    }
}
