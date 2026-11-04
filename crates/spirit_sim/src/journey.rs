//! Light guidance through the first progression cycle of a real playthrough.
//!
//! This is not a quest system. It watches the world, says what would make
//! sense to do next and records when each milestone happened. It blocks
//! nothing and rewards nothing: a player who does things in another order just
//! finds the guidance has moved on without them.

use crate::defs::{JobKind, SkillId};
use crate::events::EventKind;
use crate::world::World;
use serde::{Deserialize, Serialize};

/// The stages of the first cycle, in their intended order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Phase {
    /// Move, gather, deposit, earn XP.
    #[default]
    Beginnings,
    /// Create a Woodcutting Spirit.
    FirstSpirit,
    /// Train Woodworking by hand.
    FirstCraft,
    /// Create a Woodworking Spirit and see planks made.
    Automation,
    /// Spend planks on a settlement improvement.
    Improvement,
    /// Regain a sacrificed level, with the settlement's help.
    Return,
    /// The first cycle is done; the player sets their own goals.
    Settled,
}

/// Moments worth timing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Milestone {
    FirstLevel,
    FirstUnlock,
    FirstSpirit,
    FirstPlank,
    CraftUnlock,
    CraftSpirit,
    ChainPlank,
    FirstImprovement,
    LevelRegained,
}

impl Milestone {
    pub fn label(self) -> &'static str {
        match self {
            Milestone::FirstLevel => "First level gained",
            Milestone::FirstUnlock => "First Woodcutting unlock",
            Milestone::FirstSpirit => "First spirit created",
            Milestone::FirstPlank => "First plank made by hand",
            Milestone::CraftUnlock => "First Woodworking unlock",
            Milestone::CraftSpirit => "Woodworking Spirit created",
            Milestone::ChainPlank => "First plank made by a spirit",
            Milestone::FirstImprovement => "First settlement improvement",
            Milestone::LevelRegained => "Sacrificed level regained",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Journey {
    pub phase: Phase,
    /// Each milestone reached and its tick, in the order they happened.
    pub reached: Vec<(Milestone, u64)>,
}

impl Journey {
    pub fn tick_of(&self, m: Milestone) -> Option<u64> {
        self.reached.iter().find(|(r, _)| *r == m).map(|(_, t)| *t)
    }
}

/// What the guidance says right now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Guidance {
    pub title: String,
    /// The next sensible step, in one sentence.
    pub hint: String,
    /// Progress toward the current goal, when it has a number.
    pub progress: Option<(u64, u64, String)>,
}

impl World {
    fn skill_named(&self, id: &str) -> Option<SkillId> {
        self.defs.skill_id(id)
    }

    fn spirits_from(&self, skill: Option<SkillId>) -> u32 {
        skill.map_or(0, |s| self.player.history(s).spirits_created)
    }

    fn level_of(&self, skill: Option<SkillId>) -> u32 {
        skill.map_or(1, |s| self.player.level(s, &self.defs.xp_table))
    }

    /// A player-ordered building has been finished.
    fn improved(&self) -> bool {
        self.projects
            .iter()
            .any(|p| !p.is_active() && self.defs.building(p.kind).capacity_bonus > 0)
    }

    /// Some skill has been sacrificed and is back at its best.
    fn level_regained(&self) -> bool {
        (0..self.defs.skills.len())
            .map(|i| SkillId(i as u16))
            .any(|s| {
                let h = self.player.history(s);
                h.spirits_created > 0
                    && self.player.level(s, &self.defs.xp_table) >= h.highest_level
            })
    }

    /// Whether `phase` is finished. Checked in order, so stages a player did
    /// early pass straight through.
    fn phase_done(&self, phase: Phase) -> bool {
        let wc = self.skill_named("woodcutting");
        let ww = self.skill_named("woodworking");
        match phase {
            Phase::Beginnings => {
                self.level_of(wc) >= self.defs.journey.first_spirit_level
                    || self.spirits_from(wc) > 0
            }
            Phase::FirstSpirit => self.spirits_from(wc) > 0,
            Phase::FirstCraft => {
                self.level_of(ww) >= self.defs.journey.woodworking_level
                    || self.spirits_from(ww) > 0
            }
            // Hand-made planks may already meet the target, leaving a new
            // spirit nothing to do yet. Creating it is the step that counts.
            Phase::Automation => self.crafters().next().is_some(),
            Phase::Improvement => self.improved(),
            Phase::Return => self.level_regained(),
            Phase::Settled => false,
        }
    }

    fn next_phase(phase: Phase) -> Phase {
        match phase {
            Phase::Beginnings => Phase::FirstSpirit,
            Phase::FirstSpirit => Phase::FirstCraft,
            Phase::FirstCraft => Phase::Automation,
            Phase::Automation => Phase::Improvement,
            Phase::Improvement => Phase::Return,
            Phase::Return | Phase::Settled => Phase::Settled,
        }
    }

    fn milestone_met(&self, m: Milestone) -> bool {
        let wc = self.skill_named("woodcutting");
        let ww = self.skill_named("woodworking");
        let first_tier = |skill: Option<SkillId>| {
            skill.map_or(u32::MAX, |s| {
                let trees = self.defs.tree_tiers(s);
                let recipes = self.defs.recipe_tiers(s);
                trees
                    .iter()
                    .map(|k| self.defs.tree(*k).level_required)
                    .chain(recipes.iter().map(|r| self.defs.recipe(*r).level_required))
                    .filter(|l| *l > 1)
                    .min()
                    .unwrap_or(u32::MAX)
            })
        };
        let highest =
            |skill: Option<SkillId>| skill.map_or(1, |s| self.player.history(s).highest_level);
        match m {
            Milestone::FirstLevel => highest(wc) >= 2 || highest(ww) >= 2,
            Milestone::FirstUnlock => highest(wc) >= first_tier(wc),
            Milestone::FirstSpirit => self.spirits_from(wc) + self.spirits_from(ww) > 0,
            Milestone::FirstPlank => ww.is_some_and(|s| self.player.xp(s) > 0) || highest(ww) >= 2,
            Milestone::CraftUnlock => highest(ww) >= first_tier(ww),
            Milestone::CraftSpirit => self.spirits_from(ww) > 0,
            Milestone::ChainPlank => self.crafters().any(|s| s.units_crafted > 0),
            Milestone::FirstImprovement => self.improved(),
            Milestone::LevelRegained => self.level_regained(),
        }
    }

    /// Advances the guidance. Only playthroughs have any.
    pub(crate) fn update_journey(&mut self) {
        const MILESTONES: [Milestone; 9] = [
            Milestone::FirstLevel,
            Milestone::FirstUnlock,
            Milestone::FirstSpirit,
            Milestone::FirstPlank,
            Milestone::CraftUnlock,
            Milestone::CraftSpirit,
            Milestone::ChainPlank,
            Milestone::FirstImprovement,
            Milestone::LevelRegained,
        ];
        let Some(journey) = &self.journey else {
            return;
        };
        let mut phase = journey.phase;
        let start = phase;
        while phase != Phase::Settled && self.phase_done(phase) {
            phase = Self::next_phase(phase);
        }
        let reached: Vec<Milestone> = MILESTONES
            .into_iter()
            .filter(|m| journey.tick_of(*m).is_none() && self.milestone_met(*m))
            .collect();
        let tick = self.tick;
        if let Some(j) = &mut self.journey {
            j.phase = phase;
            j.reached.extend(reached.into_iter().map(|m| (m, tick)));
        }
        if phase != start
            && let Some(g) = self.guidance()
        {
            let msg = format!("New goal: {}", g.title);
            self.events.push(tick, EventKind::Player, msg.clone());
            self.notices.push(tick, msg);
        }
    }

    /// The current goal and next step, for playthroughs.
    pub fn guidance(&self) -> Option<Guidance> {
        let phase = self.journey.as_ref()?.phase;
        let wc = self.skill_named("woodcutting")?;
        let ww = self.skill_named("woodworking")?;
        let table = &self.defs.xp_table;
        let bag = &self.player.actor.inventory;
        let g = match phase {
            Phase::Beginnings => {
                let goal = self.defs.journey.first_spirit_level;
                let hint = if bag.is_full() {
                    "Your bag is full. Click Main Storage to deposit your logs."
                } else if bag.total() > 0 {
                    "Keep chopping, or click Main Storage to deposit what you have."
                } else {
                    "Click a tree in the Starting Grove to walk over and chop it."
                };
                Guidance {
                    title: format!("Reach Woodcutting level {goal}"),
                    hint: hint.into(),
                    progress: Some((
                        self.player.xp(wc),
                        table.xp_for_level(goal),
                        "Woodcutting XP".into(),
                    )),
                }
            }
            Phase::FirstSpirit => {
                let oak = self.defs.tree_tiers(wc).last().copied();
                let oak_level = oak.map_or(1, |k| self.defs.tree(k).level_required);
                Guidance {
                    title: "Create a Woodcutting Spirit".into(),
                    hint: format!(
                        "Soul Separation trades your XP for a worker that gathers forever. It costs you levels, for good. You can train on first: a spirit needs {} XP to reach level {oak_level}.",
                        table.xp_for_level(oak_level)
                    ),
                    progress: Some((
                        self.player.xp(wc),
                        self.defs.soul_min_xp,
                        "XP (the smallest spirit)".into(),
                    )),
                }
            }
            Phase::FirstCraft => {
                let goal = self.defs.journey.woodworking_level;
                let hint = if bag.get(self.defs.objective.resource) > 0 {
                    "Click the Workshop to saw your logs into planks."
                } else if bag.total() > 0 {
                    "Click Main Storage to store what you're carrying."
                } else {
                    "Click Main Storage with empty hands to take logs, then click the Workshop."
                };
                Guidance {
                    title: format!("Train Woodworking to level {goal}"),
                    hint: hint.into(),
                    progress: Some((
                        self.player.xp(ww),
                        table.xp_for_level(goal),
                        "Woodworking XP".into(),
                    )),
                }
            }
            Phase::Automation => {
                Guidance {
                    title: "Create a Woodworking Spirit".into(),
                    hint: "Pick Woodworking in Soul Separation. A level 5 spirit keeps Precise Planks for good, and the Production tab (P) shows how the chain is doing.".into(),
                    progress: None,
                }
            }
            Phase::Improvement => {
                let storehouse = self
                    .defs
                    .buildings
                    .iter()
                    .find(|b| b.capacity_bonus > 0)
                    .map_or("improvement".to_string(), |b| b.name.clone());
                let hint = if self.active_project().is_some() {
                    "It's underway. Your Construction Spirit needs the materials in Main Storage."
                } else {
                    "Order it from the Production tab (P). The spirits make what's missing."
                };
                Guidance {
                    title: format!("Build a {storehouse}"),
                    hint: hint.into(),
                    progress: None,
                }
            }
            Phase::Return => {
                let target = [wc, ww]
                    .into_iter()
                    .filter(|s| self.player.history(*s).spirits_created > 0)
                    .min_by_key(|s| {
                        let h = self.player.history(*s);
                        table
                            .xp_for_level(h.highest_level)
                            .saturating_sub(self.player.xp(*s))
                    })?;
                let h = self.player.history(target);
                let name = &self.defs.skill(target).name;
                Guidance {
                    title: format!("Regain {name} level {}", h.highest_level),
                    hint: "Your settlement can help now. The Training tab (T) reserves a site for you and sets Hauling Spirits to carry your logs.".into(),
                    progress: Some((
                        self.player.xp(target),
                        table.xp_for_level(h.highest_level),
                        format!("{name} XP"),
                    )),
                }
            }
            Phase::Settled => {
                let has_hauling = self.has_role_with(JobKind::Haul);
                Guidance {
                    title: "Your settlement runs itself".into(),
                    hint: if has_hauling {
                        "Keep training, sacrifice more or spend planks. What comes next is up to you."
                            .into()
                    } else {
                        "What comes next is up to you.".into()
                    },
                    progress: None,
                }
            }
        };
        Some(g)
    }
}
