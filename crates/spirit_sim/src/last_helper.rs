//! Whoever cut the old stones made helpers too, and gave everything away to
//! them. The Last Stone says its chisel marks are very neat, "the way a helper
//! would make them". One of those helpers is still out there by it, carving.
//! Once you've read that stone you can see it. Walk up, and it looks up at you.
//! You can ask it to come home with you, or leave it where it is. It does no
//! work either way: it's what the old stones' story left behind, and both
//! endings remember what you did.

use crate::events::EventKind;
use crate::grid::TilePos;
use crate::world::World;
use serde::{Deserialize, Serialize};

/// Where it stands and what's said, from the data file.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct LastHelperDefs {
    pub at: [i32; 2],
    /// How close the player has to come before it looks up. Close enough
    /// that reading its stone is enough: a player who reads the stone and
    /// walks away mustn't miss it.
    #[serde(default)]
    pub notices_within: u32,
    /// The old stone that has to be read before it can be seen.
    #[serde(default)]
    pub after_stone: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub found: String,
    #[serde(default)]
    pub question: String,
    /// The two answers, as the buttons say them.
    #[serde(default)]
    pub ask: String,
    #[serde(default)]
    pub leave: String,
    /// What happens after each answer.
    #[serde(default)]
    pub came: String,
    #[serde(default)]
    pub stayed: String,
    /// The closing words, by ending and answer.
    #[serde(default)]
    pub hearth_came: String,
    #[serde(default)]
    pub hearth_stayed: String,
    #[serde(default)]
    pub boat_came: String,
    #[serde(default)]
    pub boat_stayed: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum HelperAnswer {
    /// It has looked up, and is waiting for you to say something.
    Waiting,
    Came,
    Stayed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LastHelper {
    pub answer: HelperAnswer,
    pub since: u64,
    /// Its walk home, once asked. It's old, so it walks at half your pace.
    #[serde(default)]
    pub walker: Option<crate::entities::Mover>,
}

impl World {
    fn helper_answer(&self) -> Option<HelperAnswer> {
        self.settlement.last_helper.as_ref().map(|h| h.answer)
    }

    /// Whether it can be seen yet: the stone that points to it has been read.
    pub fn last_helper_seen(&self) -> bool {
        let stone = &self.defs.last_helper.after_stone;
        !stone.is_empty() && self.settlement.markers_read.contains(stone)
    }

    /// Where it stands now, if it can be seen: by its stone, or by your fire
    /// once it has come home. After the Still Boat, it's gone with the rest.
    pub fn last_helper_at(&self) -> Option<TilePos> {
        if !self.last_helper_seen() {
            return None;
        }
        let [x, y] = self.defs.last_helper.at;
        if self.helper_answer() != Some(HelperAnswer::Came) {
            return Some(TilePos::new(x, y));
        }
        let gone = self
            .settlement
            .homecoming
            .as_ref()
            .is_some_and(|h| h.finished.is_some());
        if gone {
            return None;
        }
        if let Some(m) = self
            .settlement
            .last_helper
            .as_ref()
            .and_then(|h| h.walker.as_ref())
        {
            return Some(m.pos);
        }
        let home = self.home_for_helper();
        Some(TilePos::new(home.x + 1, home.y + 1))
    }

    /// Where it's going once asked: the Hearth if it's lit, or Main Storage.
    fn home_for_helper(&self) -> TilePos {
        self.hearth().unwrap_or(self.main().pos)
    }

    /// Where to draw it, in tile units, between tiles while it walks.
    pub fn last_helper_visual(&self) -> Option<(f32, f32)> {
        let at = self.last_helper_at()?;
        let walker = self
            .settlement
            .last_helper
            .as_ref()
            .and_then(|h| h.walker.as_ref());
        match walker {
            Some(m) if m.pos == at => Some(m.visual_pos()),
            _ => Some((at.x as f32, at.y as f32)),
        }
    }

    /// Whether it's on its way home right now.
    pub fn last_helper_walking(&self) -> bool {
        let walker = self
            .settlement
            .last_helper
            .as_ref()
            .and_then(|h| h.walker.as_ref());
        walker.is_some_and(|m| !m.path.is_empty())
    }

    /// One tick of the walk home.
    pub(crate) fn step_last_helper(&mut self) {
        let Some(m) = self
            .settlement
            .last_helper
            .as_mut()
            .and_then(|h| h.walker.as_mut())
        else {
            return;
        };
        if m.path.is_empty() {
            return;
        }
        m.progress += 1;
        if m.progress >= m.ticks_per_tile {
            m.progress = 0;
            if let Some(next) = m.path.pop_front() {
                m.pos = next;
            }
        }
    }

    /// A walk from its stone to beside your fire, if there's a way. Without
    /// one, it's simply there, as before.
    fn walk_home_from(&self, from: TilePos) -> Option<crate::entities::Mover> {
        let home = self.home_for_helper();
        let path = self
            .grid
            .find_path(from, |p| p.manhattan(home) <= 2 && p != home)?;
        let mut m = crate::entities::Mover::new(from, self.defs.player.move_ticks.max(1) * 2);
        m.path = path.into_iter().collect();
        Some(m)
    }

    /// Whether it's carving: always, except while it's looking at you, and
    /// once it has come home, where it only watches.
    pub fn last_helper_carving(&self) -> bool {
        !matches!(
            self.helper_answer(),
            Some(HelperAnswer::Waiting | HelperAnswer::Came)
        )
    }

    /// What it's asking, while it waits for an answer.
    pub fn last_helper_question(&self) -> Option<String> {
        (self.helper_answer() == Some(HelperAnswer::Waiting))
            .then(|| self.defs.last_helper.question.clone())
    }

    /// It looks up when the player walks up to it.
    pub(crate) fn update_last_helper(&mut self) {
        if self.settlement.last_helper.is_some() || !self.last_helper_seen() {
            return;
        }
        let [x, y] = self.defs.last_helper.at;
        let within = self.defs.last_helper.notices_within.max(1);
        if self.player.actor.pos().manhattan(TilePos::new(x, y)) > within {
            return;
        }
        let tick = self.tick;
        self.settlement.last_helper = Some(LastHelper {
            answer: HelperAnswer::Waiting,
            since: tick,
            walker: None,
        });
        let msg = self.defs.last_helper.found.clone();
        self.events.push(tick, EventKind::Player, msg.clone());
        self.notices.push(tick, msg);
    }

    /// Asks it home, or leaves it be. Either is for good.
    pub(crate) fn answer_helper(&mut self, come: bool) -> Result<String, String> {
        if self.helper_answer() != Some(HelperAnswer::Waiting) {
            return Err("There's nobody waiting for an answer".into());
        }
        let tick = self.tick;
        let (answer, msg) = if come {
            (HelperAnswer::Came, self.defs.last_helper.came.clone())
        } else {
            (HelperAnswer::Stayed, self.defs.last_helper.stayed.clone())
        };
        let [x, y] = self.defs.last_helper.at;
        let walker = if come {
            self.walk_home_from(TilePos::new(x, y))
        } else {
            None
        };
        self.settlement.last_helper = Some(LastHelper {
            answer,
            since: tick,
            walker,
        });
        self.events.push(tick, EventKind::Player, msg.clone());
        self.notices.push(tick, msg.clone());
        Ok(msg)
    }

    /// A line for the closing words, once you've answered it.
    pub(crate) fn last_helper_line(&self, boat: bool) -> Option<String> {
        let d = &self.defs.last_helper;
        let line = match (self.helper_answer()?, boat) {
            (HelperAnswer::Came, false) => &d.hearth_came,
            (HelperAnswer::Stayed, false) => &d.hearth_stayed,
            (HelperAnswer::Came, true) => &d.boat_came,
            (HelperAnswer::Stayed, true) => &d.boat_stayed,
            (HelperAnswer::Waiting, _) => return None,
        };
        (!line.is_empty()).then(|| line.clone())
    }

    /// What happened, for the notes.
    pub(crate) fn last_helper_notes(&self) -> Vec<String> {
        let d = &self.defs.last_helper;
        match self.helper_answer() {
            Some(HelperAnswer::Came) => vec![d.found.clone(), d.came.clone()],
            Some(HelperAnswer::Stayed) => vec![d.found.clone(), d.stayed.clone()],
            Some(HelperAnswer::Waiting) => vec![d.found.clone()],
            None => Vec::new(),
        }
    }
}
