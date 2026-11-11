//! The old stones end: "We built no hearth. Nobody was left who thought of
//! it." Near the Mossed Stone there's a ring of stones where they camped, with
//! no ashes in it. Once your own Hearth is lit, you can carry its light there
//! and light the fire they never had. It changes nothing but the camp, and how
//! both endings read.

use crate::events::EventKind;
use crate::grid::TilePos;
use crate::world::World;
use serde::{Deserialize, Serialize};

/// Where it is and what's said, from the data file.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct ColdCampDefs {
    pub at: [i32; 2],
    #[serde(default)]
    pub name: String,
    /// On first finding it.
    #[serde(default)]
    pub found: String,
    /// On coming back to it with the Hearth lit.
    #[serde(default)]
    pub lit: String,
    /// In the closing words, once it's lit.
    #[serde(default)]
    pub ending: String,
}

/// When it was found, and when it was lit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColdCamp {
    pub found: Option<u64>,
    pub lit: Option<u64>,
}

impl World {
    /// Where the camp is, if this game has one.
    pub fn cold_camp_at(&self) -> Option<TilePos> {
        let d = &self.defs.cold_camp;
        (!d.name.is_empty()).then(|| TilePos::new(d.at[0], d.at[1]))
    }

    pub fn cold_camp_lit(&self) -> bool {
        self.settlement.cold_camp.lit.is_some()
    }

    /// Found when the player walks up to it, and lit when they come back to
    /// it with their own Hearth burning.
    pub(crate) fn update_cold_camp(&mut self) {
        let Some(at) = self.cold_camp_at() else {
            return;
        };
        if self.player.actor.pos().manhattan(at) > 1 {
            return;
        }
        let tick = self.tick;
        if self.settlement.cold_camp.found.is_none() {
            self.settlement.cold_camp.found = Some(tick);
            let msg = self.defs.cold_camp.found.clone();
            self.events.push(tick, EventKind::Player, msg.clone());
            self.notices.push(tick, msg);
        }
        if self.settlement.cold_camp.lit.is_none() && self.hearth().is_some() {
            self.settlement.cold_camp.lit = Some(tick);
            let msg = self.defs.cold_camp.lit.clone();
            self.events.push(tick, EventKind::Player, msg.clone());
            self.notices.push(tick, msg);
        }
    }

    /// A line for the closing words, once it's lit.
    pub(crate) fn cold_camp_line(&self) -> Option<String> {
        let ending = &self.defs.cold_camp.ending;
        (self.cold_camp_lit() && !ending.is_empty()).then(|| ending.clone())
    }

    /// What happened there, for the notes.
    pub(crate) fn cold_camp_notes(&self) -> Vec<String> {
        let d = &self.defs.cold_camp;
        let c = &self.settlement.cold_camp;
        let mut lines = Vec::new();
        if c.found.is_some() {
            lines.push(d.found.clone());
        }
        if c.lit.is_some() {
            lines.push(d.lit.clone());
        }
        lines
    }
}
