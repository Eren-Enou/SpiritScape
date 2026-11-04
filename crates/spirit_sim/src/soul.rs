//! Making Soul Separation a readable decision: named options to compare, and
//! plain statements of what each one gives and costs. The ritual itself is in
//! `world`; nothing here changes its rules.

use crate::defs::{JobKind, RecipeId, RoleId, SkillId, TreeKindId};
use crate::world::{SoulPreview, World};

/// Most options offered at once.
const MAX_OPTIONS: usize = 5;

/// A sacrifice worth comparing, with the reason it's on the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SoulOption {
    pub label: String,
    pub preview: SoulPreview,
}

impl World {
    /// What a spirit of this skill and level would add to the settlement.
    pub(crate) fn spirit_benefit(&self, skill: SkillId, level: u32) -> String {
        let Some(role) = self.defs.soul_role_for(skill).map(|r| self.defs.role(r)) else {
            return String::new();
        };
        let tpm = self.defs.ticks_per_second * 60;
        if role.has_job(JobKind::Gather) {
            let best = self
                .defs
                .tree_tiers(skill)
                .into_iter()
                .filter(|k| self.defs.tree(*k).level_required <= level)
                .next_back();
            return match best {
                Some(k) => {
                    let def = self.defs.tree(k);
                    format!(
                        "Cuts about {} logs/min from {} while chopping",
                        tpm / def.chop_ticks_at(level).max(1),
                        def.name
                    )
                }
                None => "Too weak to cut anything".into(),
            };
        }
        if role.has_job(JobKind::Craft) {
            let best = self
                .defs
                .recipe_tiers(skill)
                .into_iter()
                .filter(|r| self.defs.recipe(*r).level_required <= level)
                .next_back();
            return match best {
                Some(r) => {
                    let def = self.defs.recipe(r);
                    let out = &self.defs.resource(def.output.0).name;
                    format!(
                        "Makes about {} {out}/min at the bench ({})",
                        tpm * def.output.1 / def.craft_ticks_at(level).max(1),
                        def.name
                    )
                }
                None => "Too weak to make anything".into(),
            };
        }
        String::new()
    }

    /// Sacrifices worth comparing for `skill`, smallest first: the minimum,
    /// the most that keeps each thing the player can do, the least that gives
    /// the spirit each tier, and everything.
    pub fn soul_options(&self, skill: SkillId) -> Vec<SoulOption> {
        let table = &self.defs.xp_table;
        let have = self.player.xp(skill);
        let min = self.defs.soul_min_xp;
        if have < min {
            return Vec::new();
        }
        let level = table.level_for_xp(have);
        let mut tiers: Vec<(String, u32)> = self
            .defs
            .tree_tiers(skill)
            .into_iter()
            .map(|k| self.defs.tree(k))
            .map(|d| (d.name.clone(), d.level_required))
            .chain(
                self.defs
                    .recipe_tiers(skill)
                    .into_iter()
                    .map(|r| self.defs.recipe(r))
                    .map(|d| (d.name.clone(), d.level_required)),
            )
            .filter(|(_, required)| *required > 1)
            .collect();
        tiers.sort_by_key(|t| t.1);

        let mut amounts: Vec<(u64, String)> = vec![(min, "Smallest".to_string())];
        for (name, required) in &tiers {
            let needed = table.xp_for_level(*required);
            if needed <= have {
                amounts.push((needed, format!("{name} spirit")));
            }
            if level >= *required && have - needed >= min {
                amounts.push((have - needed, format!("Keep {name}")));
            }
        }
        amounts.push((have, "Everything".to_string()));
        amounts.sort();
        amounts.dedup_by_key(|a| a.0);
        // Over the limit, the ends of the range matter most.
        while amounts.len() > MAX_OPTIONS {
            amounts.remove(amounts.len() / 2);
        }
        amounts
            .into_iter()
            .map(|(xp, label)| SoulOption {
                label,
                preview: self.soul_preview(skill, xp),
            })
            .collect()
    }

    /// The short message shown when a spirit awakens.
    pub(crate) fn spirit_notice(&self, role: RoleId, p: &SoulPreview) -> String {
        let def = self.defs.role(role);
        if let Some(best) = p.spirit_recipes.last() {
            return format!(
                "Your {} can produce {}.",
                def.name,
                self.defs.recipe(*best).name
            );
        }
        if !p.spirit_can_cut.is_empty() {
            return format!(
                "Your {} can cut {}.",
                def.name,
                self.tree_names(&p.spirit_can_cut)
            );
        }
        format!("Your {} has awakened.", def.name)
    }

    /// The short message for a level gained or regained.
    pub(crate) fn level_notice(
        &self,
        skill: SkillId,
        level: u32,
        highest_before: u32,
        trees: &[TreeKindId],
        recipes: &[RecipeId],
    ) -> String {
        let name = &self.defs.skill(skill).name;
        let regained = level <= highest_before;
        let opening = if regained {
            format!("{name} level {level} regained.")
        } else if trees.is_empty() && recipes.is_empty() {
            format!("{name} level {level}.")
        } else {
            format!("{name} level increased.")
        };
        let state = if regained { "again" } else { "now" };
        let mut parts = vec![opening];
        if !trees.is_empty() {
            parts.push(format!(
                "{} trees are {state} available.",
                self.tree_names(trees)
            ));
        }
        if !recipes.is_empty() {
            parts.push(format!(
                "{} are {state} available.",
                self.recipe_names(recipes)
            ));
        }
        parts.join(" ")
    }
}
