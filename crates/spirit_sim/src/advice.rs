//! Plain-language readings of the world: the one-line settlement summary, what
//! to do when storage runs out, and the report for comparing play sessions.
//!
//! Almost everything here only reads. The exception is the storage notice,
//! which tells the player once when room gets short.

use crate::defs::{JobKind, SkillId};
use crate::entities::Activity;
use crate::metrics::{TimeUse, WaitKind};
use crate::outpost::WorkerState;
use crate::world::World;

/// The settlement in a few numbers and at most one sentence of concern.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettlementGlance {
    pub working: u32,
    pub waiting: u32,
    pub blocked: u32,
    pub idle: u32,
    /// Of the idle ones, how many are awaiting an assignment.
    pub unassigned: u32,
    /// How production stands, in a few words.
    pub production: String,
    pub production_ok: bool,
    /// The most important thing that needs the player, if anything does.
    pub attention: Option<String>,
}

impl World {
    /// When Main Storage can't take another bagful: what's happening and what
    /// the player can do about it with what they have.
    pub fn storage_advice(&self) -> Option<String> {
        let main = &self.main().storage;
        if main.free() >= self.defs.player.carry_capacity {
            return None;
        }
        let mut fixes = Vec::new();
        if self.crafters().next().is_some() {
            fixes.push("raise the plank target in Production (P) so spirits use the logs".into());
        } else if self.station().is_some() {
            fixes.push("saw logs at the Workshop: planks take up half the room".to_string());
        }
        let storehouse = self
            .defs
            .buildings
            .iter()
            .enumerate()
            .find(|(_, b)| b.manual && b.capacity_bonus > 0);
        if let Some((i, def)) = storehouse {
            let built = self
                .buildings
                .iter()
                .filter(|b| b.kind.index() == i)
                .count() as u32;
            if def.max_count.is_none_or(|m| built < m) {
                fixes.push(format!(
                    "order a {} (+{} room) in Production (P)",
                    def.name, def.capacity_bonus
                ));
            }
        }
        let tail = if fixes.is_empty() {
            String::new()
        } else {
            format!(" You can {}.", fixes.join(", or "))
        };
        Some(format!(
            "Main Storage is nearly full ({} / {}), and a full bag can't earn XP.{tail}",
            main.total(),
            main.capacity
        ))
    }

    /// Says so once when room runs short, and again only after it's recovered.
    pub(crate) fn update_storage_warning(&mut self) {
        let free = self.main().storage.free();
        let bag = self.defs.player.carry_capacity;
        if self.player.log.storage_warned {
            if free >= bag * 2 {
                self.player.log.storage_warned = false;
            }
        } else if let Some(advice) = self.storage_advice() {
            self.player.log.storage_warned = true;
            self.notices.push(self.tick, advice);
        }
    }

    pub fn settlement_glance(&self) -> SettlementGlance {
        let (mut working, mut waiting, mut blocked, mut idle, mut unassigned) = (0, 0, 0, 0, 0);
        for s in &self.spirits {
            match self.worker_state(s) {
                WorkerState::Working => working += 1,
                WorkerState::NoResources => waiting += 1,
                WorkerState::Logistics => blocked += 1,
                WorkerState::GoalsMet => idle += 1,
                WorkerState::Unassigned => {
                    idle += 1;
                    unassigned += 1;
                }
            }
        }
        let orders = self.order_reports();
        let wanted: Vec<_> = orders.iter().filter(|o| o.enabled && o.goal > 0).collect();
        let (production, production_ok) = if self.crafters().next().is_none() {
            ("No Woodworking Spirit yet".to_string(), true)
        } else if let Some(o) = wanted.iter().find(|o| o.stalled) {
            (format!("{} stalled", o.name), false)
        } else if wanted.iter().all(|o| o.in_main >= o.goal) {
            ("Targets met".to_string(), true)
        } else {
            let rate: f32 = wanted.iter().map(|o| o.per_min).sum();
            (format!("Running, {rate:.0} a minute"), true)
        };
        let attention = self.storage_advice().or_else(|| {
            let project = self.projects.iter().find(|p| p.is_active());
            project
                .and_then(|p| {
                    let name = &self.defs.building(p.kind).name;
                    p.blocked.as_ref().map(|b| format!("{name}: {b}"))
                })
                .or_else(|| {
                    // Before there's a spirit to do it, nothing is wrong.
                    let staffed = self.crafters().next().is_some();
                    let stalled = wanted.iter().find(|o| staffed && o.stalled);
                    stalled.map(|o| o.status.clone())
                })
                .or_else(|| {
                    self.spirits
                        .iter()
                        .find(|s| {
                            matches!(s.activity, Activity::Blocked(_))
                                && matches!(s.wait, Some(WaitKind::Storage | WaitKind::Other))
                        })
                        .map(|s| format!("{}: {}", s.name, s.activity.text()))
                })
        });
        SettlementGlance {
            working,
            waiting,
            blocked,
            idle,
            unassigned,
            production,
            production_ok,
            attention,
        }
    }

    /// How the play so far went, as text to read or compare.
    pub fn play_report(&self) -> String {
        let tps = self.defs.ticks_per_second as u64;
        let clock = |ticks: u64| {
            let s = ticks / tps;
            format!("{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
        };
        let t: &TimeUse = &self.player.time;
        let log = &self.player.log;
        let total = t.total().max(1);
        let row =
            |name: &str, n: u64| format!("  {name:<28}{}  {:>3}%\n", clock(n), n * 100 / total);
        let mut out = format!(
            "SpiritScape play report: {} played, {} profile\n\nWhere your time went\n",
            clock(self.tick),
            self.defs.profile.title
        );
        out += &row("Gathering", t.gathering);
        out += &row("Processing", t.crafting);
        out += &row("Moving", t.traveling);
        out += &row("Loading and unloading", t.handling);
        out += &row("Waiting for trees", t.waiting_recovery);
        out += &row("Waiting for materials", t.waiting_input);
        out += &row("Stuck with a full bag", t.blocked_storage);
        out += &row("Idle", t.idle);
        let minutes = (self.tick / tps / 60).max(1);
        let clicks = log.gather_interactions
            + log.workshop_interactions
            + log.storage_interactions
            + log.walk_interactions;
        out += &format!(
            "\nInteractions ({} a minute)\n  Trees {}, workshops {}, storages {}, walking {}, refused {}\n  Sessions started: {}\n",
            clicks as u64 / minutes,
            log.gather_interactions,
            log.workshop_interactions,
            log.storage_interactions,
            log.walk_interactions,
            log.refused_interactions,
            log.sessions
        );
        out += "\nXP earned\n";
        for i in 0..self.defs.skills.len() {
            let skill = SkillId(i as u16);
            if let Some(xp) = log.xp_earned.get(&skill) {
                out += &format!(
                    "  {:<28}{xp} ({} a minute)\n",
                    self.defs.skill(skill).name,
                    xp / minutes
                );
            }
        }
        out += &format!(
            "\nSpirits while you trained ({} of training)\n  Busy {}% of the time, cut {} logs, made {} goods\n",
            clock(log.training_ticks),
            (log.spirit_busy_ticks * 100)
                .checked_div(log.spirit_ticks)
                .unwrap_or(0),
            log.spirit_gathered,
            log.spirit_crafted
        );
        if !log.regains.is_empty() {
            out += "\nRegaining sacrificed levels\n";
            for r in &log.regains {
                let name = &self.defs.skill(r.skill).name;
                let state = match r.done {
                    Some(done) => format!("back after {}", clock(done - r.started)),
                    None => format!("not yet, {} so far", clock(self.tick - r.started)),
                };
                out += &format!(
                    "  {name} level {} ({} XP given): {state}\n",
                    r.level, r.xp_given
                );
            }
        }
        out
    }

    /// Whether a spirit could be working toward the player's training at all.
    pub fn has_training_help(&self) -> bool {
        self.has_worker_for(JobKind::TrainingSupply)
            || self.has_worker_for(JobKind::TrainingCollect)
    }
}
