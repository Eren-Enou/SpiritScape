//! Long-term goals. Like the journey, these watch the world and change nothing:
//! there's no reward, and doing them in another order is fine. They exist so a
//! player past the first cycle always has somewhere to look next.

use crate::world::World;
use serde::Deserialize;

/// A goal, from data. It's met when every condition given is.
#[derive(Clone, Debug, Deserialize)]
pub struct AmbitionDef {
    pub id: String,
    pub title: String,
    pub hint: String,
    /// Buildings that must all be finished.
    #[serde(default)]
    pub built: Vec<String>,
    /// A skill to reach `level` in, at any time.
    #[serde(default)]
    pub skill: Option<String>,
    #[serde(default)]
    pub level: u32,
    /// A resource to have made `amount` of, ever.
    #[serde(default)]
    pub made: Option<String>,
    #[serde(default)]
    pub amount: u32,
    /// Trade Caravans to have sent.
    #[serde(default)]
    pub caravans: u32,
    /// Old stones to have read.
    #[serde(default)]
    pub markers: u32,
}

/// How a goal stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ambition {
    pub title: String,
    pub hint: String,
    pub done: bool,
    /// How far along it is, when that's a number.
    pub progress: Option<(u64, u64, String)>,
}

impl World {
    fn finished(&self, id: &str) -> bool {
        let kind = self.defs.building_kind_id(id);
        self.buildings
            .iter()
            .any(|b| Some(b.kind) == kind && b.is_complete())
    }

    fn ambition(&self, def: &AmbitionDef) -> Ambition {
        let mut done = def.built.iter().all(|b| self.finished(b));
        let mut progress = None;
        if def.built.len() > 1 {
            let have = def.built.iter().filter(|b| self.finished(b)).count() as u64;
            progress = Some((have, def.built.len() as u64, "built".to_string()));
        }
        if let Some(skill) = def.skill.as_ref().and_then(|s| self.defs.skill_id(s)) {
            let now = self.player.level(skill, &self.defs.xp_table);
            let best = self.player.history(skill).highest_level.max(now);
            done &= best >= def.level;
            let name = &self.defs.skill(skill).name;
            progress = Some((best as u64, def.level as u64, format!("{name} level")));
        }
        if let Some(r) = def.made.as_ref().and_then(|r| self.defs.resource_id(r)) {
            let have = self.ledger.crafted.get(&r).copied().unwrap_or(0);
            done &= have >= def.amount as u64;
            let name = self.defs.resource(r).name.clone();
            progress = Some((have, def.amount as u64, name));
        }
        if def.markers > 0 {
            let read = self.settlement.markers_read.len() as u32;
            done &= read >= def.markers;
            progress = Some((read as u64, def.markers as u64, "read".to_string()));
        }
        if def.caravans > 0 {
            let sent = self.settlement.caravans;
            done &= sent >= def.caravans;
            progress = Some((sent as u64, def.caravans as u64, "sent".to_string()));
        }
        Ambition {
            title: def.title.clone(),
            hint: def.hint.clone(),
            done,
            progress,
        }
    }

    /// Every long-term goal, in the order they're suggested.
    pub fn ambitions(&self) -> Vec<Ambition> {
        self.defs
            .ambitions
            .iter()
            .map(|d| self.ambition(d))
            .collect()
    }

    /// Tells a playthrough when a goal is met. The first look only takes note
    /// of what's already done.
    pub(crate) fn update_ambitions(&mut self) {
        if self.journey.is_none() {
            return;
        }
        let done: Vec<(String, String)> = self
            .defs
            .ambitions
            .iter()
            .filter(|d| self.ambition(d).done)
            .map(|d| (d.id.clone(), d.title.clone()))
            .collect();
        let tick = self.tick;
        let Some(seen) = &mut self.settlement.ambitions_seen else {
            self.settlement.ambitions_seen = Some(done.into_iter().map(|d| d.0).collect());
            return;
        };
        for (id, title) in done {
            if !seen.contains(&id) {
                seen.push(id);
                self.notices.push(tick, format!("Ambition met: {title}"));
            }
        }
    }

    /// The next goal not yet met.
    pub fn next_ambition(&self) -> Option<Ambition> {
        self.ambitions().into_iter().find(|a| !a.done)
    }
}
