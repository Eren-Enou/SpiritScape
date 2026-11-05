//! Settlement planner: watches gatherer deliveries, detects inefficient
//! logistics and decides whether an autonomous building would solve them.
//!
//! Pipeline (each step must pass before the next runs):
//! 1. detect   - recent deliveries spend too much time walking
//! 2. evaluate - a buildable site exists that cuts the delivery leg enough
//! 3. check    - permission, building limits, builder and hauler available
//! 4. create   - place a construction site and a project
//!
//! Projects are then advanced every tick: reserve materials, assign a builder.
//! The builder physically moves the reserved materials and builds.

use crate::defs::{BuildingKindId, JobKind};
use crate::entities::{
    Building, BuildingId, BuildingState, Project, ProjectId, ProjectState, SpiritId,
};
use crate::events::EventKind;
use crate::grid::TilePos;
use crate::inventory::Storage;
use crate::world::World;
use std::collections::{BTreeMap, VecDeque};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DeliverySample {
    pub tick: u64,
    pub spirit: SpiritId,
    pub storage: BuildingId,
    pub gather_pos: TilePos,
    pub leg_tiles: u32,
    pub work_ticks: u32,
    pub travel_ticks: u32,
    pub amount: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProblemReport {
    pub samples: usize,
    pub avg_leg_tiles: u32,
    pub travel_share_pct: u32,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct PlannerState {
    pub samples: VecDeque<DeliverySample>,
    pub problem: Option<ProblemReport>,
    pub status: String,
    pub cooldown_until: u64,
    pub last_rejection: Option<String>,
    problem_logged: bool,
    permission_logged: bool,
}

impl World {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn record_delivery(
        &mut self,
        spirit: SpiritId,
        storage: BuildingId,
        gather_pos: TilePos,
        leg_tiles: u32,
        work_ticks: u32,
        amount: u32,
        move_ticks: u32,
    ) {
        let planner = &mut self.settlement.planner;
        planner.samples.push_back(DeliverySample {
            tick: self.tick,
            spirit,
            storage,
            gather_pos,
            leg_tiles,
            work_ticks,
            // The walk back to the gathering spot is assumed to mirror the delivery leg.
            travel_ticks: 2 * leg_tiles * move_ticks,
            amount,
        });
        let keep = self.defs.planner.delivery_window * 4;
        while planner.samples.len() > keep {
            planner.samples.pop_front();
        }
    }

    /// Recent samples within the configured window and age limit.
    fn recent_samples(&self) -> Vec<DeliverySample> {
        let rules = &self.defs.planner;
        let min_tick = self.tick.saturating_sub(rules.max_sample_age_ticks);
        let mut v: Vec<_> = self
            .settlement
            .planner
            .samples
            .iter()
            .filter(|s| s.tick >= min_tick)
            .cloned()
            .collect();
        let skip = v.len().saturating_sub(rules.delivery_window);
        v.drain(..skip);
        v
    }

    /// Measures route efficiency from recent deliveries.
    pub fn measure_logistics(&self) -> Option<ProblemReport> {
        let samples = self.recent_samples();
        if samples.len() < self.defs.planner.delivery_window {
            return None;
        }
        let n = samples.len() as u32;
        let avg_leg_tiles = samples.iter().map(|s| s.leg_tiles).sum::<u32>() / n;
        let travel: u64 = samples.iter().map(|s| s.travel_ticks as u64).sum();
        let work: u64 = samples.iter().map(|s| s.work_ticks as u64).sum();
        let travel_share_pct = (travel * 100 / (travel + work).max(1)) as u32;
        Some(ProblemReport {
            samples: samples.len(),
            avg_leg_tiles,
            travel_share_pct,
        })
    }

    fn set_status(&mut self, status: impl Into<String>) {
        self.settlement.planner.status = status.into();
    }

    /// Rejects the current plan. `category` identifies the kind of rejection
    /// so repeated re-evaluations with slightly different numbers aren't re-logged.
    fn reject(&mut self, kind: BuildingKindId, category: &str, reason: String) {
        let name = self.defs.building(kind).name.clone();
        let planner = &mut self.settlement.planner;
        planner.cooldown_until = self.tick + self.defs.planner.reject_cooldown_ticks;
        planner.status = format!("Rejected {name}: {reason}");
        if planner.last_rejection.as_deref() != Some(category) {
            planner.last_rejection = Some(category.to_string());
            self.events.push(
                self.tick,
                EventKind::Planner,
                format!("Considered a {name} but rejected it: {reason}"),
            );
        }
    }

    /// Periodic planner evaluation (steps 1-4).
    pub(crate) fn evaluate_logistics(&mut self) {
        if let Some(p) = self.active_project() {
            let name = self.defs.building(p.kind).name.clone();
            self.set_status(format!("Managing {name} project"));
            return;
        }
        // The player's plan comes first.
        if !self.settlement.queue.is_empty() {
            self.set_status("Following the construction plan");
            return;
        }
        // Only storage-type autonomous buildings exist for now.
        let Some(kind) = self.next_autonomous_kind() else {
            self.set_status("All planned infrastructure built; monitoring deliveries");
            return;
        };
        let rules = self.defs.planner.clone();

        // 1. Detect
        let Some(report) = self.measure_logistics() else {
            let n = self.recent_samples().len();
            self.settlement.planner.problem = None;
            self.settlement.planner.problem_logged = false;
            self.set_status(format!(
                "Monitoring deliveries ({n}/{} recent samples)",
                rules.delivery_window
            ));
            return;
        };
        if report.travel_share_pct < rules.travel_share_threshold_pct
            || report.avg_leg_tiles < rules.min_leg_tiles
        {
            self.settlement.planner.problem = None;
            self.settlement.planner.problem_logged = false;
            self.set_status(format!(
                "Logistics OK: deliveries average {} tiles, walking is {}% of trip time",
                report.avg_leg_tiles, report.travel_share_pct
            ));
            return;
        }
        let problem_text = format!(
            "gatherer deliveries average {} tiles and walking takes {}% of trip time (limits: {} tiles, {}%)",
            report.avg_leg_tiles,
            report.travel_share_pct,
            rules.min_leg_tiles,
            rules.travel_share_threshold_pct
        );
        self.settlement.planner.problem = Some(report.clone());
        if !self.settlement.planner.problem_logged {
            self.settlement.planner.problem_logged = true;
            self.events.push(
                self.tick,
                EventKind::Planner,
                format!("Inefficiency detected: {problem_text}"),
            );
        }
        if self.tick < self.settlement.planner.cooldown_until {
            let secs = (self.settlement.planner.cooldown_until - self.tick)
                / self.defs.ticks_per_second as u64;
            self.set_status(format!("Problem persists; re-evaluating in {secs}s"));
            return;
        }

        // 2. Evaluate a solution
        // Place the building to serve the long deliveries that cause the problem.
        let long: Vec<_> = self
            .recent_samples()
            .into_iter()
            .filter(|s| s.leg_tiles >= rules.min_leg_tiles)
            .collect();
        let long_avg = long.iter().map(|s| s.leg_tiles).sum::<u32>() / (long.len() as u32).max(1);
        let Some((site, est_leg)) = self.find_site(&long) else {
            self.reject(
                kind,
                "site",
                "no buildable site near the gathering area".into(),
            );
            return;
        };
        // Depositing always takes at least a step, even from a tile next to the trees.
        let est_leg = est_leg.max(1);
        let savings_pct = 100u32.saturating_sub(est_leg * 100 / long_avg.max(1));
        if savings_pct < rules.min_savings_pct {
            self.reject(
                kind,
                "savings",
                format!(
                    "best site {site} only shortens deliveries by {savings_pct}% (need {}%)",
                    rules.min_savings_pct
                ),
            );
            return;
        }

        // 3. Permissions and workers
        let name = self.defs.building(kind).name.clone();
        if !self.settlement.allow_autonomous_construction {
            self.set_status(format!(
                "{name} at {site} is justified, but autonomous construction is not permitted"
            ));
            if !self.settlement.planner.permission_logged {
                self.settlement.planner.permission_logged = true;
                self.events.push(
                    self.tick,
                    EventKind::Planner,
                    format!("Wants to build a {name} at {site}, but autonomous construction is not permitted"),
                );
            }
            return;
        }
        self.settlement.planner.permission_logged = false;
        if !self.has_role_with(JobKind::Construct) {
            self.reject(
                kind,
                "builder",
                "no Construction Spirit exists to build it".into(),
            );
            return;
        }
        if !self.has_role_with(JobKind::Haul) {
            self.reject(
                kind,
                "hauler",
                "no Hauling Spirit exists to empty it into Main Storage".into(),
            );
            return;
        }

        // 4. Create the project
        let def = self.defs.building(kind);
        let cost: BTreeMap<_, _> = def.cost.iter().copied().collect();
        let reason = format!(
            "{problem_text}; a {name} at {site} cuts long legs from ~{long_avg} to ~{est_leg} tiles ({savings_pct}% shorter)"
        );
        let pid = self.create_project(kind, site, reason.clone());
        self.settlement.planner.last_rejection = None;
        self.events.push(
            self.tick,
            EventKind::Planner,
            format!(
                "Decision: build a {name} at {site} (cost {}). Reason: {reason}",
                self.defs.describe_amounts(&cost)
            ),
        );
        self.set_status(format!("Started project #{pid}: {name} at {site}"));
        // Reserve materials and assign a builder only after the decision is
        // logged, so the event log reads in cause-and-effect order.
        self.update_projects();
    }

    fn next_autonomous_kind(&self) -> Option<BuildingKindId> {
        self.defs.buildings.iter().enumerate().find_map(|(i, def)| {
            let kind = BuildingKindId(i as u16);
            let count = self.buildings.iter().filter(|b| b.kind == kind).count() as u32;
            (def.autonomous && def.max_count.is_none_or(|m| count < m)).then_some(kind)
        })
    }

    /// Finds the buildable tile that minimizes the average walk from the
    /// given gathering spots, without cutting off any reachable tree or storage.
    fn find_site(&self, samples: &[DeliverySample]) -> Option<(TilePos, u32)> {
        let gather: Vec<TilePos> = samples.iter().map(|s| s.gather_pos).collect();
        if gather.is_empty() {
            return None;
        }
        let n = gather.len() as i32;
        let cx = (gather.iter().map(|p| p.x).sum::<i32>() + n / 2) / n;
        let cy = (gather.iter().map(|p| p.y).sum::<i32>() + n / 2) / n;
        let r = self.defs.planner.site_search_radius;

        let mut occupied_by_actors: Vec<TilePos> = vec![self.player.actor.pos()];
        for s in &self.spirits {
            occupied_by_actors.push(s.actor.pos());
            occupied_by_actors.extend(s.actor.mover.path.front().copied());
        }
        occupied_by_actors.extend(self.player.actor.mover.path.front().copied());

        let mut candidates = Vec::new();
        for y in cy - r..=cy + r {
            for x in cx - r..=cx + r {
                let p = TilePos::new(x, y);
                if !self.grid.is_buildable(p) || occupied_by_actors.contains(&p) {
                    continue;
                }
                // Avoid chokepoints: need at least two open sides.
                if self.grid.walkable_neighbors(p).count() < 2 {
                    continue;
                }
                let sources: Vec<TilePos> = self.grid.walkable_neighbors(p).collect();
                let field = self.grid.distance_field_with_block(&sources, Some(p));
                let mut total = 0;
                let mut ok = true;
                for g in &gather {
                    match self
                        .grid
                        .walkable_neighbors(*g)
                        .filter(|t| *t != p)
                        .filter_map(|t| field.get(t))
                        .min()
                    {
                        Some(d) => total += d,
                        None => {
                            ok = false;
                            break;
                        }
                    }
                }
                if ok {
                    candidates.push((total / gather.len() as u32, p));
                }
            }
        }
        candidates.sort_by_key(|&(d, p)| (d, p.y, p.x));
        candidates
            .into_iter()
            .take(10)
            .find(|&(_, p)| self.keeps_connectivity(p))
            .map(|(d, p)| (p, d))
    }

    /// True if blocking `p` leaves every currently reachable tree and
    /// building reachable from main storage.
    fn keeps_connectivity(&self, p: TilePos) -> bool {
        // Open ground around the site has to stay joined up, or someone
        // standing there could be walled in for good.
        let around: Vec<TilePos> = self.grid.walkable_neighbors(p).collect();
        if let Some(first) = around.first() {
            let field = self.grid.distance_field_with_block(&[*first], Some(p));
            if around.iter().any(|t| field.get(*t).is_none()) {
                return false;
            }
        }
        let main = self.main().pos;
        let sources: Vec<TilePos> = self.grid.walkable_neighbors(main).collect();
        let before = self.grid.distance_field(&sources);
        let after = self.grid.distance_field_with_block(&sources, Some(p));
        let reachable_after = |target: TilePos| {
            self.grid
                .walkable_neighbors(target)
                .filter(|t| *t != p)
                .any(|t| after.get(t).is_some())
        };
        let targets = self
            .trees
            .iter()
            .map(|t| t.pos)
            .chain(self.buildings.iter().map(|b| b.pos))
            .chain([p]);
        targets.into_iter().all(|t| {
            let was = t == p || self.grid.approach_distance(&before, t).is_some();
            !was || reachable_after(t)
        })
    }

    /// The player approves a building the planner won't start on its own.
    /// It goes up near Main Storage and is built like any other project.
    pub(crate) fn request_building(&mut self, kind: BuildingKindId) -> Result<String, String> {
        self.check_order(kind)?;
        let site = self.default_site(kind)?;
        Ok(self.start_order(kind, site))
    }

    /// Where a building goes when the player doesn't choose.
    pub(crate) fn default_site(&self, kind: BuildingKindId) -> Result<TilePos, String> {
        let def = self.defs.building(kind);
        if let Some([x, y]) = def.access_site {
            return Ok(TilePos::new(x, y));
        }
        if !def.paves.is_empty() {
            // Work starts at the near end, or the first place that's clear.
            return def
                .paves
                .iter()
                .map(|&[x, y]| TilePos::new(x, y))
                .find(|p| self.site_ready(*p))
                .ok_or_else(|| "There's nowhere clear to start".to_string());
        }
        if let Some(site) = def.home_site.as_ref().and_then(|s| self.defs.site_id(s)) {
            let (x, y) = self.defs.site(site).center();
            return self
                .site_near(TilePos::new(x as i32, y as i32), 0)
                .ok_or_else(|| "No room there".to_string());
        }
        if def.slot_bonus > 0 {
            let bench = self.training_bench().ok_or("There's no bench to add to")?;
            return self
                .site_near(bench.pos, 1)
                .ok_or_else(|| "No room beside the bench".to_string());
        }
        if def.training_cache {
            let area = self
                .settlement
                .training
                .area
                .ok_or_else(|| "Choose a training area first".to_string())?;
            let (x, y) = self.defs.site(area).center();
            self.site_near(TilePos::new(x as i32, y as i32), 0)
                .ok_or_else(|| "No room in the training area".to_string())
        } else {
            self.site_near(self.main().pos, 2)
                .ok_or_else(|| "No room near Main Storage".to_string())
        }
    }

    /// The player picks the tile. Workers must be able to reach it, and it
    /// mustn't cut anything off.
    pub(crate) fn request_building_at(
        &mut self,
        kind: BuildingKindId,
        site: TilePos,
    ) -> Result<String, String> {
        self.check_order(kind)?;
        if !self.site_ready(site) {
            return Err(format!("You can't build at {site}"));
        }
        Ok(self.start_order(kind, site))
    }

    /// Whether a tile can take a building right now.
    pub(crate) fn site_ready(&self, site: TilePos) -> bool {
        let occupied = self.player.actor.pos() == site
            || self
                .spirits
                .iter()
                .any(|s| s.actor.pos() == site || s.actor.mover.path.iter().any(|p| *p == site));
        self.grid.is_buildable(site)
            && !occupied
            && self.grid.walkable_neighbors(site).count() >= 2
            && self.keeps_connectivity(site)
    }

    /// Whether a building can be ordered at all right now.
    fn check_order(&self, kind: BuildingKindId) -> Result<(), String> {
        let def = self.defs.building(kind);
        let name = &def.name;
        if !def.manual {
            return Err(format!("A {name} can't be ordered"));
        }
        let count = self
            .buildings
            .iter()
            .filter(|b| b.kind == kind && b.state != BuildingState::Removed)
            .count() as u32;
        if def.max_count.is_some_and(|m| count >= m) {
            return Err(format!("The settlement can't have more {name}s"));
        }
        if !self.settlement.queue.is_empty() {
            return Err("Finish the current construction project first: the plan".into());
        }
        if let Some(p) = self.active_project() {
            return Err(format!(
                "Finish the current construction project first: the {}",
                self.defs.building(p.kind).name
            ));
        }
        Ok(())
    }

    fn start_order(&mut self, kind: BuildingKindId, site: TilePos) -> String {
        let def = self.defs.building(kind);
        let name = def.name.clone();
        let cost: BTreeMap<_, _> = def.cost.iter().copied().collect();
        let article = if name.starts_with(['A', 'E', 'I', 'O', 'U']) {
            "an"
        } else {
            "a"
        };
        let msg = format!(
            "Ordered {article} {name} at {site} (cost {})",
            self.defs.describe_amounts(&cost)
        );
        self.create_project(kind, site, "ordered by the player".into());
        self.events.push(self.tick, EventKind::Player, msg.clone());
        self.update_projects();
        msg
    }

    /// The closest free tile to `center`, at least `min_distance` from it
    /// (workers need to reach what's there), that doesn't cut anything off.
    fn site_near(&self, center: TilePos, min_distance: i32) -> Option<TilePos> {
        let r = self.defs.planner.site_search_radius;
        let mut blocked: Vec<TilePos> = vec![self.player.actor.pos()];
        blocked.extend(self.player.actor.mover.path.iter().copied());
        for s in &self.spirits {
            blocked.push(s.actor.pos());
            blocked.extend(s.actor.mover.path.iter().copied());
        }
        let mut candidates = Vec::new();
        for y in center.y - r..=center.y + r {
            for x in center.x - r..=center.x + r {
                let p = TilePos::new(x, y);
                let distance = (x - center.x).abs() + (y - center.y).abs();
                if distance >= min_distance
                    && self.grid.is_buildable(p)
                    && !blocked.contains(&p)
                    && self.grid.walkable_neighbors(p).count() >= 3
                {
                    candidates.push((distance, p.y, p.x, p));
                }
            }
        }
        candidates.sort_by_key(|&(d, y, x, _)| (d, y, x));
        candidates
            .into_iter()
            .map(|c| c.3)
            .find(|p| self.keeps_connectivity(*p))
    }

    pub(crate) fn create_project(
        &mut self,
        kind: BuildingKindId,
        site: TilePos,
        reason: String,
    ) -> ProjectId {
        let pid = self.projects.len();
        let bid = self.buildings.len();
        let def = self.defs.building(kind);
        self.buildings.push(Building {
            id: bid,
            kind,
            pos: site,
            name: def.name.clone(),
            state: BuildingState::UnderConstruction { project: pid },
            storage: Storage::new(
                def.storage_capacity()
                    + if def.is_station {
                        self.settlement.bench_bonus
                    } else {
                        0
                    },
            ),
            crafted: 0,
            keep: BTreeMap::new(),
            extra_slots: 0,
        });
        self.grid.set_occupied(site, true);
        self.projects.push(Project {
            id: pid,
            kind,
            building: bid,
            site,
            state: ProjectState::AwaitingMaterials,
            assigned: None,
            reserved: false,
            claimed: BTreeMap::new(),
            delivered: BTreeMap::new(),
            consumed: BTreeMap::new(),
            build_progress: 0,
            build_required: 0,
            reason,
            blocked: None,
            created_tick: self.tick,
            completed_tick: None,
        });
        pid
    }

    /// Per-tick project upkeep: reserve materials and assign a builder.
    pub(crate) fn update_projects(&mut self) {
        for pid in 0..self.projects.len() {
            if !self.projects[pid].is_active() {
                continue;
            }
            let kind = self.projects[pid].kind;
            let def = self.defs.building(kind).clone();
            let mut blocked = None;

            if !self.projects[pid].reserved {
                let tight = self.main().storage.free() < self.defs.construction.raw_room;
                let main = &mut self.buildings[self.main_storage].storage;
                let p = &mut self.projects[pid];
                let left = |p: &Project, r, n| n - p.claimed_of(r, n).min(n);
                let short: Vec<String> = def
                    .cost
                    .iter()
                    .filter(|&&(r, n)| main.available(r) < left(p, r, n))
                    .map(|&(r, n)| {
                        let have = main.available(r) + p.claimed_of(r, n);
                        format!("{have}/{n} {}", self.defs.resource(r).name)
                    })
                    .collect();
                if short.is_empty() {
                    for &(r, n) in &def.cost {
                        main.try_reserve(r, left(p, r, n));
                    }
                    p.reserved = true;
                    p.claimed.clear();
                    p.state = ProjectState::Supplying;
                    let cost: BTreeMap<_, _> = def.cost.iter().copied().collect();
                    let msg = format!(
                        "Reserved {} in Main Storage for the {}",
                        self.defs.describe_amounts(&cost),
                        def.name
                    );
                    self.events.push(self.tick, EventKind::Construction, msg);
                } else {
                    // With no room for what's missing, what's here starts
                    // moving. That frees the room the rest needs.
                    if tight {
                        for &(r, n) in &def.cost {
                            let take = left(p, r, n).min(main.available(r));
                            if take > 0 && main.try_reserve(r, take) {
                                *p.claimed.entry(r).or_default() += take;
                            }
                        }
                    }
                    blocked = Some(format!(
                        "Waiting for materials: only {} available in Main Storage",
                        short.join(", ")
                    ));
                }
            }

            if self.projects[pid].assigned.is_none() {
                let busy: Vec<SpiritId> = self
                    .projects
                    .iter()
                    .filter(|p| p.is_active())
                    .filter_map(|p| p.assigned)
                    .collect();
                let builder = self
                    .spirits
                    .iter()
                    .find(|s| {
                        self.defs.role(s.role).has_job(JobKind::Construct) && !busy.contains(&s.id)
                    })
                    .map(|s| s.id);
                match builder {
                    Some(sid) => {
                        self.projects[pid].assigned = Some(sid);
                        // Let the builder react immediately.
                        self.spirits[sid].next_think = self.tick;
                        let msg = format!(
                            "Assigned {} to build the {}",
                            self.spirits[sid].name, def.name
                        );
                        self.events.push(self.tick, EventKind::Construction, msg);
                    }
                    None => {
                        blocked.get_or_insert_with(|| "No Construction Spirit available".into());
                    }
                }
            }

            let p = &mut self.projects[pid];
            if p.blocked != blocked {
                // Logged once: the text changes as materials trickle in.
                if let (Some(reason), None) = (&blocked, &p.blocked) {
                    let msg = format!("{} project blocked: {reason}", def.name);
                    self.events.push(self.tick, EventKind::Construction, msg);
                }
                p.blocked = blocked;
            }
        }
    }

    pub(crate) fn on_project_completed(&mut self, pid: ProjectId) {
        let p = &self.projects[pid];
        let def = self.defs.building(p.kind);
        let bonus = def.capacity_bonus;
        let mut msg = format!(
            "{} completed at {} using {}.",
            def.name,
            p.site,
            self.defs.describe_amounts(&p.consumed)
        );
        let storage = &mut self.buildings[self.main_storage].storage;
        storage.capacity += bonus;
        if bonus > 0 {
            msg += &format!(" Main Storage now holds {} (+{bonus}).", storage.capacity);
        }
        let bench_bonus = def.training_bonus;
        let is_station = def.is_station;
        let opens = def.opens.clone();
        let paves = def.paves.clone();
        let carry_bonus = def.carry_bonus;
        let repeatable = def.repeatable;
        let building = p.building;
        let slot_bonus = def.slot_bonus;
        let site = p.site;
        let benefit = def.benefit.clone();
        let name = def.name.clone();
        let notice = if repeatable {
            Some(format!(
                "{name} sent. That's {} so far.",
                self.settlement.caravans + 1
            ))
        } else if !opens.is_empty() || !paves.is_empty() || slot_bonus > 0 || carry_bonus > 0 {
            Some(format!("{name} complete. {benefit}"))
        } else if is_station {
            Some(format!(
                "{} complete. It's a new place to work, with {} more benches.",
                def.name, def.slots
            ))
        } else if bench_bonus > 0 {
            Some(format!(
                "{} complete. The bench can hold {bench_bonus} more logs for your training.",
                def.name
            ))
        } else if bonus > 0 {
            Some(format!(
                "{} complete. Settlement storage capacity increased to {}.",
                def.name, storage.capacity
            ))
        } else if def.training_cache {
            Some(format!(
                "{} complete. Drop your logs here and a Hauling Spirit will collect them.",
                def.name
            ))
        } else {
            None
        };
        let gatherers_use_it = def.accepts_gatherer_deposits;
        for [x, y] in opens {
            self.grid.open_crossing(TilePos::new(x, y));
        }
        if repeatable {
            // It's gone, and the place it was loaded is free again.
            self.settlement.caravans += 1;
            self.grid.set_occupied(site, false);
            self.buildings[building].state = BuildingState::Removed;
        }
        if carry_bonus > 0 {
            self.settlement.carry_bonus += carry_bonus;
            for s in &mut self.spirits {
                s.actor.inventory.capacity += carry_bonus;
            }
        }
        if !paves.is_empty() {
            // The works are over: the place they stood is road like the rest.
            self.grid.set_occupied(site, false);
            let pct = self.defs.construction.road_pct;
            for [x, y] in paves {
                self.grid.pave(TilePos::new(x, y), pct);
            }
        }
        if slot_bonus > 0 {
            let reach = self.defs.construction.annex_reach;
            let bench = self
                .stations()
                .map(|b| (b.pos.manhattan(site), b.id))
                .filter(|(d, _)| *d <= reach)
                .min();
            if let Some((_, id)) = bench {
                self.buildings[id].extra_slots += slot_bonus;
            }
        }
        if bench_bonus > 0 {
            self.settlement.bench_bonus += bench_bonus;
            for b in &mut self.buildings {
                if self.defs.buildings[b.kind.index()].is_station {
                    b.storage.capacity += bench_bonus;
                }
            }
        }
        if gatherers_use_it {
            msg += " Gatherers can now deliver here; haulers carry its stock to Main Storage.";
        }
        self.events.push(self.tick, EventKind::Construction, msg);
        if let Some(text) = notice {
            self.notices.push(self.tick, text);
        }
        if !gatherers_use_it {
            return;
        }
        // Logistics changed: start measuring afresh.
        let planner = &mut self.settlement.planner;
        planner.samples.clear();
        planner.problem = None;
        planner.problem_logged = false;
        self.set_status("New infrastructure complete; re-measuring deliveries");
    }
}

impl World {
    /// Anyone cut off from the main stretch of open ground steps out to its
    /// nearest edge. Buildings can't do this any more, but older saves can
    /// hold a spirit walled in for good. What they were doing is left alone:
    /// a walk simply finds its route again from the new place.
    pub(crate) fn free_the_stranded(&mut self) {
        let land = self.grid.mainland();
        let stuck = |w: &World, p: TilePos| !w.grid.on_land(&land, p);
        let player = self.player.actor.pos();
        if stuck(self, player)
            && let Some(to) = self.grid.nearest_land(&land, player)
        {
            let m = &mut self.player.actor.mover;
            (m.pos, m.progress) = (to, 0);
            m.path.clear();
            self.notices
                .push(self.tick, "You climbed out of a walled-in corner.");
        }
        for id in 0..self.spirits.len() {
            let at = self.spirits[id].actor.pos();
            if !stuck(self, at) {
                continue;
            }
            let Some(to) = self.grid.nearest_land(&land, at) else {
                continue;
            };
            let s = &mut self.spirits[id];
            let m = &mut s.actor.mover;
            (m.pos, m.progress) = (to, 0);
            m.path.clear();
            s.next_think = self.tick;
            let msg = format!("{} was walled in, and climbed out at {to}", s.name);
            self.events.push(self.tick, EventKind::Spirit, msg.clone());
            self.notices.push(self.tick, msg);
        }
    }

    /// Clone-free accessor for UI: delivery samples currently considered.
    pub fn planner_samples(&self) -> Vec<DeliverySample> {
        self.recent_samples()
    }
}
