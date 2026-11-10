//! Spirits who keep turning up in each other's moments come to keep each
//! other company. Like the moments, it changes nothing they do. It changes how
//! their stories read, and both endings remember it.

use crate::entities::{Spirit, SpiritId};
use crate::events::EventKind;
use crate::world::World;
use serde::Deserialize;

/// The rule for companions, and the words for them, from the data file.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct BondDefs {
    /// Moments two spirits must share before they keep company. 0 turns
    /// companions off.
    #[serde(default)]
    pub moments: u32,
    /// Said when two spirits start keeping company, with `{a}` and `{b}`.
    #[serde(default)]
    pub formed: String,
    /// A line in a spirit's story, with `{other}` and `{time}`.
    #[serde(default)]
    pub story: String,
    /// A line when the Hearth is lit, with `{a}` and `{b}`.
    #[serde(default)]
    pub hearth: String,
    /// A line when the Still Boat has gone, with `{a}` and `{b}`.
    #[serde(default)]
    pub boat: String,
}

/// Two companions, and when they became companions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bond {
    pub a: SpiritId,
    pub b: SpiritId,
    pub since: u64,
}

impl World {
    /// The spirit this one keeps company with, if it has one.
    pub fn companion_of(&self, s: &Spirit) -> Option<&Spirit> {
        let (other, _) = s.companion?;
        self.spirits.get(other)
    }

    /// Every pair of companions once, oldest first.
    pub fn bonds(&self) -> Vec<Bond> {
        let mut all: Vec<Bond> = self
            .spirits
            .iter()
            .filter_map(|s| s.companion.map(|(o, since)| (s.id, o, since)))
            .filter(|(a, b, _)| a < b)
            .map(|(a, b, since)| Bond { a, b, since })
            .collect();
        all.sort_by_key(|b| (b.since, b.a, b.b));
        all
    }

    /// Two spirits shared a moment. Enough of those, and they keep company.
    pub(crate) fn shared_moment(&mut self, a: SpiritId, b: SpiritId) {
        let (a, b) = (a.min(b), a.max(b));
        let pairs = &mut self.settlement.shared_moments;
        let count = match pairs.iter_mut().find(|p| p.0 == a && p.1 == b) {
            Some(p) => {
                p.2 += 1;
                p.2
            }
            None => {
                pairs.push((a, b, 1));
                1
            }
        };
        let need = self.defs.bonds.moments;
        if need > 0 && count >= need {
            self.form_bond(a, b);
        }
    }

    /// Makes two spirits companions, unless either already has one. Each keeps
    /// company with one other, so it means something.
    pub fn form_bond(&mut self, a: SpiritId, b: SpiritId) -> bool {
        let n = self.spirits.len();
        if a == b || a >= n || b >= n {
            return false;
        }
        if self.spirits[a].companion.is_some() || self.spirits[b].companion.is_some() {
            return false;
        }
        let tick = self.tick;
        self.spirits[a].companion = Some((b, tick));
        self.spirits[b].companion = Some((a, tick));
        let text = self.pair_text(&self.defs.bonds.formed, a, b);
        if !text.is_empty() {
            self.events.push(tick, EventKind::Spirit, text.clone());
            self.notices.push(tick, text);
        }
        true
    }

    fn pair_text(&self, template: &str, a: SpiritId, b: SpiritId) -> String {
        template
            .replace("{a}", &self.spirits[a].name)
            .replace("{b}", &self.spirits[b].name)
    }

    /// A line for a spirit's story, naming its companion as it's called now.
    pub(crate) fn bond_story_line(&self, s: &Spirit) -> Option<String> {
        let (_, since) = s.companion?;
        let other = self.companion_of(s)?;
        let template = &self.defs.bonds.story;
        (!template.is_empty()).then(|| {
            template
                .replace("{other}", &other.name)
                .replace("{time}", &self.clock_text(since))
        })
    }

    /// Who stood together when the Hearth was lit.
    pub(crate) fn hearth_bond_lines(&self) -> Vec<String> {
        let template = &self.defs.bonds.hearth;
        if template.is_empty() {
            return Vec::new();
        }
        let bonds = self.bonds().into_iter().take(3);
        bonds.map(|b| self.pair_text(template, b.a, b.b)).collect()
    }

    /// Who keeps company, for the notes.
    pub(crate) fn bond_notes(&self) -> Vec<String> {
        let template = &self.defs.bonds.formed;
        self.bonds()
            .into_iter()
            .map(|b| {
                let text = self.pair_text(template, b.a, b.b);
                format!("{}: {text}", self.clock_text(b.since))
            })
            .collect()
    }

    /// A tick as hours and minutes, as the stories show it.
    pub fn clock_text(&self, tick: u64) -> String {
        let secs = tick / self.defs.ticks_per_second as u64;
        format!("{}:{:02}", secs / 3600, secs / 60 % 60)
    }
}
