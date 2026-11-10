//! Teaching each system when it first matters. A new game shows little, and
//! each tip opens the next part of the screen with a line about what it's for.
//! Like the journey this only watches: everything still works, with or without
//! the words, and a key press opens any tab early.

use crate::journey::Phase;
use crate::world::World;
use serde::Deserialize;

/// Something to say once, and what it opens.
#[derive(Clone, Debug, Deserialize)]
pub struct TipDef {
    pub id: String,
    /// `phase:NAME`, `built:ID`, `level:SKILL:N`, `season:ID` or `caravans:N`.
    pub when: String,
    pub text: String,
    /// The tab or skill that stays hidden until this has been said.
    #[serde(default)]
    pub opens: Option<String>,
}

fn phase_named(name: &str) -> Option<Phase> {
    Some(match name {
        "beginnings" => Phase::Beginnings,
        "first_spirit" => Phase::FirstSpirit,
        "first_craft" => Phase::FirstCraft,
        "automation" => Phase::Automation,
        "improvement" => Phase::Improvement,
        "return" => Phase::Return,
        "settled" => Phase::Settled,
        _ => return None,
    })
}

impl World {
    /// Whether a tip's condition reads properly, for checking the data.
    pub fn tip_condition_known(&self, when: &str) -> bool {
        let parts: Vec<&str> = when.split(':').collect();
        match parts.as_slice() {
            ["phase", name] => phase_named(name).is_some(),
            ["built", id] => self.defs.building_kind_id(id).is_some(),
            ["level", skill, n] => self.defs.skill_id(skill).is_some() && n.parse::<u32>().is_ok(),
            ["season", id] => self.defs.seasons.iter().any(|s| s.id == *id),
            ["caravans", n] => n.parse::<u32>().is_ok(),
            _ => false,
        }
    }

    fn tip_condition_met(&self, when: &str) -> bool {
        let parts: Vec<&str> = when.split(':').collect();
        match parts.as_slice() {
            ["phase", name] => {
                let now = self.journey.as_ref().map(|j| j.phase);
                now.zip(phase_named(name)).is_some_and(|(a, b)| a >= b)
            }
            ["built", id] => {
                let kind = self.defs.building_kind_id(id);
                self.buildings
                    .iter()
                    .any(|b| Some(b.kind) == kind && b.is_complete())
            }
            ["level", skill, n] => self.defs.skill_id(skill).is_some_and(|s| {
                let now = self.player.level(s, &self.defs.xp_table);
                let best = self.player.history(s).highest_level.max(now);
                n.parse::<u32>().is_ok_and(|n| best >= n)
            }),
            ["season", id] => self.season().is_some_and(|s| s.id == *id),
            ["caravans", n] => n
                .parse::<u32>()
                .is_ok_and(|n| self.settlement.caravans >= n),
            _ => false,
        }
    }

    /// Says what's newly become relevant. The first look only takes note of
    /// what has already happened, so loading a save doesn't repeat them all.
    pub(crate) fn update_tips(&mut self) {
        if self.journey.is_none() {
            return;
        }
        let due: Vec<(String, String)> = self
            .defs
            .tips
            .iter()
            .filter(|t| self.tip_condition_met(&t.when))
            .map(|t| (t.id.clone(), t.text.clone()))
            .collect();
        let tick = self.tick;
        // A brand new game has nothing to catch up on, so it starts saying them.
        let fresh = tick < self.defs.ticks_per_second as u64 * 60;
        if self.settlement.tips_seen.is_none() {
            let seen = if fresh {
                Vec::new()
            } else {
                due.iter().map(|d| d.0.clone()).collect()
            };
            self.settlement.tips_seen = Some(seen);
        }
        let Some(seen) = &mut self.settlement.tips_seen else {
            return;
        };
        for (id, text) in due {
            if !seen.contains(&id) {
                seen.push(id);
                self.notices.push(tick, text);
            }
        }
    }

    /// Whether a tab or skill is shown yet. Scenarios show everything.
    pub fn is_open(&self, id: &str) -> bool {
        if self.journey.is_none() {
            return true;
        }
        let Some(tip) = self
            .defs
            .tips
            .iter()
            .find(|t| t.opens.as_deref() == Some(id))
        else {
            return true;
        };
        match &self.settlement.tips_seen {
            Some(seen) => seen.contains(&tip.id),
            None => self.tip_condition_met(&tip.when),
        }
    }
}
