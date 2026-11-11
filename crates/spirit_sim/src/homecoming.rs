//! The other way to end. A Still Boat carries spirits over the water they won't
//! walk to, and everything the player gave them comes back. They don't.
//!
//! Spirits are only removed once every one of them is aboard, so nothing ever
//! has to cope with a gap in the list while the game is running.

use crate::entities::{Activity, SpiritId};
use crate::events::EventKind;
use crate::grid::TilePos;
use crate::tasks::{Goal, Task};
use crate::world::World;
use serde::{Deserialize, Serialize};

/// Someone who came back, kept for the closing story.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Returned {
    pub name: String,
    pub skill: String,
    pub xp: u64,
}

/// The spirits being called home, or already gone.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Homecoming {
    pub called: u64,
    pub last_aboard: u64,
    pub finished: Option<u64>,
    pub returned: Vec<Returned>,
    /// Whether the Hearth was already lit when they were called.
    pub hearth_stood: bool,
    /// Companions who crossed together, by name.
    #[serde(default)]
    pub together: Vec<(String, String)>,
}

impl World {
    /// Where the Still Boat is, once it's finished.
    pub fn boat(&self) -> Option<TilePos> {
        self.buildings
            .iter()
            .find(|b| self.defs.building(b.kind).calls_home && b.is_complete())
            .map(|b| b.pos)
    }

    pub fn can_call_home(&self) -> bool {
        self.boat().is_some() && self.settlement.homecoming.is_none() && !self.spirits.is_empty()
    }

    /// Whether every spirit has gone.
    pub fn homecoming_finished(&self) -> bool {
        let h = self.settlement.homecoming.as_ref();
        h.is_some_and(|h| h.finished.is_some())
    }

    /// Spirits aboard so far, of everyone called.
    pub fn aboard(&self) -> (usize, usize) {
        let on = self.spirits.iter().filter(|s| s.aboard).count();
        (on, self.spirits.len())
    }

    fn hearth_stands(&self) -> bool {
        self.buildings
            .iter()
            .any(|b| self.defs.building(b.kind).ending && b.is_complete())
    }

    pub(crate) fn call_home(&mut self) -> Result<String, String> {
        if !self.can_call_home() {
            return Err("There's nothing to call them home to".into());
        }
        self.settlement.homecoming = Some(Homecoming {
            called: self.tick,
            last_aboard: self.tick,
            finished: None,
            returned: Vec::new(),
            hearth_stood: self.hearth_stands(),
            together: Vec::new(),
        });
        let msg = "You call them home. One by one, they stop what they're doing.".to_string();
        self.events.push(self.tick, EventKind::Player, msg.clone());
        self.notices.push(self.tick, msg.clone());
        Ok(msg)
    }

    /// What a spirit was carrying stays where it put it down.
    fn drop_load(&mut self, sid: SpiritId) {
        let inventory = &mut self.spirits[sid].actor.inventory;
        let items: Vec<_> = inventory.items().iter().map(|(r, n)| (*r, *n)).collect();
        for (r, n) in items {
            inventory.remove(r, n);
            *self.ledger.left_behind.entry(r).or_default() += n as u64;
        }
    }

    /// Sends a spirit that has finished what it was doing to the boat.
    pub(crate) fn walk_home(&mut self, sid: SpiritId) -> bool {
        let called = self.settlement.homecoming.is_some() && !self.homecoming_finished();
        let Some(boat) = self.boat().filter(|_| called) else {
            return false;
        };
        self.drop_load(sid);
        let wait = self.tick + self.defs.idle_rethink_ticks;
        let s = &mut self.spirits[sid];
        s.next_think = wait;
        s.activity = Activity::Idle("Going home".into());
        if !s.actor.pos().is_adjacent(boat) {
            s.actor.queue.push_back(Task::Move {
                goal: Goal::Adjacent(boat),
                label: "the boat".into(),
            });
        }
        true
    }

    /// Takes spirits aboard one at a time, and ends it when nobody's left.
    pub(crate) fn update_homecoming(&mut self) {
        let Some(h) = &self.settlement.homecoming else {
            return;
        };
        if h.finished.is_some() {
            return;
        }
        let c = &self.defs.construction;
        let late = self.tick >= h.called + c.homecoming_limit_ticks;
        let due = late || self.tick >= h.last_aboard + c.homecoming_gap_ticks;
        let boat = self.boat();
        if due {
            // Once one of a pair is aboard, the boat waits for the other.
            let awaited = self
                .spirits
                .iter()
                .find(|s| !s.aboard && self.companion_of(s).is_some_and(|o| o.aboard))
                .map(|s| s.id);
            let ready = self
                .spirits
                .iter()
                .filter(|s| {
                    let there = boat.is_some_and(|b| s.actor.pos().is_adjacent(b));
                    !s.aboard && (late || (there && s.actor.queue.is_empty()))
                })
                .filter(|s| late || awaited.is_none_or(|a| a == s.id))
                .min_by_key(|s| (Some(s.id) != awaited, s.id))
                .map(|s| s.id);
            if let Some(sid) = ready {
                self.board(sid);
            }
        }
        if self.spirits.iter().all(|s| s.aboard) {
            self.finish_homecoming();
        }
    }

    fn board(&mut self, sid: SpiritId) {
        self.drop_load(sid);
        let title = self.spirit_title(&self.spirits[sid]);
        let s = &mut self.spirits[sid];
        s.aboard = true;
        s.actor.clear_tasks();
        s.activity = Activity::Idle("Aboard".into());
        let (skill, xp) = (s.skill(), s.gift_xp);
        let name = self.defs.skill(skill).name.clone();
        if xp > 0 {
            self.player_regained_xp(skill, xp);
        }
        let msg = if xp > 0 {
            format!("{title} has come home. {xp} {name} XP is yours again.")
        } else {
            format!("{title} was never yours to take back. It crosses anyway.")
        };
        self.events.push(self.tick, EventKind::Spirit, msg.clone());
        self.notices.push(self.tick, msg);
        let tick = self.tick;
        let together = self
            .companion_of(&self.spirits[sid])
            .filter(|o| o.aboard)
            .map(|o| (o.name.clone(), self.spirits[sid].name.clone()));
        if let Some(h) = &mut self.settlement.homecoming {
            h.together.extend(together);
            h.last_aboard = tick;
            h.returned.push(Returned {
                name: title,
                skill: name,
                xp,
            });
        }
    }

    fn finish_homecoming(&mut self) {
        for t in &mut self.trees {
            t.claimed_by = None;
        }
        for p in &mut self.projects {
            p.assigned = None;
        }
        self.spirits.clear();
        let tick = self.tick;
        if let Some(h) = &mut self.settlement.homecoming {
            h.finished = Some(tick);
        }
        self.settlement.ended_at.get_or_insert(tick);
        let msg =
            "The boat is gone, and the benches are quiet. Your story is in the Progress tab (J).";
        self.events.push(tick, EventKind::Player, msg.to_string());
        self.notices.push(tick, msg);
    }

    /// The closing account of a game that ended at the boat.
    pub(crate) fn homecoming_story(&self) -> Option<Vec<String>> {
        let h = self.settlement.homecoming.as_ref()?;
        let at = h.finished?;
        let secs = at / self.defs.ticks_per_second as u64;
        let total: u64 = h.returned.iter().map(|r| r.xp).sum();
        let count = h.returned.len();
        let who = if count == 1 { "spirit" } else { "spirits" };
        let mut lines = vec![
            format!(
                "You called them home after {} hours and {} minutes.",
                secs / 3600,
                secs / 60 % 60
            ),
            format!("{count} {who} crossed the water, and {total} XP came back to you."),
        ];
        for r in h.returned.iter().filter(|r| r.xp > 0).take(4) {
            lines.push(format!(
                "{}: {} {} XP, back where it began.",
                r.name, r.xp, r.skill
            ));
        }
        let template = &self.defs.bonds.boat;
        if !template.is_empty() {
            for (a, b) in h.together.iter().take(3) {
                lines.push(template.replace("{a}", a).replace("{b}", b));
            }
        }
        lines.push(if h.hearth_stood {
            "You lit them a hearth, and took them back anyway. It's still burning.".to_string()
        } else if self.hearth_stands() {
            "You lit the Hearth after they'd gone. Nobody stands by it.".to_string()
        } else {
            "There's no hearth here. Nobody was left who thought of it. But there's you."
                .to_string()
        });
        lines.extend(self.visitor_lines());
        lines.extend(self.building_lines(false));
        lines.extend(self.relic_line());
        lines.extend(self.last_helper_line(true));
        lines.extend(self.cold_camp_line());
        lines.extend(self.your_stone_line());
        if self.settlement.markers_read.len() == self.defs.markers.len() {
            lines.push(
                "Whoever cut those stones gave it all away, and never came back for it. You did."
                    .into(),
            );
        }
        lines.push(
            "The benches are quiet. The work goes on if you want it, and all of it is yours."
                .into(),
        );
        Some(lines)
    }
}
