//! What makes one spirit different from another: a trait from the moment it
//! awakens, what it was made from, and the deeds it builds up over a game.
//! Deeds only record what happened, so nothing here changes what a spirit does.
//! A trait is the one exception, and it's fixed when the spirit is made.

use crate::entities::{Activity, Spirit, SpiritId, TreeState};
use crate::events::EventKind;
use crate::grid::TilePos;
use crate::tasks::{Goal, Task};
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

/// A part of the year.
#[derive(Clone, Debug, Deserialize)]
pub struct SeasonDef {
    pub id: String,
    pub name: String,
    pub minutes: u64,
    /// Ticks of regrowth a seasonal tree gets each tick.
    pub growth: u64,
    pub text: String,
}

/// An old stone, read by walking up to it.
#[derive(Clone, Debug, Deserialize)]
pub struct MarkerDef {
    pub id: String,
    pub at: [i32; 2],
    pub title: String,
    pub text: String,
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

    /// The season now, in a playthrough. It comes from the clock alone, so
    /// there's nothing to save.
    pub fn season(&self) -> Option<&SeasonDef> {
        // Fast development games would spin through the year in minutes.
        self.journey.as_ref()?;
        if self.defs.profile.id == "development" {
            return None;
        }
        let tpm = self.defs.ticks_per_second as u64 * 60;
        let year: u64 = self.defs.seasons.iter().map(|s| s.minutes * tpm).sum();
        if year == 0 {
            return None;
        }
        let mut left = self.tick % year;
        for s in &self.defs.seasons {
            let length = s.minutes * tpm;
            if left < length {
                return Some(s);
            }
            left -= length;
        }
        None
    }

    /// Moves seasonal regrowth along, and says when the season turns.
    pub(crate) fn update_season(&mut self) {
        let Some(s) = self.season() else {
            return;
        };
        let (growth, name, text) = (s.growth, s.id.clone(), s.text.clone());
        if self.settlement.season_seen.as_deref() != Some(name.as_str()) {
            if self.settlement.season_seen.is_some() {
                self.notices.push(self.tick, text);
            }
            self.settlement.season_seen = Some(name);
        }
        if growth == 1 {
            return;
        }
        for t in &mut self.trees {
            if !self.defs.trees[t.kind.index()].seasonal {
                continue;
            }
            if let TreeState::Depleted { regrow_at } = &mut t.state {
                // Each tick already counts for one.
                *regrow_at = if growth == 0 {
                    *regrow_at + 1
                } else {
                    regrow_at.saturating_sub(growth - 1)
                };
            }
        }
    }

    /// Reads a stone the player has walked up to.
    pub(crate) fn update_markers(&mut self) {
        let at = self.player.actor.pos();
        let near = self.defs.markers.iter().find(|m| {
            let p = TilePos::new(m.at[0], m.at[1]);
            p.manhattan(at) <= 1 && !self.settlement.markers_read.contains(&m.id)
        });
        let Some(m) = near.cloned() else {
            return;
        };
        self.settlement.markers_read.push(m.id);
        let msg = format!("{}: {}", m.title, m.text);
        self.events.push(self.tick, EventKind::Player, msg.clone());
        self.notices.push(self.tick, msg);
    }

    /// The stones read so far, as title and text.
    pub fn markers_read(&self) -> Vec<(String, String)> {
        let read = &self.settlement.markers_read;
        self.defs
            .markers
            .iter()
            .filter(|m| read.contains(&m.id))
            .map(|m| (m.title.clone(), m.text.clone()))
            .collect()
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

    /// Where the Hearth stands, once it's lit.
    pub fn hearth(&self) -> Option<TilePos> {
        self.settlement.ended_at?;
        self.buildings
            .iter()
            .find(|b| self.defs.building(b.kind).ending && b.is_complete())
            .map(|b| b.pos)
    }

    /// Whether spirits are gathering at the Hearth right now.
    pub fn hearth_gathering(&self) -> bool {
        let length = self.defs.construction.hearth_gathering_ticks;
        self.settlement
            .ended_at
            .is_some_and(|at| self.tick < at + length)
    }

    /// When the Hearth is lit, every spirit finishes what it's doing and comes
    /// to stand by it for a while. Then they go back to work.
    pub(crate) fn gather_at_hearth(&mut self, sid: SpiritId) -> bool {
        if !self.hearth_gathering() || self.spirits[sid].actor.inventory.total() > 0 {
            return false;
        }
        let Some(hearth) = self.hearth() else {
            return false;
        };
        let wait = self.tick + self.defs.idle_rethink_ticks;
        let s = &mut self.spirits[sid];
        s.next_think = wait;
        s.activity = Activity::Idle("At the Hearth".into());
        if !s.actor.pos().is_adjacent(hearth) {
            s.actor.queue.push_back(Task::Move {
                goal: Goal::Adjacent(hearth),
                label: "the Hearth".into(),
            });
        }
        true
    }

    /// The closing account of a game, once the Hearth is lit.
    pub fn ending_story(&self) -> Option<Vec<String>> {
        let at = self.settlement.ended_at?;
        let tps = self.defs.ticks_per_second as u64;
        let secs = at / tps;
        let made: Vec<&Spirit> = self.spirits.iter().filter(|s| s.gift_xp > 0).collect();
        let given: u64 = made.iter().map(|s| s.gift_xp).sum();
        let mut lines = vec![
            format!(
                "The Hearth of Spirits was lit after {} hours and {} minutes.",
                secs / 3600,
                secs / 60 % 60
            ),
            format!(
                "You gave {given} XP of yourself to make {} spirits, and it's still here, in them.",
                made.len()
            ),
        ];
        for s in self.spirits.iter().filter(|s| !s.deeds.is_empty()).take(6) {
            let last = s.deeds.last().map(|(d, _)| &self.defs.deeds[*d as usize]);
            if let Some(deed) = last {
                lines.push(format!("{}: {}", self.spirit_title(s), deed.text));
            }
        }
        lines.push(format!(
            "{} Trade Caravans went out, and the settlement knows your name.",
            self.settlement.caravans
        ));
        if self.settlement.markers_read.len() == self.defs.markers.len() {
            lines.push("Whoever cut those stones built no hearth. You did.".into());
        }
        lines.push("There's nothing left you have to do. The work goes on if you want it.".into());
        Some(lines)
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
