//! What makes one spirit different from another: a trait from the moment it
//! awakens, what it was made from, and the deeds it builds up over a game.
//! Deeds only record what happened, so nothing here changes what a spirit does.
//! A trait is the one exception, and it's fixed when the spirit is made.

use crate::entities::{Activity, Spirit, SpiritId, TreeState};
use crate::events::EventKind;
use crate::grid::TilePos;
use crate::tasks::{Goal, Task};
use crate::world::World;
use serde::{Deserialize, Serialize};

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

/// A running total, kept at the end of each year so the notes can show how
/// much each year added.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct YearEnd {
    pub gathered: u64,
    pub made: u64,
    pub spirits: u32,
    pub built: u32,
}

/// Something left on the far shore by whoever was here before.
#[derive(Clone, Debug, Deserialize)]
pub struct RelicDef {
    pub id: String,
    pub at: [i32; 2],
    pub title: String,
    pub text: String,
    /// What the player picks up from it, if anything.
    #[serde(default)]
    pub teaches: Option<crate::visitors::Lesson>,
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

/// The longest name a spirit can have.
pub const MAX_NAME: usize = 18;

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
        let pick = id % count;
        s.character = Some(pick as u16);
        s.actor.inventory.capacity += self.defs.traits[pick].carry;
    }

    /// The season now, in a playthrough. It comes from the clock alone, so
    /// there's nothing to save.
    pub fn season(&self) -> Option<&SeasonDef> {
        // Fast development games would spin through the year in minutes.
        if self.journey.is_none() && !self.settlement.seasons {
            return None;
        }
        // A scenario that asks for seasons gets them, whatever its pace.
        if self.defs.profile.id == "development" && !self.settlement.seasons {
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

    /// What a new season brings, besides the weather.
    fn season_turned(&mut self, name: &str, growth: u64) {
        if self.defs.seasons.first().is_some_and(|s| s.id == name) {
            let end = self.year_end();
            self.settlement.years.push(end);
            let msg = "Another year. Your notes (L) say how the last one went.";
            self.notices.push(self.tick, msg);
        }
        if growth == 0 && self.hearth().is_some() && !self.spirits.is_empty() {
            let msg = "Midwinter. They've stopped what they were doing, and gone to the Hearth.";
            self.notices.push(self.tick, msg);
        }
    }

    /// The totals so far.
    fn year_end(&self) -> YearEnd {
        let built = self.buildings.iter().filter(|b| b.is_complete()).count();
        YearEnd {
            gathered: self.ledger.harvested.values().sum(),
            made: self.ledger.crafted.values().sum(),
            spirits: self.spirits.len() as u32,
            built: built as u32,
        }
    }

    /// Each year so far, as a line for the notes.
    pub(crate) fn year_lines(&self) -> Vec<String> {
        let mut before = YearEnd::default();
        let years = self.settlement.years.iter().enumerate();
        let plural = |n: u32| if n == 1 { "" } else { "s" };
        years
            .map(|(i, end)| {
                let gathered = end.gathered.saturating_sub(before.gathered);
                let made = end.made.saturating_sub(before.made);
                let spirits = end.spirits.saturating_sub(before.spirits);
                let built = end.built.saturating_sub(before.built);
                before = *end;
                format!(
                    "Year {}: {gathered} gathered, {made} made, {spirits} new spirit{}, {built} thing{} built.",
                    i + 1,
                    plural(spirits),
                    plural(built),
                )
            })
            .collect()
    }

    /// Whether this is the first of winter, with the Hearth lit.
    fn midwinter(&self) -> bool {
        let tpm = self.defs.ticks_per_second as u64 * 60;
        let Some(season) = self.season().filter(|s| s.growth == 0) else {
            return false;
        };
        let passed = (season.minutes * tpm).saturating_sub(self.season_left());
        self.hearth().is_some() && passed < self.defs.construction.midwinter_ticks
    }

    /// Ticks left in the season it is now.
    fn season_left(&self) -> u64 {
        let tpm = self.defs.ticks_per_second as u64 * 60;
        let year: u64 = self.defs.seasons.iter().map(|s| s.minutes * tpm).sum();
        if year == 0 {
            return 0;
        }
        let mut left = self.tick % year;
        for s in &self.defs.seasons {
            let length = s.minutes * tpm;
            if left < length {
                return length - left;
            }
            left -= length;
        }
        0
    }

    /// The south river freezes in winter. Going out on it is up to the player.
    fn update_ice(&mut self, winter: bool) {
        let area = self.defs.construction.ice_area;
        let frozen = self.grid.frozen_area().is_some();
        if winter && !frozen {
            self.grid.freeze(Some(area));
        } else if !winter && frozen {
            self.grid.freeze(None);
            self.settlement.ice_warned = false;
            self.caught_by_the_thaw();
        }
        let far_out = self.player.actor.pos().y >= area[1];
        let thin = self.season_left() < self.defs.construction.thaw_warning_ticks;
        if winter && thin && far_out && !self.settlement.ice_warned {
            self.settlement.ice_warned = true;
            let msg = "The ice is groaning. Spring is close, and you're a long way from home.";
            self.notices.push(self.tick, msg);
        }
    }

    /// Anyone still out there when the ice goes gets home, but only just.
    fn caught_by_the_thaw(&mut self) {
        let land = self.grid.mainland();
        let at = self.player.actor.pos();
        if self.grid.on_land(&land, at) {
            return;
        }
        let Some(to) = self.grid.nearest_land(&land, at) else {
            return;
        };
        let lost: Vec<_> = self
            .player
            .actor
            .inventory
            .items()
            .iter()
            .map(|(r, n)| (*r, *n))
            .collect();
        for (r, n) in &lost {
            self.player.actor.inventory.remove(*r, *n);
            *self.ledger.left_behind.entry(*r).or_default() += *n as u64;
        }
        self.end_session(crate::session::SessionEnd::Player, String::new());
        let actor = &mut self.player.actor;
        actor.clear_tasks();
        actor.mover.pos = to;
        self.player.activity = "Idle".into();
        let msg = if lost.is_empty() {
            "The ice went out, and you only just got back."
        } else {
            "The ice went out. You got back, but what you were carrying didn't."
        };
        self.events
            .push(self.tick, EventKind::Player, msg.to_string());
        self.notices.push(self.tick, msg);
    }

    /// Whether something standing keeps winter off.
    fn walls_are_warm(&self) -> bool {
        self.buildings
            .iter()
            .any(|b| b.is_complete() && self.defs.building(b.kind).winter_growth)
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
                self.season_turned(&name, growth);
            }
            self.settlement.season_seen = Some(name);
        }
        self.update_ice(growth == 0);
        if growth == 1 {
            return;
        }
        let kept_warm = growth == 0 && self.tick.is_multiple_of(2) && self.walls_are_warm();
        for t in &mut self.trees {
            if !self.defs.trees[t.kind.index()].seasonal {
                continue;
            }
            if let TreeState::Depleted { regrow_at } = &mut t.state {
                // Each tick already counts for one.
                *regrow_at = if kept_warm {
                    *regrow_at
                } else if growth == 0 {
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

    /// Finds whatever the player has walked up to on the far shore.
    pub(crate) fn update_relics(&mut self) {
        let at = self.player.actor.pos();
        let near = self.defs.relics.iter().find(|r| {
            let p = TilePos::new(r.at[0], r.at[1]);
            p.manhattan(at) <= 1 && !self.settlement.relics_found.contains(&r.id)
        });
        let Some(r) = near.cloned() else {
            return;
        };
        self.settlement.relics_found.push(r.id);
        let msg = format!("{}: {}", r.title, r.text);
        self.events.push(self.tick, EventKind::Player, msg.clone());
        self.notices.push(self.tick, msg);
        let lesson = r.teaches.as_ref();
        if let Some((skill, xp)) = lesson.and_then(|l| Some((self.defs.skill_id(&l.skill)?, l.xp)))
        {
            self.player_earned_xp(skill, xp);
        }
    }

    /// What's been found over the water, as title and text.
    pub fn relics_found(&self) -> Vec<(String, String)> {
        let found = &self.settlement.relics_found;
        let all = self.defs.relics.iter();
        all.filter(|r| found.contains(&r.id))
            .map(|r| (r.title.clone(), r.text.clone()))
            .collect()
    }

    /// What the closing story says about what was left on the far shore.
    pub(crate) fn relic_line(&self) -> Option<String> {
        let total = self.defs.relics.len();
        (total > 0 && self.settlement.relics_found.len() == total)
            .then(|| "You found what they left on the far shore, and you carried it home.".into())
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

    /// What the next spirit made will be like. Traits come round in order, so
    /// a player can see it coming and decide what to make of it.
    pub fn next_trait(&self) -> Option<&TraitDef> {
        let count = self.defs.traits.len();
        (count > 0).then(|| &self.defs.traits[self.spirits.len() % count])
    }

    /// Gives a spirit the name the player chose.
    pub(crate) fn rename_spirit(&mut self, id: SpiritId, name: &str) -> Result<String, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("A name needs at least one letter".into());
        }
        if name.chars().count() > MAX_NAME {
            return Err(format!("Names can be {MAX_NAME} letters at most"));
        }
        let Some(s) = self.spirits.get_mut(id) else {
            return Err("There's no such spirit".into());
        };
        let msg = format!("{} is now called {name}", s.name);
        s.name = name.to_string();
        s.renamed = true;
        self.events.push(self.tick, EventKind::Player, msg.clone());
        Ok(msg)
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

    /// What a spirit standing by the Hearth is doing.
    pub const AT_THE_HEARTH: &'static str = "At the Hearth";

    /// Whether spirits are gathering at the Hearth right now.
    pub fn hearth_gathering(&self) -> bool {
        let length = self.defs.construction.hearth_gathering_ticks;
        let just_lit = self
            .settlement
            .ended_at
            .is_some_and(|at| self.tick < at + length);
        just_lit || self.midwinter()
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
        s.activity = Activity::Idle(Self::AT_THE_HEARTH.into());
        if !s.actor.pos().is_adjacent(hearth) {
            s.actor.queue.push_back(Task::Move {
                goal: Goal::Adjacent(hearth),
                label: "the Hearth".into(),
            });
        }
        true
    }

    /// What the closing story says about what's standing. The boat only has
    /// something to say if it's still at the shore.
    pub(crate) fn building_lines(&self, boat: bool) -> Vec<String> {
        let stands = |i: usize| {
            self.buildings
                .iter()
                .any(|b| b.kind.index() == i && b.is_complete())
        };
        let lines = self.defs.buildings.iter().enumerate();
        lines
            .filter(|(i, def)| (boat || !def.calls_home) && stands(*i))
            .filter_map(|(_, def)| def.ending_line.clone())
            .collect()
    }

    /// The closing account of a game, once the Hearth is lit.
    pub fn ending_story(&self) -> Option<Vec<String>> {
        if let Some(story) = self.homecoming_story() {
            return Some(story);
        }
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
                "You gave {given} XP of yourself to make {}, and it's still here, in {}.",
                match made.len() {
                    1 => "1 spirit".to_string(),
                    n => format!("{n} spirits"),
                },
                if made.len() == 1 { "it" } else { "them" },
            ),
        ];
        for s in self.spirits.iter().filter(|s| !s.deeds.is_empty()).take(4) {
            let last = s.deeds.last().map(|(d, _)| &self.defs.deeds[*d as usize]);
            if let Some(deed) = last {
                lines.push(format!("{}: {}", self.spirit_title(s), deed.text));
            }
        }
        lines.extend(self.hearth_bond_lines());
        lines.push(format!(
            "{} Trade Caravans went out, and the settlement knows your name.",
            self.settlement.caravans
        ));
        lines.extend(self.visitor_lines());
        lines.extend(self.building_lines(true));
        lines.extend(self.relic_line());
        lines.extend(self.last_helper_line(false));
        lines.extend(self.cold_camp_line());
        lines.extend(self.your_stone_line());
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
        lines.extend(self.bond_story_line(s));
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
        lines.extend(self.recent_moments(s));
        lines
    }
}
