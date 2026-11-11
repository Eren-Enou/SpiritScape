//! People who pass through a real game with an offer. Each comes once, and
//! leaves if nobody answers. Whoever is due first comes first, and anyone
//! waiting on something holds up nobody behind them. An offer is a choice with
//! a cost, so what they give should never be simply better than what they ask.

use crate::defs::{ResourceId, SkillId};
use crate::events::EventKind;
use crate::world::World;
use serde::{Deserialize, Serialize};

/// How many of the people who came through the closing story mentions.
const MAX_ENDING_LINES: usize = 3;

#[derive(Clone, Debug, Deserialize)]
pub struct AmountDef {
    pub resource: String,
    pub amount: u32,
}

/// Something the player picks up from someone who knows their trade.
#[derive(Clone, Debug, Deserialize)]
pub struct Lesson {
    pub skill: String,
    pub xp: u64,
}

/// Someone who passes through.
#[derive(Clone, Debug, Deserialize)]
pub struct VisitorDef {
    pub id: String,
    pub name: String,
    /// What they say.
    pub text: String,
    /// What they say if you take the offer.
    pub thanks: String,
    /// Game minutes before they can come.
    pub after_minutes: u64,
    /// A building that has to be finished first.
    #[serde(default)]
    pub needs_built: Option<String>,
    /// Only comes if the player took this visitor's offer.
    #[serde(default)]
    pub remembers: Option<String>,
    /// Only comes if the player turned this visitor down.
    #[serde(default)]
    pub refused: Option<String>,
    /// Comes either way, but has heard how this visitor got on.
    #[serde(default)]
    pub knows: Option<String>,
    /// What they say instead if that offer was taken.
    #[serde(default)]
    pub text_took: Option<String>,
    /// What they say instead if that offer was turned down.
    #[serde(default)]
    pub text_refused: Option<String>,
    /// What the player learns from them, if the offer's taken.
    #[serde(default)]
    pub teaches: Option<Lesson>,
    /// What the closing story says if the offer was taken.
    #[serde(default)]
    pub ending_took: Option<String>,
    /// What the closing story says if it was turned down.
    #[serde(default)]
    pub ending_refused: Option<String>,
    #[serde(default)]
    pub wants: Vec<AmountDef>,
    #[serde(default)]
    pub gives: Vec<AmountDef>,
}

/// Who's here at the moment.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VisitorHere {
    pub index: u32,
    pub arrived: u64,
}

/// A visitor as a player sees them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisitorView {
    pub name: String,
    pub text: String,
    pub offer: String,
    pub can_accept: bool,
    pub minutes_left: u64,
}

impl World {
    fn amounts(&self, list: &[AmountDef]) -> Vec<(ResourceId, u32)> {
        list.iter()
            .filter_map(|a| self.defs.resource_id(&a.resource).map(|r| (r, a.amount)))
            .collect()
    }

    fn amounts_text(&self, list: &[AmountDef]) -> String {
        let parts: Vec<String> = self
            .amounts(list)
            .iter()
            .map(|(r, n)| self.defs.amount(*r, *n))
            .collect();
        parts.join(" and ")
    }

    fn patience_ticks(&self) -> u64 {
        self.defs.construction.visitor_patience_minutes * self.defs.ticks_per_second as u64 * 60
    }

    /// Brings the next visitor, and sees off one who has waited too long.
    pub(crate) fn update_visitors(&mut self) {
        if self.journey.is_none() {
            return;
        }
        if let Some(here) = &self.settlement.visitor {
            if self.tick >= here.arrived + self.patience_ticks() {
                let name = self.defs.visitors[here.index as usize].name.clone();
                self.settlement.visitor = None;
                self.notices
                    .push(self.tick, format!("{name} has moved on."));
            }
            return;
        }
        let tpm = self.defs.ticks_per_second as u64 * 60;
        let gap = self.defs.construction.visitor_gap_minutes * tpm;
        if self
            .settlement
            .visitor_last
            .is_some_and(|t| self.tick < t + gap)
        {
            return;
        }
        let first = self.settlement.visitors_met as usize;
        let Some(next) = (first..self.defs.visitors.len()).find(|i| self.visitor_due(*i)) else {
            return;
        };
        let msg = format!(
            "{} has arrived, with an offer.",
            self.defs.visitors[next].name
        );
        self.settlement.visitor = Some(VisitorHere {
            index: next as u32,
            arrived: self.tick,
        });
        self.settlement.visitor_last = Some(self.tick);
        self.settlement.visitors_seen.push(next as u32);
        // Everyone before this count has been. Anyone past it is listed.
        while self
            .settlement
            .visitors_seen
            .contains(&self.settlement.visitors_met)
        {
            self.settlement.visitors_met += 1;
        }
        self.notices.push(self.tick, msg);
    }

    /// Whether this visitor could come right now.
    fn visitor_due(&self, index: usize) -> bool {
        let def = &self.defs.visitors[index];
        let s = &self.settlement;
        let tpm = self.defs.ticks_per_second as u64 * 60;
        let built = def.needs_built.as_ref().is_none_or(|id| {
            let kind = self.defs.building_kind_id(id);
            self.buildings
                .iter()
                .any(|b| Some(b.kind) == kind && b.is_complete())
        });
        self.tick >= def.after_minutes * tpm
            && built
            && !s.visitors_seen.contains(&(index as u32))
            && def
                .remembers
                .as_ref()
                .is_none_or(|id| s.offers_accepted.contains(id))
            && def
                .refused
                .as_ref()
                .is_none_or(|id| s.offers_refused.contains(id))
    }

    fn lesson_of(&self, def: &VisitorDef) -> Option<(SkillId, u64)> {
        let lesson = def.teaches.as_ref()?;
        Some((self.defs.skill_id(&lesson.skill)?, lesson.xp))
    }

    /// What's left of the people who came through. Only the latest few: a
    /// closing story isn't a ledger.
    pub(crate) fn visitor_lines(&self) -> Vec<String> {
        let s = &self.settlement;
        let lines: Vec<String> = self
            .defs
            .visitors
            .iter()
            .filter_map(|v| {
                if s.offers_accepted.contains(&v.id) {
                    v.ending_took.clone()
                } else if s.offers_refused.contains(&v.id) {
                    v.ending_refused.clone()
                } else {
                    None
                }
            })
            .collect();
        let from = lines.len().saturating_sub(MAX_ENDING_LINES);
        lines[from..].to_vec()
    }

    /// What a visitor says, given what they've heard.
    fn visitor_text(&self, def: &VisitorDef) -> String {
        let s = &self.settlement;
        let heard = def.knows.as_ref().and_then(|id| {
            if s.offers_accepted.contains(id) {
                def.text_took.clone()
            } else if s.offers_refused.contains(id) {
                def.text_refused.clone()
            } else {
                None
            }
        });
        heard.unwrap_or_else(|| def.text.clone())
    }

    /// Who's here, and what they're offering.
    pub fn visitor(&self) -> Option<VisitorView> {
        let here = self.settlement.visitor.as_ref()?;
        let def = &self.defs.visitors[here.index as usize];
        let main = &self.main().storage;
        let can_accept = self
            .amounts(&def.wants)
            .iter()
            .all(|(r, n)| main.available(*r) >= *n);
        let wants = self.amounts_text(&def.wants);
        let gives = self.amounts_text(&def.gives);
        let offer = match (wants.is_empty(), gives.is_empty()) {
            (false, false) => format!("Give {wants} for {gives}."),
            (false, true) => format!("Give {wants}."),
            _ => format!("Accept {gives}."),
        };
        let tpm = self.defs.ticks_per_second as u64 * 60;
        let left = (here.arrived + self.patience_ticks()).saturating_sub(self.tick);
        Some(VisitorView {
            name: def.name.clone(),
            text: self.visitor_text(def),
            offer: match self.lesson_of(def) {
                Some((skill, _)) => {
                    let name = &self.defs.skill(skill).name;
                    format!("{offer} You'd learn something of {name}, too.")
                }
                None => offer,
            },
            can_accept,
            minutes_left: left.div_ceil(tpm),
        })
    }

    /// Takes or turns down what's on offer.
    pub(crate) fn answer_visitor(&mut self, accept: bool) -> Result<String, String> {
        let here = self
            .settlement
            .visitor
            .clone()
            .ok_or("There's nobody here")?;
        let def = self.defs.visitors[here.index as usize].clone();
        if !accept {
            self.settlement.visitor = None;
            self.settlement.offers_refused.push(def.id);
            return Ok(format!("{} nods, and moves on.", def.name));
        }
        let wants = self.amounts(&def.wants);
        let main = self.main_storage;
        if wants
            .iter()
            .any(|(r, n)| self.buildings[main].storage.available(*r) < *n)
        {
            return Err("Main Storage doesn't have that".into());
        }
        for (r, n) in wants {
            self.buildings[main].storage.withdraw_available(r, n);
            *self.ledger.consumed.entry(r).or_default() += n as u64;
        }
        for (r, n) in self.amounts(&def.gives) {
            let put = self.buildings[main].storage.deposit(r, n);
            *self.ledger.seeded.entry(r).or_default() += put as u64;
        }
        self.settlement.visitor = None;
        self.settlement.offers_taken += 1;
        self.settlement.offers_accepted.push(def.id.clone());
        if let Some((skill, xp)) = self.lesson_of(&def) {
            self.player_earned_xp(skill, xp);
            let name = &self.defs.skill(skill).name;
            let msg = format!("You picked up a thing or two: {xp} {name} XP.");
            self.notices.push(self.tick, msg);
        }
        self.events
            .push(self.tick, EventKind::Player, def.thanks.clone());
        self.notices.push(self.tick, def.thanks.clone());
        Ok(def.thanks)
    }
}
