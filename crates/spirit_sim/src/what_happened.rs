//! What happened in a game, for the play report. A playtest is the only way to
//! learn whether any of this works, and the report is what comes back from
//! one. Where the time went says how it played. This says how far someone
//! got, and what they did with the parts that are about meaning.

use crate::last_helper::HelperAnswer;
use crate::world::World;

impl World {
    /// The story so far, a line at a time, for the play report.
    pub fn what_happened(&self) -> Vec<String> {
        let mut lines = Vec::new();
        let clock = |tick: u64| self.clock_text(tick);

        let ambitions = self.ambitions();
        let done: Vec<_> = ambitions.iter().filter(|a| a.done).collect();
        lines.push(format!("Goals met: {} of {}.", done.len(), ambitions.len()));

        let made: Vec<_> = self.spirits.iter().filter(|s| s.gift_xp > 0).collect();
        if made.is_empty() {
            lines.push("Spirits made: none yet.".into());
        } else {
            let names: Vec<String> = made
                .iter()
                .map(|s| {
                    if s.renamed {
                        format!("{} (named by you)", s.name)
                    } else {
                        s.name.clone()
                    }
                })
                .collect();
            let first = made.iter().map(|s| s.born).min().unwrap_or(0);
            lines.push(format!(
                "Spirits made: {}, the first at {}: {}.",
                made.len(),
                clock(first),
                names.join(", ")
            ));
        }

        let bonds: Vec<String> = self
            .bonds()
            .iter()
            .map(|b| format!("{} and {}", self.spirits[b.a].name, self.spirits[b.b].name))
            .collect();
        if !bonds.is_empty() {
            lines.push(format!("Keeping company: {}.", bonds.join("; ")));
        }

        lines.push(format!(
            "Old stones read: {} of {}.",
            self.settlement.markers_read.len(),
            self.defs.markers.len()
        ));
        lines.push(format!(
            "Found on the far shore: {} of {}.",
            self.settlement.relics_found.len(),
            self.defs.relics.len()
        ));

        let helper = match (&self.settlement.last_helper, self.last_helper_seen()) {
            (Some(h), _) => match h.answer {
                HelperAnswer::Waiting => "met, and still waiting for an answer",
                HelperAnswer::Came => "asked home",
                HelperAnswer::Stayed => "left by its stone",
            },
            (None, true) => "seen, never met",
            (None, false) => "never seen",
        };
        lines.push(format!("The last helper: {helper}."));

        // A tally, not every name: the notes have the names.
        let people = self.notes_people();
        let count = |end: &str| people.iter().filter(|p| p.ends_with(end)).count();
        if people.is_empty() {
            lines.push("Visitors: none yet.".into());
        } else {
            lines.push(format!(
                "Visitors: {} came. You took {} offers and turned down {}. {} left without an answer, and {} waiting.",
                people.len(),
                count("you took the offer."),
                count("you turned them down."),
                count("they left without an answer."),
                count("waiting for an answer."),
            ));
        }

        match (self.settlement.ended_at, &self.settlement.homecoming) {
            (_, Some(h)) if h.finished.is_some() => lines.push(format!(
                "Ending: the Still Boat, called at {}. {} crossed.",
                clock(h.called),
                h.returned.len()
            )),
            (_, Some(h)) => lines.push(format!(
                "Ending: the Still Boat, called at {}, still boarding.",
                clock(h.called)
            )),
            (Some(at), None) => lines.push(format!("Ending: the Hearth, lit at {}.", clock(at))),
            (None, None) => lines.push("Ending: not yet.".into()),
        }
        if let Some(stone) = &self.settlement.your_stone {
            lines.push(format!(
                "Carved a stone at {}, ending: {}",
                clock(stone.carved),
                stone.lines.last().cloned().unwrap_or_default()
            ));
        }
        let left = &self.settlement.left_before;
        if !left.is_empty() {
            let found = left.iter().filter(|s| s.found).count();
            lines.push(format!(
                "Stones from earlier games: {} standing, {found} found.",
                left.len()
            ));
        }
        lines
    }
}
