//! The old stones were left by someone who gave everything away. Once your
//! game has ended, you can leave a stone of your own, where you stand. Most of
//! what it says comes from what you did. The last line is yours to choose.

use crate::events::EventKind;
use crate::world::World;
use serde::{Deserialize, Serialize};

/// What a stone of your own can say, from the data file.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct YourStoneDefs {
    #[serde(default)]
    pub title: String,
    /// The first line after the Hearth, with `{xp}` and `{made}`.
    #[serde(default)]
    pub hearth: String,
    /// The first line after the Still Boat, with `{xp}` and `{made}`.
    #[serde(default)]
    pub boat: String,
    /// About a pair who kept company, with `{a}` and `{b}`.
    #[serde(default)]
    pub company: String,
    /// When every old stone has been read.
    #[serde(default)]
    pub stones: String,
    /// What's said as it's carved.
    #[serde(default)]
    pub carved: String,
    /// The last lines to choose from.
    #[serde(default)]
    pub choices: Vec<StoneChoice>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct StoneChoice {
    /// What the button says.
    pub label: String,
    pub text: String,
}

/// A stone the player carved, with its words as they were that day.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct YourStone {
    pub at: [i32; 2],
    pub carved: u64,
    pub choice: usize,
    pub lines: Vec<String>,
}

impl World {
    /// Whether there's a stone to carve now: the game has ended, by either
    /// ending, and you haven't carved one yet.
    pub fn can_carve_stone(&self) -> bool {
        let over = match &self.settlement.homecoming {
            Some(h) => h.finished.is_some(),
            None => self.settlement.ended_at.is_some(),
        };
        over && self.settlement.your_stone.is_none() && !self.defs.your_stone.choices.is_empty()
    }

    /// What a stone carved now would say, ending with `choice`.
    pub fn stone_words(&self, choice: usize) -> Option<Vec<String>> {
        let defs = &self.defs.your_stone;
        let last = defs.choices.get(choice)?;
        let mut lines = Vec::new();
        let (first, xp, made) = match &self.settlement.homecoming {
            Some(h) => {
                let gave = h.returned.iter().filter(|r| r.xp > 0);
                let xp: u64 = gave.clone().map(|r| r.xp).sum();
                (&defs.boat, xp, gave.count())
            }
            None => {
                let made = self.spirits.iter().filter(|s| s.gift_xp > 0);
                let xp: u64 = made.clone().map(|s| s.gift_xp).sum();
                (&defs.hearth, xp, made.count())
            }
        };
        lines.push(
            first
                .replace("{xp}", &xp.to_string())
                .replace("{made}", &made.to_string()),
        );
        let pair = match &self.settlement.homecoming {
            Some(h) => h.together.first().cloned(),
            None => self.bonds().first().map(|b| {
                let name = |i: usize| self.spirits[i].name.clone();
                (name(b.a), name(b.b))
            }),
        };
        if let Some((a, b)) = pair.filter(|_| !defs.company.is_empty()) {
            lines.push(defs.company.replace("{a}", &a).replace("{b}", &b));
        }
        let all_read = self.settlement.markers_read.len() == self.defs.markers.len();
        if all_read && !defs.stones.is_empty() {
            lines.push(defs.stones.clone());
        }
        lines.push(last.text.clone());
        Some(lines)
    }

    /// Carves a stone of your own where you stand. It can only be done once.
    pub(crate) fn carve_stone(&mut self, choice: usize) -> Result<String, String> {
        if self.settlement.your_stone.is_some() {
            return Err("You've already carved your stone".into());
        }
        if !self.can_carve_stone() {
            return Err("Not yet. There's nothing to carve until it's over".into());
        }
        let lines = self
            .stone_words(choice)
            .ok_or_else(|| "There's no such line to carve".to_string())?;
        let at = self.player.actor.pos();
        let tick = self.tick;
        self.settlement.your_stone = Some(YourStone {
            at: [at.x, at.y],
            carved: tick,
            choice,
            lines: lines.clone(),
        });
        let defs = &self.defs.your_stone;
        let msg = format!("{} {}: {}", defs.carved, defs.title, lines.join(" "));
        self.events.push(tick, EventKind::Player, msg.clone());
        self.notices.push(tick, msg.clone());
        Ok(msg)
    }

    /// A line for either ending's closing words, once there's a stone.
    pub(crate) fn your_stone_line(&self) -> Option<String> {
        self.settlement
            .your_stone
            .as_ref()
            .map(|_| "You left a stone of your own, for whoever comes next.".to_string())
    }
}
