//! Small things spirits do, now and then, for no reason. They change nothing in
//! the world. The old stones say the helpers never looked up. These do, and one
//! you've named does it more often.

use crate::entities::{Spirit, SpiritId};
use crate::events::EventKind;
use crate::world::World;
use serde::Deserialize;

/// How far from the southern water a spirit can still be said to be at it.
const SHORE_ROW: i32 = 25;
/// How many more chances a named spirit gets than one you never named.
const NAMED_WEIGHT: usize = 3;

#[derive(Clone, Debug, Deserialize)]
pub struct MomentDef {
    pub id: String,
    pub text: String,
    /// Only a spirit with this trait.
    #[serde(default, rename = "trait")]
    pub character: Option<String>,
    /// Only in this season.
    #[serde(default)]
    pub season: Option<String>,
    /// Only a spirit of this skill.
    #[serde(default)]
    pub skill: Option<String>,
    /// Only a spirit made from the player's own XP.
    #[serde(default)]
    pub gifted: bool,
    /// Only a spirit the player has named.
    #[serde(default)]
    pub named: bool,
    /// Only near the southern water.
    #[serde(default)]
    pub shore: bool,
    /// Only a spirit that has been around this long.
    #[serde(default)]
    pub after_hours: u64,
}

/// A number that looks random, but is the same every time.
fn mix(n: u64) -> usize {
    (n.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 33) as usize
}

impl World {
    fn moment_fits(&self, s: &Spirit, m: &MomentDef) -> bool {
        let character = s
            .character
            .and_then(|c| self.defs.traits.get(c as usize))
            .map(|t| t.id.as_str());
        let skill = self.defs.skill(s.skill()).id.as_str();
        let season = self.season().map(|d| d.id.as_str());
        let hour = self.defs.ticks_per_second as u64 * 3600;
        m.character.as_deref().is_none_or(|c| Some(c) == character)
            && m.season.as_deref().is_none_or(|x| Some(x) == season)
            && m.skill.as_deref().is_none_or(|x| x == skill)
            && (!m.gifted || s.gift_xp > 0)
            && (!m.named || s.renamed)
            && (!m.shore || s.actor.pos().y >= SHORE_ROW)
            && self.tick >= s.born + m.after_hours * hour
    }

    /// Who gets a moment this time. Named spirits are more likely.
    fn moment_pool(&self) -> Vec<SpiritId> {
        let mut pool = Vec::new();
        for s in self.spirits.iter().filter(|s| !s.aboard) {
            let chances = if s.renamed { NAMED_WEIGHT } else { 1 };
            pool.extend(std::iter::repeat_n(s.id, chances));
        }
        pool
    }

    /// Now and then, one spirit does something small.
    pub(crate) fn update_moments(&mut self) {
        if self.journey.is_none() && !self.settlement.moments {
            return;
        }
        let period = self.defs.construction.moment_ticks;
        if period == 0 || !self.tick.is_multiple_of(period) {
            return;
        }
        let pool = self.moment_pool();
        if pool.is_empty() {
            return;
        }
        let turn = self.tick / period;
        let sid = pool[mix(turn) % pool.len()];
        let s = &self.spirits[sid];
        let fits: Vec<usize> = (0..self.defs.moments.len())
            .filter(|i| !s.moments.iter().any(|(m, _)| *m as usize == *i))
            .filter(|i| self.moment_fits(s, &self.defs.moments[*i]))
            .collect();
        if fits.is_empty() {
            return;
        }
        let pick = fits[mix(turn + 1) % fits.len()];
        let tick = self.tick;
        let text = self.moment_text(s, pick);
        self.spirits[sid].moments.push((pick as u16, tick));
        self.events.push(tick, EventKind::Spirit, text);
    }

    fn moment_text(&self, s: &Spirit, moment: usize) -> String {
        self.defs.moments[moment].text.replace("{name}", &s.name)
    }

    /// What a spirit is doing that's worth a second look, for a few seconds.
    pub fn spirit_moment(&self, s: &Spirit) -> Option<String> {
        let (moment, at) = s.moments.last()?;
        let shown = self.tick < at + self.defs.construction.moment_shown_ticks;
        shown.then(|| self.moment_text(s, *moment as usize))
    }

    /// The last few things this spirit did for no reason, newest last.
    pub(crate) fn recent_moments(&self, s: &Spirit) -> Vec<String> {
        let from = s.moments.len().saturating_sub(3);
        s.moments[from..]
            .iter()
            .map(|(m, _)| self.moment_text(s, *m as usize))
            .collect()
    }
}
