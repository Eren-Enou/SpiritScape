//! Physical things in the world: trees, buildings, the player and spirits.

use crate::defs::{
    BuildingKindId, JobKind, RecipeId, ResourceId, RoleId, SiteId, SkillId, TreeKindId,
};
use crate::grid::TilePos;
use crate::inventory::{Inventory, Storage};
use crate::metrics::{PlayLog, TimeLog, TimeUse, WaitKind};
use crate::session::{Session, SessionKind, SessionSummary};
use crate::skills::XpTable;
use crate::tasks::Task;
use std::collections::{BTreeMap, VecDeque};

pub type TreeId = usize;
pub type BuildingId = usize;
pub type SpiritId = usize;
pub type ProjectId = usize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TreeState {
    Grown { logs: u32 },
    Depleted { regrow_at: u64 },
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Tree {
    pub id: TreeId,
    pub kind: TreeKindId,
    pub pos: TilePos,
    pub state: TreeState,
    /// The gathering site this tree belongs to.
    pub site: Option<SiteId>,
    /// Regrowth time for this tree (type time scaled by its site).
    pub regrow_ticks: u64,
    /// A spirit currently working this tree, so others pick a different one.
    pub claimed_by: Option<SpiritId>,
}

impl Tree {
    pub fn logs_left(&self) -> u32 {
        match self.state {
            TreeState::Grown { logs } => logs,
            TreeState::Depleted { .. } => 0,
        }
    }

    pub fn is_grown(&self) -> bool {
        matches!(self.state, TreeState::Grown { .. })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum BuildingState {
    UnderConstruction { project: ProjectId },
    Complete,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Building {
    pub id: BuildingId,
    pub kind: BuildingKindId,
    pub pos: TilePos,
    pub name: String,
    pub state: BuildingState,
    pub storage: Storage,
    /// Units made here since it was built.
    #[serde(default)]
    pub crafted: u32,
}

impl Building {
    pub fn is_complete(&self) -> bool {
        self.state == BuildingState::Complete
    }
}

/// Tile-to-tile movement. `progress` counts ticks toward entering `path[0]`.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Mover {
    pub pos: TilePos,
    pub path: VecDeque<TilePos>,
    pub progress: u32,
    pub ticks_per_tile: u32,
}

impl Mover {
    pub fn new(pos: TilePos, ticks_per_tile: u32) -> Self {
        Self {
            pos,
            path: VecDeque::new(),
            progress: 0,
            ticks_per_tile,
        }
    }

    /// Interpolated position for rendering, in tile units.
    pub fn visual_pos(&self) -> (f32, f32) {
        match self.path.front() {
            Some(next) if self.progress > 0 => {
                let t = self.progress as f32 / self.ticks_per_tile as f32;
                (
                    self.pos.x as f32 + (next.x - self.pos.x) as f32 * t,
                    self.pos.y as f32 + (next.y - self.pos.y) as f32 * t,
                )
            }
            _ => (self.pos.x as f32, self.pos.y as f32),
        }
    }
}

/// State shared by everything that walks around and carries items.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Actor {
    pub mover: Mover,
    pub inventory: Inventory,
    pub queue: VecDeque<Task>,
    /// Ticks spent on the current timed action (chopping, crafting).
    pub action_ticks: u32,
    /// Tiles walked since the last harvested unit (the delivery leg).
    pub leg_tiles: u32,
    /// Work ticks spent on the current load.
    pub work_ticks: u32,
    /// Where the current load was harvested.
    pub last_gather: Option<TilePos>,
}

impl Actor {
    pub fn new(pos: TilePos, ticks_per_tile: u32, capacity: u32) -> Self {
        Self {
            mover: Mover::new(pos, ticks_per_tile),
            inventory: Inventory::new(capacity),
            queue: VecDeque::new(),
            action_ticks: 0,
            leg_tiles: 0,
            work_ticks: 0,
            last_gather: None,
        }
    }

    pub fn pos(&self) -> TilePos {
        self.mover.pos
    }

    pub fn clear_tasks(&mut self) {
        self.queue.clear();
        self.mover.path.clear();
        self.mover.progress = 0;
        self.action_ticks = 0;
    }

    /// Replaces the task queue with a new order without snapping back: a step
    /// already under way is finished first. Returns false (and changes nothing)
    /// when the queue is already the rest of this same order.
    pub fn redirect(&mut self, tasks: Vec<Task>) -> bool {
        let remaining = self.queue.len();
        if remaining > 0
            && remaining <= tasks.len()
            && self.queue.iter().eq(&tasks[tasks.len() - remaining..])
        {
            return false;
        }
        self.queue.clear();
        self.action_ticks = 0;
        let keep = if self.mover.progress > 0 { 1 } else { 0 };
        self.mover.path.truncate(keep);
        self.queue.extend(tasks);
        true
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Activity {
    Idle(String),
    Working(String),
    Blocked(String),
}

impl Activity {
    pub fn label(&self) -> &'static str {
        match self {
            Activity::Idle(_) => "Idle",
            Activity::Working(_) => "Working",
            Activity::Blocked(_) => "Blocked",
        }
    }

    pub fn text(&self) -> &str {
        match self {
            Activity::Idle(s) | Activity::Working(s) | Activity::Blocked(s) => s,
        }
    }
}

/// What a spirit gives its time to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Focus {
    /// Whatever its role allows, in the role's order.
    #[default]
    Anything,
    /// The settlement's work only: no training support.
    Settlement,
    /// The player's training only.
    Training,
    /// Nothing: it finishes what it's carrying and waits.
    Rest,
}

impl Focus {
    pub fn label(self) -> &'static str {
        match self {
            Focus::Anything => "Anything",
            Focus::Settlement => "Settlement work",
            Focus::Training => "My training",
            Focus::Rest => "Rest",
        }
    }

    pub fn next(self) -> Focus {
        match self {
            Focus::Anything => Focus::Settlement,
            Focus::Settlement => Focus::Training,
            Focus::Training => Focus::Rest,
            Focus::Rest => Focus::Anything,
        }
    }

    /// Whether a spirit with this focus may take a job.
    pub fn allows(self, job: JobKind) -> bool {
        let training = matches!(job, JobKind::TrainingCollect | JobKind::TrainingSupply);
        match self {
            Focus::Anything => true,
            Focus::Settlement => !training,
            // Collecting is included: the player's planks wait at the bench.
            Focus::Training => {
                training || matches!(job, JobKind::Deliver | JobKind::Gather | JobKind::Collect)
            }
            Focus::Rest => job == JobKind::Deliver,
        }
    }
}

/// The player's simple instructions to one spirit. A spirit can still only do
/// what its role and level allow: these narrow its work and never widen it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SpiritPolicy {
    pub focus: Focus,
    /// Gather only here.
    pub site: Option<SiteId>,
    /// Work only at this station.
    pub station: Option<BuildingId>,
    /// Gather the highest-tier material in demand before anything else.
    pub prefer_advanced: bool,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Spirit {
    pub id: SpiritId,
    pub name: String,
    pub role: RoleId,
    skill: SkillId,
    /// Fixed at creation. Spirits never gain XP; there is no setter.
    xp: u64,
    level: u32,
    pub actor: Actor,
    pub activity: Activity,
    pub job: Option<JobKind>,
    pub next_think: u64,
    pub last_deliver_target: Option<BuildingId>,
    pub units_gathered: u32,
    pub units_delivered: u32,
    pub units_crafted: u32,
    /// Assigned to the production order (only matters to spirits that craft).
    pub assigned: bool,
    /// The player's instructions.
    #[serde(default)]
    pub policy: SpiritPolicy,
    /// Why the spirit is currently not working, when blocked.
    pub wait: Option<WaitKind>,
    /// Site of the last tree this spirit chose; it prefers to stay there.
    pub home_site: Option<SiteId>,
    pub last_tree_kind: Option<TreeKindId>,
    /// How this spirit has spent its time.
    pub time: TimeLog,
    /// Last tick each kind of decision was logged, to rate-limit repeats.
    pub(crate) log_times: BTreeMap<String, u64>,
}

impl Spirit {
    pub(crate) fn new(
        id: SpiritId,
        name: String,
        role: RoleId,
        skill: SkillId,
        xp: u64,
        table: &XpTable,
        actor: Actor,
    ) -> Self {
        Self {
            id,
            name,
            role,
            skill,
            xp,
            level: table.level_for_xp(xp),
            actor,
            activity: Activity::Idle("Just awakened".into()),
            job: None,
            next_think: 0,
            last_deliver_target: None,
            units_gathered: 0,
            units_delivered: 0,
            units_crafted: 0,
            assigned: true,
            policy: SpiritPolicy::default(),
            wait: None,
            home_site: None,
            last_tree_kind: None,
            time: TimeLog::default(),
            log_times: BTreeMap::new(),
        }
    }

    pub fn xp(&self) -> u64 {
        self.xp
    }

    pub fn level(&self) -> u32 {
        self.level
    }

    pub fn skill(&self) -> SkillId {
        self.skill
    }

    /// Re-derives the level from the fixed XP when a save is loaded, so the
    /// XP is the one source of truth. XP itself is never touched.
    pub(crate) fn refresh_level(&mut self, table: &XpTable) {
        self.level = table.level_for_xp(self.xp);
    }
}

/// Lifetime progression in one skill, kept separately from current XP.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SkillHistory {
    pub highest_level: u32,
    pub xp_sacrificed: u64,
    pub spirits_created: u32,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Player {
    pub actor: Actor,
    xp: BTreeMap<SkillId, u64>,
    history: BTreeMap<SkillId, SkillHistory>,
    pub activity: String,
    /// How the player has spent their time since the start.
    pub time: TimeUse,
    /// The continuous activity the player started, if any.
    #[serde(default)]
    pub session: Option<Session>,
    /// How the last session went and why it ended.
    #[serde(default)]
    pub last_session: Option<SessionSummary>,
    /// A gathering session moves on to another tree by itself.
    #[serde(default = "auto_gather_default")]
    pub auto_gather: bool,
    /// The recipe the player has chosen. With none, the best they can make.
    #[serde(default)]
    pub recipe: Option<RecipeId>,
    /// Measurements for comparing play sessions.
    #[serde(default)]
    pub log: PlayLog,
}

fn auto_gather_default() -> bool {
    true
}

impl Player {
    pub fn new(actor: Actor) -> Self {
        Self {
            actor,
            xp: BTreeMap::new(),
            history: BTreeMap::new(),
            activity: "Idle".into(),
            time: TimeUse::default(),
            session: None,
            last_session: None,
            auto_gather: true,
            recipe: None,
            log: PlayLog::default(),
        }
    }

    /// Adds this tick to the matching time-use bucket.
    pub(crate) fn account_tick(&mut self) {
        let t = &mut self.time;
        let session = self.session.as_ref().map(|s| &s.kind);
        match self.actor.queue.front() {
            Some(Task::Chop { .. }) => t.gathering += 1,
            Some(Task::Craft { .. }) => t.crafting += 1,
            Some(Task::Move { .. }) => t.traveling += 1,
            Some(_) => t.handling += 1,
            None => match session {
                Some(SessionKind::Gathering { .. }) => t.waiting_recovery += 1,
                Some(SessionKind::Crafting { .. }) => t.waiting_input += 1,
                None if self.actor.inventory.is_full() => t.blocked_storage += 1,
                None => t.idle += 1,
            },
        }
    }

    /// Total XP the player has given to spirits, across every skill.
    pub fn xp_invested(&self) -> u64 {
        self.history.values().map(|h| h.xp_sacrificed).sum()
    }

    pub fn xp(&self, skill: SkillId) -> u64 {
        self.xp.get(&skill).copied().unwrap_or(0)
    }

    pub fn level(&self, skill: SkillId, table: &XpTable) -> u32 {
        table.level_for_xp(self.xp(skill))
    }

    pub fn history(&self, skill: SkillId) -> SkillHistory {
        let h = self.history.get(&skill).copied().unwrap_or_default();
        SkillHistory {
            highest_level: h.highest_level.max(1),
            ..h
        }
    }

    pub(crate) fn add_xp(&mut self, skill: SkillId, amount: u64, table: &XpTable) {
        *self.xp.entry(skill).or_default() += amount;
        let level = self.level(skill, table);
        let h = self.history.entry(skill).or_default();
        h.highest_level = h.highest_level.max(level);
    }

    pub(crate) fn record_sacrifice(&mut self, skill: SkillId, amount: u64) {
        let h = self.history.entry(skill).or_default();
        h.xp_sacrificed += amount;
        h.spirits_created += 1;
    }

    /// Removes exactly `amount` XP, or nothing if the player has less.
    pub(crate) fn take_xp(&mut self, skill: SkillId, amount: u64) -> bool {
        let have = self.xp(skill);
        if amount > have {
            return false;
        }
        self.xp.insert(skill, have - amount);
        true
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ProjectState {
    /// Waiting until the cost can be reserved in main storage.
    AwaitingMaterials,
    /// Materials reserved; the builder is carrying them to the site.
    Supplying,
    /// All materials on site; the builder is assembling the building.
    Building,
    Complete,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Project {
    pub id: ProjectId,
    pub kind: BuildingKindId,
    pub building: BuildingId,
    pub site: TilePos,
    pub state: ProjectState,
    pub assigned: Option<SpiritId>,
    pub reserved: bool,
    pub delivered: BTreeMap<ResourceId, u32>,
    pub consumed: BTreeMap<ResourceId, u32>,
    pub build_progress: u32,
    pub build_required: u32,
    /// Why the planner started this project.
    pub reason: String,
    /// Readable reason when the project cannot progress.
    pub blocked: Option<String>,
    pub created_tick: u64,
    pub completed_tick: Option<u64>,
}

impl Project {
    pub fn is_active(&self) -> bool {
        self.state != ProjectState::Complete
    }
}
