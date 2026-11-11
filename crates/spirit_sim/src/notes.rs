//! What the player has been told, for when they've forgotten. A long game can't
//! rely on a line that fades after a few seconds.

use crate::world::World;

/// One part of the player's notes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoteSection {
    pub title: &'static str,
    pub lines: Vec<String>,
}

impl World {
    /// Everything worth looking up again, leaving out anything still empty.
    pub fn notes(&self) -> Vec<NoteSection> {
        let sections = [
            NoteSection {
                title: "What you've been told",
                lines: self.notes_told(),
            },
            NoteSection {
                title: "What the caravans found",
                lines: self.notes_found(),
            },
            NoteSection {
                title: "Who has come through",
                lines: self.notes_people(),
            },
            NoteSection {
                title: "The last helper",
                lines: self.last_helper_notes(),
            },
            NoteSection {
                title: "What you left last time",
                lines: self
                    .settlement
                    .left_before
                    .iter()
                    .filter(|s| s.found)
                    .flat_map(|s| s.lines.clone())
                    .collect(),
            },
            NoteSection {
                title: "Your stone",
                lines: self
                    .settlement
                    .your_stone
                    .as_ref()
                    .map(|s| s.lines.clone())
                    .unwrap_or_default(),
            },
            NoteSection {
                title: "Who keeps company",
                lines: self.bond_notes(),
            },
            NoteSection {
                title: "How the years have gone",
                lines: self.year_lines(),
            },
            NoteSection {
                title: "What you found over the water",
                lines: self
                    .relics_found()
                    .into_iter()
                    .map(|(title, text)| format!("{title}: {text}"))
                    .collect(),
            },
        ];
        sections
            .into_iter()
            .filter(|s| !s.lines.is_empty())
            .collect()
    }

    fn notes_told(&self) -> Vec<String> {
        let seen = self.settlement.tips_seen.iter().flatten();
        seen.filter_map(|id| self.defs.tips.iter().find(|t| &t.id == id))
            .map(|t| t.text.clone())
            .collect()
    }

    fn notes_found(&self) -> Vec<String> {
        let sent = self.settlement.caravans;
        let mut finds: Vec<_> = self
            .defs
            .buildings
            .iter()
            .filter(|b| b.needs_caravans > 0 && b.needs_caravans <= sent)
            .collect();
        finds.sort_by_key(|b| b.needs_caravans);
        finds.iter().map(|b| b.caravan_find.clone()).collect()
    }

    fn notes_people(&self) -> Vec<String> {
        let s = &self.settlement;
        let here = s.visitor.as_ref().map(|v| v.index as usize);
        let came = |i: usize| i < s.visitors_met as usize || s.visitors_seen.contains(&(i as u32));
        let visitors = self.defs.visitors.iter().enumerate();
        visitors
            .filter(|(i, _)| came(*i))
            .map(|(i, v)| {
                let answer = if here == Some(i) {
                    "waiting for an answer."
                } else if s.offers_accepted.contains(&v.id) {
                    "you took the offer."
                } else if s.offers_refused.contains(&v.id) {
                    "you turned them down."
                } else {
                    "they left without an answer."
                };
                format!("{}: {answer}", v.name)
            })
            .collect()
    }
}
