//! What makes one spirit different from another: a trait from the moment it
//! awakens, what it was made from, and the deeds it builds up over a game.
//! Deeds only record what happened, so nothing here changes what a spirit does.
//! A trait is the one exception, and it's fixed when the spirit is made.

use crate::entities::{Spirit, SpiritId};
use crate::events::EventKind;
use crate::world::World;
use serde::Deserialize;

/// A lasting difference, given to spirits made by Soul Separation.
#[derive(Clone, Debug, Deserialize)]
pub struct TraitDef {
    pub id: String,
    pub name: String,
    pub text: String,
    /// More this spirit can carry.
    #[serde(default)]
    pub carry: u32,
}

/// A milestone in a spirit's working life.
#[derive(Clone, Debug, Deserialize)]
pub struct DeedDef {
    pub id: String,
    /// One of `gathered`, `delivered`, `crafted` or `trips`.
    pub measure: String,
    pub amount: u32,
    /// What it's known as afterwards.
    pub title: String,
    pub text: String,
}

fn measured(s: &Spirit, measure: &str) -> u32 {
    match measure {
        "gathered" => s.units_gathered,
        "delivered" => s.units_delivered,
        "crafted" => s.units_crafted,
        "trips" => s.trips,
        _ => 0,
    }
}

impl World {
    /// Fixes a new spirit's trait, and notes where it came from.
    pub(crate) fn begin_story(&mut self, id: SpiritId, gift_xp: u64) {
        let count = self.defs.traits.len();
        let tick = self.tick;
        let s = &mut self.spirits[id];
        s.born = tick;
        s.gift_xp = gift_xp;
        if count == 0 {
            return;
        }
        // Spread by when it was made, so no spirit's trait can be chosen.
        let pick = (id as u64 * 7 + tick / 10) as usize % count;
        s.character = Some(pick as u16);
        s.actor.inventory.capacity += self.defs.traits[pick].carry;
    }

    /// Records deeds as they're earned.
    pub(crate) fn update_deeds(&mut self) {
        for id in 0..self.spirits.len() {
            for d in 0..self.defs.deeds.len() {
                let deed = &self.defs.deeds[d];
                let s = &self.spirits[id];
                let earned = s.deeds.iter().any(|(n, _)| *n as usize == d);
                if earned || measured(s, &deed.measure) < deed.amount {
                    continue;
                }
                let msg = format!(
                    "{} is now {} the {}. {}",
                    s.name, s.name, deed.title, deed.text
                );
                let tick = self.tick;
                self.spirits[id].deeds.push((d as u16, tick));
                self.events.push(tick, EventKind::Spirit, msg.clone());
                if self.journey.is_some() {
                    self.notices.push(tick, msg);
                }
            }
        }
    }

    /// A spirit's name with the title of its latest deed.
    pub fn spirit_title(&self, s: &Spirit) -> String {
        match s.deeds.last() {
            Some((d, _)) => {
                let title = &self.defs.deeds[*d as usize].title;
                format!("{} the {title}", s.name)
            }
            None => s.name.clone(),
        }
    }

    /// A spirit's life so far, a line at a time.
    pub fn spirit_story(&self, s: &Spirit) -> Vec<String> {
        let tps = self.defs.ticks_per_second as u64;
        let clock = |t: u64| {
            let secs = t / tps;
            format!("{}:{:02}", secs / 3600, secs / 60 % 60)
        };
        let mut lines = Vec::new();
        if s.gift_xp > 0 {
            let skill = &self.defs.skill(s.skill()).name;
            lines.push(format!(
                "Made at {} from {} of your {skill} XP.",
                clock(s.born),
                s.gift_xp
            ));
        } else {
            lines.push("It was here before you.".to_string());
        }
        if let Some(t) = s.character.and_then(|c| self.defs.traits.get(c as usize)) {
            lines.push(format!("{}: {}", t.name, t.text));
        }
        let mut work = Vec::new();
        for (n, what) in [
            (s.units_gathered, "gathered"),
            (s.units_crafted, "made"),
            (s.units_delivered, "delivered"),
        ] {
            if n > 0 {
                work.push(format!("{what} {n}"));
            }
        }
        if s.trips > 0 {
            work.push(format!("made {} trips", s.trips));
        }
        if !work.is_empty() {
            lines.push(format!("So far it has {}.", work.join(", ")));
        }
        for (d, at) in &s.deeds {
            let deed = &self.defs.deeds[*d as usize];
            lines.push(format!("{}: {}", clock(*at), deed.text));
        }
        lines
    }
}
