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
    /// Where stones from earlier games stand, most recent first.
    #[serde(default)]
    pub left_spots: Vec<[i32; 2]>,
    /// Said on finding a stone from an earlier game, before its words.
    #[serde(default)]
    pub found: String,
    /// A line for a new stone, once an old one of yours has been found.
    #[serde(default)]
    pub found_line: String,
    /// A line for the closing words, once an old one of yours has been found.
    #[serde(default)]
    pub found_ending: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct StoneChoice {
    /// What the button says.
    pub label: String,
    pub text: String,
}

/// A stone carved in an earlier game, standing in this one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeftStone {
    pub at: [i32; 2],
    pub lines: Vec<String>,
    pub found: bool,
}

/// The most lines a stone from an earlier game can bring, and how long each.
const MAX_LINES: usize = 6;
const MAX_CHARS: usize = 200;

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
                .replace("{made}", &made.to_string())
                .replace(
                    "{helpers}",
                    &if made == 1 {
                        "1 helper".to_string()
                    } else {
                        format!("{made} helpers")
                    },
                ),
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
        if self.found_one_from_before() && !defs.found_line.is_empty() {
            lines.push(defs.found_line.clone());
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

    /// Lines for either ending's closing words, about stones of your own.
    pub(crate) fn your_stone_line(&self) -> Vec<String> {
        let mut lines = Vec::new();
        let found = &self.defs.your_stone.found_ending;
        if self.found_one_from_before() && !found.is_empty() {
            lines.push(found.clone());
        }
        if self.settlement.your_stone.is_some() {
            lines.push("You left a stone of your own, for whoever comes next.".to_string());
        }
        lines
    }

    /// Stands stones carved in earlier games by the first old stone, most
    /// recent first, as many as there are places for. Only once, at the start.
    pub fn leave_from_before(&mut self, stones: Vec<Vec<String>>) {
        if !self.settlement.left_before.is_empty() {
            return;
        }
        let spots = self.defs.your_stone.left_spots.clone();
        for (at, lines) in spots.into_iter().zip(stones) {
            let lines: Vec<String> = lines
                .into_iter()
                .map(|l| l.trim().chars().take(MAX_CHARS).collect::<String>())
                .filter(|l| !l.is_empty())
                .take(MAX_LINES)
                .collect();
            if !lines.is_empty() {
                self.settlement.left_before.push(LeftStone {
                    at,
                    lines,
                    found: false,
                });
            }
        }
    }

    /// Whether the player has found a stone from an earlier game.
    pub fn found_one_from_before(&self) -> bool {
        self.settlement.left_before.iter().any(|s| s.found)
    }

    /// Finds a stone from an earlier game the player has walked up to.
    pub(crate) fn update_left_before(&mut self) {
        let at = self.player.actor.pos();
        let near = self.settlement.left_before.iter().position(|s| {
            let p = crate::grid::TilePos::new(s.at[0], s.at[1]);
            !s.found && p.manhattan(at) <= 1
        });
        let Some(i) = near else {
            return;
        };
        self.settlement.left_before[i].found = true;
        let words = self.settlement.left_before[i].lines.join(" ");
        let msg = format!("{} {words}", self.defs.your_stone.found);
        self.events.push(self.tick, EventKind::Player, msg.clone());
        self.notices.push(self.tick, msg);
    }
}
