//! Player-directed work on the same routes and projects used by the settlement.

use crate::defs::{GameDefs, ResourceId};
use crate::entities::{Building, BuildingId, ProjectId};
use crate::session::SessionEnd;
use crate::tasks::{Goal, Task};
use crate::world::World;

fn walk_to(b: &Building) -> Task {
    Task::Move {
        goal: Goal::Adjacent(b.pos),
        label: b.name.clone(),
    }
}

pub(crate) fn manual_station_room(defs: &GameDefs, station: &Building, r: ResourceId) -> u32 {
    let def = defs.building(station.kind);
    // Raw inputs with a one-way route. An output that a hauler can bring
    // home again must never pay XP for being delivered back to the station.
    let makes = defs
        .recipes
        .iter()
        .filter(|recipe| recipe.station == station.kind);
    let is_input = makes
        .clone()
        .any(|recipe| recipe.inputs.iter().any(|&(input, _)| input == r));
    let is_output = defs.recipes.iter().any(|recipe| recipe.output.0 == r);
    if !def.is_station || def.accepts_gatherer_deposits || !is_input || is_output {
        return 0;
    }
    def.input_buffer
        .saturating_sub(station.storage.get(r))
        .min(station.storage.free())
}

impl World {
    pub fn workforce_level(&self, id: &str) -> u32 {
        self.defs
            .skill_id(id)
            .map_or(1, |skill| self.player.level(skill, &self.defs.xp_table))
    }

    pub fn manual_haul_load(&self) -> u32 {
        self.defs
            .workforce
            .load_at(self.workforce_level("hauling"))
            .min(self.player.actor.inventory.capacity)
    }

    pub(crate) fn earned_hauling_xp(&mut self, amount: u32) {
        if let Some(skill) = self.defs.skill_id("hauling") {
            self.player_earned_xp(
                skill,
                u64::from(amount) * self.defs.workforce.haul_xp_per_unit,
            );
        }
    }

    pub fn project_materials_ready(&self, id: ProjectId) -> bool {
        self.projects.get(id).is_some_and(|p| {
            p.is_active()
                && self
                    .defs
                    .building(p.kind)
                    .cost
                    .iter()
                    .all(|&(r, n)| p.remaining(r, n) == 0)
        })
    }

    fn start_work(&mut self, tasks: Vec<Task>, activity: String) -> Result<String, String> {
        self.end_session(SessionEnd::Player, String::new());
        self.player.actor.redirect(tasks);
        self.player.activity = activity.clone();
        Ok(activity)
    }

    pub(crate) fn start_project_delivery(&mut self, id: ProjectId) -> Result<String, String> {
        let p = self
            .projects
            .get(id)
            .filter(|p| p.is_active())
            .ok_or("That project is no longer under way")?;
        let def = self.defs.building(p.kind);
        let mut tasks = Vec::new();
        let bag = &self.player.actor.inventory;
        if !def
            .cost
            .iter()
            .any(|&(r, n)| p.remaining(r, n) > 0 && bag.get(r) > 0)
        {
            if self.project_materials_ready(id) {
                return Err("The site has all its materials. Choose Help build.".into());
            }
            let (r, amount) = def.cost.iter().find_map(|&(r, n)| {
                let amount = p.held(r).min(p.remaining(r, n));
                (amount > 0).then_some((r, amount))
            }).ok_or("No materials are reserved yet. Gather what's missing and bring it to the site.")?;
            let amount = amount.min(bag.free()).min(self.manual_haul_load());
            if amount == 0 {
                return Err("Make room in your bag for project materials".into());
            }
            tasks.push(walk_to(self.main()));
            tasks.push(Task::WithdrawForProject {
                project: id,
                resource: r,
                amount,
            });
        }
        tasks.push(walk_to(&self.buildings[p.building]));
        tasks.push(Task::DeliverToSite { project: id });
        self.start_work(tasks, format!("Carrying materials to the {}", def.name))
    }

    pub(crate) fn start_building(&mut self, id: ProjectId) -> Result<String, String> {
        let p = self
            .projects
            .get(id)
            .filter(|p| p.is_active())
            .ok_or("That project is no longer under way")?;
        if !self.project_materials_ready(id) {
            return Err("The site needs its materials before you can build".into());
        }
        let site = &self.buildings[p.building];
        let min = p.min_worker_level;
        if self.workforce_level("construction") < min {
            return Err(format!(
                "Building the {} needs Construction level {min}",
                site.name
            ));
        }
        self.start_work(
            vec![walk_to(site), Task::Build { project: id }],
            format!("Building the {}", site.name),
        )
    }

    pub fn station_delivery(&self, id: BuildingId) -> Result<(ResourceId, u32, bool), String> {
        let station = self
            .buildings
            .get(id)
            .filter(|b| b.is_complete() && self.defs.building(b.kind).is_station)
            .ok_or("There is no finished station here")?;
        let bag = &self.player.actor.inventory;
        let load = self.manual_haul_load();
        let mut needed = false;
        for i in 0..self.defs.resources.len() {
            let r = ResourceId(i as u16);
            let room = manual_station_room(&self.defs, station, r);
            if room == 0 {
                continue;
            }
            needed = true;
            if bag.get(r) > 0 {
                return Ok((r, bag.get(r).min(room).min(load), false));
            }
            let pickup = self.main().storage.available(r).min(bag.free());
            if pickup > 0 {
                return Ok((r, pickup.min(room).min(load), true));
            }
        }
        Err(if needed {
            "Put the station's raw materials in Main Storage or bring some in your bag".into()
        } else {
            format!("{} has enough raw materials", station.name)
        })
    }

    pub(crate) fn start_station_delivery(&mut self, id: BuildingId) -> Result<String, String> {
        let (resource, amount, pickup) = self.station_delivery(id)?;
        let station = &self.buildings[id];
        let mut tasks = Vec::new();
        if pickup {
            tasks.push(walk_to(self.main()));
            tasks.push(Task::Withdraw {
                building: self.main_storage,
                resource,
                amount,
                reserved: false,
            });
        }
        tasks.push(walk_to(station));
        tasks.push(Task::SupplyStation {
            building: id,
            resource,
            amount,
        });
        let name = &self.defs.resource(resource).name;
        self.start_work(
            tasks,
            format!("Carrying {amount} {name} to the {}", station.name),
        )
    }
}
