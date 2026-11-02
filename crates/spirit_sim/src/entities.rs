//! Physical things in the world: trees, buildings, the player and spirits.

use crate::defs::{BuildingKindId, JobKind, ResourceId, RoleId, SkillId, TreeKindId};
use crate::grid::TilePos;
use crate::inventory::{Inventory, Storage};
use crate::skills::XpTable;
use crate::tasks::Task;
use std::collections::{BTreeMap, VecDeque};

pub type TreeId = usize;
pub type BuildingId = usize;
pub type SpiritId = usize;
pub type ProjectId = usize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeState {
    Grown { logs: u32 },
    Depleted { regrow_at: u64 },
}

#[derive(Clone, Debug)]
pub struct Tree {
    pub id: TreeId,
    pub kind: TreeKindId,
    pub pos: TilePos,
    pub state: TreeState,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildingState {
    UnderConstruction { project: ProjectId },
    Complete,
}

#[derive(Clone, Debug)]
pub struct Building {
    pub id: BuildingId,
    pub kind: BuildingKindId,
    pub pos: TilePos,
    pub name: String,
    pub state: BuildingState,
    pub storage: Storage,
}

impl Building {
    pub fn is_complete(&self) -> bool {
        self.state == BuildingState::Complete
    }
}

/// Tile-to-tile movement. `progress` counts ticks toward entering `path[0]`.
#[derive(Clone, Debug)]
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
#[derive(Clone, Debug)]
pub struct Actor {
    pub mover: Mover,
    pub inventory: Inventory,
    pub queue: VecDeque<Task>,
    /// Ticks spent on the current timed action (chopping).
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

#[derive(Clone, Debug, PartialEq, Eq)]
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

#[derive(Clone, Debug)]
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
}

#[derive(Clone, Debug)]
pub struct Player {
    pub actor: Actor,
    xp: BTreeMap<SkillId, u64>,
    pub activity: String,
}

impl Player {
    pub fn new(actor: Actor) -> Self {
        Self {
            actor,
            xp: BTreeMap::new(),
            activity: "Idle".into(),
        }
    }

    pub fn xp(&self, skill: SkillId) -> u64 {
        self.xp.get(&skill).copied().unwrap_or(0)
    }

    pub fn level(&self, skill: SkillId, table: &XpTable) -> u32 {
        table.level_for_xp(self.xp(skill))
    }

    pub(crate) fn add_xp(&mut self, skill: SkillId, amount: u64) {
        *self.xp.entry(skill).or_default() += amount;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectState {
    /// Waiting until the cost can be reserved in main storage.
    AwaitingMaterials,
    /// Materials reserved; the builder is carrying them to the site.
    Supplying,
    /// All materials on site; the builder is assembling the building.
    Building,
    Complete,
}

#[derive(Clone, Debug)]
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
