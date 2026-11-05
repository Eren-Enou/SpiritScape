//! Saving and loading a world.
//!
//! A save file is one header line followed by JSON:
//!
//! ```text
//! SPIRITSCAPE-SAVE <format version> <FNV-1a checksum of the JSON>
//! {"meta": ..., "ids": ..., "state": ...}
//! ```
//!
//! `state` is the complete simulation state, so a loaded world continues
//! exactly as the saved one would have. Definitions are not saved: they come
//! from the game's data and the save's named profile, and `ids` records the
//! definition order the save's index ids refer to, so a save from incompatible
//! data is refused rather than misread.
//!
//! Nothing here simulates time away: a loaded world resumes at the saved tick.
//!
//! Older formats are upgraded when they're read (`migrate_v1`), and the first
//! time that happens `SaveStore` keeps a copy of the original file. Format 3
//! (v0.7) only added fields with safe defaults, so a format 2 save needs no
//! migration: a job under way in one finishes as a single task and the player
//! then stands idle, because older saves have no session to continue.

use crate::defs::{GameDefs, ResourceId};
use crate::entities::{Building, Player, Project, Spirit, Tree};
use crate::events::{EventLog, Notices};
use crate::grid::{BUILTIN_MAP, Grid, MapLayout};
use crate::journey::Journey;
use crate::metrics::SiteStats;
use crate::scenario::{extra_orders, new_tree};
use crate::session::SessionKind;
use crate::tasks::Task;
use crate::world::{Ledger, Settlement, World};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Bump when the format changes, and add a migration to `decode`.
pub const SAVE_VERSION: u32 = 3;
const MAGIC: &str = "SPIRITSCAPE-SAVE";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SaveError {
    /// There is no save to load.
    NotFound,
    /// The file couldn't be read or written.
    Io(String),
    /// The file is damaged.
    Corrupt(String),
    /// The file is sound but this version of the game can't use it.
    Incompatible(String),
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SaveError::NotFound => write!(f, "No save found"),
            SaveError::Io(e) => write!(f, "Save file error: {e}"),
            SaveError::Corrupt(e) => write!(f, "The save is damaged: {e}"),
            SaveError::Incompatible(e) => write!(f, "The save can't be used: {e}"),
        }
    }
}

/// What a save is, readable without loading it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveMeta {
    pub version: u32,
    /// The scenario key the world started from.
    pub scenario: String,
    /// The progression profile the world uses.
    pub profile: String,
    pub tick: u64,
    /// Wall-clock time of the save. Never used by the simulation.
    pub saved_unix: u64,
}

/// The order of definitions a save's index ids refer to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct DefIds {
    resources: Vec<String>,
    skills: Vec<String>,
    trees: Vec<String>,
    recipes: Vec<String>,
    sites: Vec<String>,
    roles: Vec<String>,
    buildings: Vec<String>,
}

impl DefIds {
    fn of(d: &GameDefs) -> Self {
        Self {
            resources: d.resources.iter().map(|x| x.id.clone()).collect(),
            skills: d.skills.iter().map(|x| x.id.clone()).collect(),
            trees: d.trees.iter().map(|x| x.id.clone()).collect(),
            recipes: d.recipes.iter().map(|x| x.id.clone()).collect(),
            sites: d.sites.iter().map(|x| x.id.clone()).collect(),
            roles: d.roles.iter().map(|x| x.id.clone()).collect(),
            buildings: d.buildings.iter().map(|x| x.id.clone()).collect(),
        }
    }

    /// New definitions may be added after the old ones; nothing may move.
    fn fits(&self, current: &DefIds) -> Result<(), String> {
        let lists = [
            ("resource", &self.resources, &current.resources),
            ("skill", &self.skills, &current.skills),
            ("tree type", &self.trees, &current.trees),
            ("recipe", &self.recipes, &current.recipes),
            ("site", &self.sites, &current.sites),
            ("role", &self.roles, &current.roles),
            ("building", &self.buildings, &current.buildings),
        ];
        for (what, saved, now) in lists {
            if !now.starts_with(saved) {
                return Err(format!(
                    "the game's {what} definitions have changed since it was made"
                ));
            }
        }
        Ok(())
    }
}

/// Everything that changes during play.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct WorldState {
    tick: u64,
    grid: Grid,
    trees: Vec<Tree>,
    buildings: Vec<Building>,
    spirits: Vec<Spirit>,
    player: Player,
    projects: Vec<Project>,
    settlement: Settlement,
    events: EventLog,
    ledger: Ledger,
    main_storage: usize,
    site_stats: Vec<SiteStats>,
    output_stats: SiteStats,
    notices: Notices,
    journey: Option<Journey>,
    #[serde(default)]
    product_stats: BTreeMap<ResourceId, SiteStats>,
}

#[derive(Serialize, Deserialize)]
struct SaveBody {
    meta: SaveMeta,
    ids: DefIds,
    state: WorldState,
}

impl World {
    fn state(&self) -> WorldState {
        WorldState {
            tick: self.tick,
            grid: self.grid.clone(),
            trees: self.trees.clone(),
            buildings: self.buildings.clone(),
            spirits: self.spirits.clone(),
            player: self.player.clone(),
            projects: self.projects.clone(),
            settlement: self.settlement.clone(),
            events: self.events.clone(),
            ledger: self.ledger.clone(),
            main_storage: self.main_storage,
            site_stats: self.site_stats.clone(),
            output_stats: self.output_stats.clone(),
            notices: self.notices.clone(),
            journey: self.journey.clone(),
            product_stats: self.product_stats.clone(),
        }
    }

    fn from_state(defs: GameDefs, s: WorldState) -> World {
        let mut world = World {
            defs,
            tick: s.tick,
            grid: s.grid,
            trees: s.trees,
            buildings: s.buildings,
            spirits: s.spirits,
            player: s.player,
            projects: s.projects,
            settlement: s.settlement,
            events: s.events,
            ledger: s.ledger,
            main_storage: s.main_storage,
            site_stats: s.site_stats,
            output_stats: s.output_stats,
            notices: s.notices,
            journey: s.journey,
            product_stats: s.product_stats,
        };
        for spirit in &mut world.spirits {
            spirit.refresh_level(&world.defs.xp_table);
        }
        world
    }

    /// The whole simulation state as text. Two worlds with the same text
    /// behave identically from here on.
    pub fn state_text(&self) -> String {
        serde_json::to_string(&self.state()).expect("simulation state always serializes")
    }
}

/// FNV-1a, 64-bit: detects accidental damage, and is stable across builds.
fn checksum(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |h, b| {
        (h ^ *b as u64).wrapping_mul(0x100000001b3)
    })
}

/// Turns a world into the contents of a save file.
pub fn encode(world: &World, scenario: &str) -> String {
    let saved_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let body = SaveBody {
        meta: SaveMeta {
            version: SAVE_VERSION,
            scenario: scenario.to_string(),
            profile: world.defs.profile.id.clone(),
            tick: world.tick,
            saved_unix,
        },
        ids: DefIds::of(&world.defs),
        state: world.state(),
    };
    seal(
        SAVE_VERSION,
        &serde_json::to_string(&body).expect("simulation state always serializes"),
    )
}

/// Puts the header on a save's JSON.
pub fn seal(version: u32, json: &str) -> String {
    format!(
        "{MAGIC} {version} {:016x}\n{json}",
        checksum(json.as_bytes())
    )
}

/// Splits a save into its checked JSON, refusing damaged or newer files.
fn open_save(text: &str) -> Result<&str, SaveError> {
    let (header, json) = text
        .split_once('\n')
        .ok_or_else(|| SaveError::Corrupt("the file is incomplete".into()))?;
    let mut parts = header.trim_end().split(' ');
    if parts.next() != Some(MAGIC) {
        return Err(SaveError::Corrupt("this isn't a SpiritScape save".into()));
    }
    let version: u32 = parts
        .next()
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| SaveError::Corrupt("the header is unreadable".into()))?;
    if version == 0 || version > SAVE_VERSION {
        return Err(SaveError::Incompatible(format!(
            "it uses save format {version}, and this game reads up to format {SAVE_VERSION}"
        )));
    }
    let stored = parts
        .next()
        .and_then(|c| u64::from_str_radix(c, 16).ok())
        .ok_or_else(|| SaveError::Corrupt("the header is unreadable".into()))?;
    if checksum(json.as_bytes()) != stored {
        return Err(SaveError::Corrupt("its contents don't match".into()));
    }
    Ok(json)
}

/// Reads a save's description without building the world.
pub fn read_meta(text: &str) -> Result<SaveMeta, SaveError> {
    #[derive(Deserialize)]
    struct MetaOnly {
        meta: SaveMeta,
    }
    let json = open_save(text)?;
    let m: MetaOnly = serde_json::from_str(json).map_err(|e| SaveError::Corrupt(e.to_string()))?;
    Ok(m.meta)
}

/// Rebuilds a world from the contents of a save file, using the built-in data.
pub fn decode(text: &str) -> Result<(World, SaveMeta), SaveError> {
    let json = open_save(text)?;
    let mut body: SaveBody =
        serde_json::from_str(json).map_err(|e| SaveError::Corrupt(e.to_string()))?;
    let defs = GameDefs::builtin_profile(&body.meta.profile).map_err(SaveError::Incompatible)?;
    body.ids
        .fits(&DefIds::of(&defs))
        .map_err(SaveError::Incompatible)?;
    let map = MapLayout::parse(BUILTIN_MAP).map_err(SaveError::Incompatible)?;
    if (body.state.grid.width, body.state.grid.height) != (map.width, map.height) {
        return Err(SaveError::Incompatible("the map has changed".into()));
    }
    if body.meta.version < 2 {
        migrate_v1(&defs, &map, &mut body.state);
    }
    validate(&defs, &body.state).map_err(SaveError::Corrupt)?;
    let meta = body.meta;
    let world = World::from_state(defs, body.state);
    world.check_conservation().map_err(SaveError::Corrupt)?;
    Ok((world, meta))
}

/// Format 1 (v0.5) to 2 (v0.6). New fields have serde defaults; this adds what
/// a default can't: the world content and storage room v0.6 introduced. Nothing
/// that existed is changed, and the new trees aren't resources until they're
/// cut, so the ledger still balances.
fn migrate_v1(defs: &GameDefs, map: &MapLayout, s: &mut WorldState) {
    s.settlement.site_open.resize(defs.sites.len(), true);
    s.site_stats.resize(defs.sites.len(), SiteStats::default());
    if s.settlement.extra_orders.is_empty() {
        s.settlement.extra_orders = extra_orders(&defs.production.more_outputs);
    }
    for b in &mut s.buildings {
        let wanted = defs.building(b.kind).storage_capacity();
        if defs.building(b.kind).is_station && b.storage.capacity < wanted {
            b.storage.capacity = wanted;
        }
    }
    for (&c, positions) in &map.markers {
        let Some(kind) = defs.tree_kind_for_char(c) else {
            continue;
        };
        for &pos in positions {
            // Something may have been built here since.
            if !s.grid.is_occupied(pos) {
                let id = s.trees.len();
                s.trees.push(new_tree(defs, kind, pos, id));
                s.grid.set_occupied(pos, true);
            }
        }
    }
}

/// Every reference in the state points at something real, so the simulation
/// can index without checking.
fn validate(defs: &GameDefs, s: &WorldState) -> Result<(), String> {
    let check = |ok: bool, what: &str| {
        if ok {
            Ok(())
        } else {
            Err(format!("bad reference: {what}"))
        }
    };
    check(s.main_storage < s.buildings.len(), "main storage")?;
    let sites = defs.sites.len();
    check(s.settlement.site_open.len() == sites, "site list")?;
    check(s.site_stats.len() == sites, "site stats")?;
    check(
        s.settlement.training.area.is_none_or(|a| a.index() < sites),
        "training area",
    )?;
    for (i, t) in s.trees.iter().enumerate() {
        check(t.id == i && t.kind.index() < defs.trees.len(), "tree")?;
        check(t.site.is_none_or(|x| x.index() < sites), "tree site")?;
        check(
            t.claimed_by.is_none_or(|c| c < s.spirits.len()),
            "tree claim",
        )?;
    }
    for (i, b) in s.buildings.iter().enumerate() {
        check(
            b.id == i && b.kind.index() < defs.buildings.len(),
            "building",
        )?;
    }
    for (i, p) in s.projects.iter().enumerate() {
        check(
            p.id == i && p.kind.index() < defs.buildings.len() && p.building < s.buildings.len(),
            "project",
        )?;
        check(
            p.assigned.is_none_or(|a| a < s.spirits.len()),
            "project worker",
        )?;
    }
    if let Some(r) = s.player.recipe {
        check(r.index() < defs.recipes.len(), "chosen recipe")?;
    }
    if let Some(session) = &s.player.session {
        match session.kind {
            SessionKind::Gathering {
                site,
                resource,
                prefer,
                target,
            } => {
                check(site.is_none_or(|x| x.index() < sites), "session site")?;
                check(resource.index() < defs.resources.len(), "session resource")?;
                check(prefer.index() < defs.trees.len(), "session tree type")?;
                check(target < s.trees.len(), "session tree")?;
            }
            SessionKind::Crafting { building } => {
                check(building < s.buildings.len(), "session building")?;
            }
        }
        check(session.started <= s.tick, "session start")?;
    }
    let task_ok = |t: &Task| match t {
        Task::Move { .. } => true,
        Task::Chop { tree } => *tree < s.trees.len(),
        Task::Deposit { building }
        | Task::DepositForPlayer { building }
        | Task::Withdraw { building, .. }
        | Task::Craft { building, .. } => *building < s.buildings.len(),
        Task::DeliverToSite { project } | Task::Build { project } => *project < s.projects.len(),
    };
    check(s.player.actor.queue.iter().all(task_ok), "player task")?;
    for (i, spirit) in s.spirits.iter().enumerate() {
        check(
            spirit.id == i && spirit.role.index() < defs.roles.len(),
            "spirit",
        )?;
        check(spirit.skill().index() < defs.skills.len(), "spirit skill")?;
        check(spirit.actor.queue.iter().all(task_ok), "spirit task")?;
    }
    Ok(())
}

/// Which save file to use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Manual,
    Auto,
}

impl Slot {
    fn file(self) -> &'static str {
        match self {
            Slot::Manual => "world.save",
            Slot::Auto => "autosave.save",
        }
    }
}

/// A save that could be continued, and a problem found on the way to it.
pub struct Loaded {
    pub world: World,
    pub meta: SaveMeta,
    /// Set when the newest file was unusable and an older one was loaded.
    pub warning: Option<String>,
}

/// The folder saves live in.
#[derive(Clone, Debug)]
pub struct SaveStore {
    dir: PathBuf,
}

impl SaveStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// The per-user data folder: `SPIRITSCAPE_SAVE_DIR` if set, otherwise the
    /// platform's application data location.
    pub fn default_dir() -> PathBuf {
        let env = |k: &str| {
            std::env::var_os(k)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        };
        if let Some(dir) = env("SPIRITSCAPE_SAVE_DIR") {
            return dir;
        }
        let base = if cfg!(windows) {
            env("APPDATA")
        } else if cfg!(target_os = "macos") {
            env("HOME").map(|h| h.join("Library/Application Support"))
        } else {
            env("XDG_DATA_HOME").or_else(|| env("HOME").map(|h| h.join(".local/share")))
        };
        base.unwrap_or_else(|| PathBuf::from("."))
            .join("SpiritScape")
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn path(&self, slot: Slot) -> PathBuf {
        self.dir.join(slot.file())
    }

    fn backup(&self, slot: Slot) -> PathBuf {
        self.dir.join(format!("{}.bak", slot.file()))
    }

    /// Writes a save so that a crash at any moment leaves a usable file: the
    /// new contents go to a temporary file first and replace the old file in
    /// one step, and the previous save is kept as a backup.
    pub fn write(&self, slot: Slot, world: &World, scenario: &str) -> Result<(), SaveError> {
        let io = |e: std::io::Error| SaveError::Io(e.to_string());
        fs::create_dir_all(&self.dir).map_err(io)?;
        let path = self.path(slot);
        let tmp = self.dir.join(format!("{}.tmp", slot.file()));
        let mut file = fs::File::create(&tmp).map_err(io)?;
        file.write_all(encode(world, scenario).as_bytes())
            .map_err(io)?;
        file.sync_all().map_err(io)?;
        drop(file);
        if path.exists() {
            fs::copy(&path, self.backup(slot)).map_err(io)?;
        }
        fs::rename(&tmp, &path).map_err(io)
    }

    fn read_file(path: &Path) -> Result<String, SaveError> {
        match fs::read(path) {
            Ok(bytes) => String::from_utf8(bytes)
                .map_err(|_| SaveError::Corrupt("the file isn't text".into())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(SaveError::NotFound),
            Err(e) => Err(SaveError::Io(e.to_string())),
        }
    }

    /// Describes a slot without loading the world.
    pub fn meta(&self, slot: Slot) -> Result<SaveMeta, SaveError> {
        read_meta(&Self::read_file(&self.path(slot))?)
    }

    /// The slot holding the most recent readable save.
    pub fn newest(&self) -> Option<(Slot, SaveMeta)> {
        [Slot::Manual, Slot::Auto]
            .into_iter()
            .filter_map(|s| self.meta(s).ok().map(|m| (s, m)))
            .max_by_key(|(_, m)| (m.saved_unix, m.tick))
    }

    /// Whether anything at all is stored, readable or not.
    pub fn any_files(&self) -> bool {
        [Slot::Manual, Slot::Auto]
            .into_iter()
            .any(|s| self.path(s).exists() || self.backup(s).exists())
    }

    /// Loads a slot, falling back to its backup if the file is damaged.
    pub fn load(&self, slot: Slot) -> Result<Loaded, SaveError> {
        let first = Self::read_file(&self.path(slot)).and_then(|t| decode(&t));
        let error = match first {
            Ok((world, meta)) => {
                let warning = self.keep_original(slot, meta.version);
                return Ok(Loaded {
                    world,
                    meta,
                    warning,
                });
            }
            Err(e) => e,
        };
        // A file that's merely from another version isn't a reason to go back.
        if matches!(error, SaveError::Incompatible(_)) {
            return Err(error);
        }
        match Self::read_file(&self.backup(slot)).and_then(|t| decode(&t)) {
            Ok((world, meta)) => Ok(Loaded {
                world,
                meta,
                warning: Some(format!("{error}. Loaded the previous save instead.")),
            }),
            Err(_) => Err(error),
        }
    }

    /// A save from an older format is about to be replaced by the next
    /// write, so the original is kept beside it. Returns what to tell the player.
    fn keep_original(&self, slot: Slot, version: u32) -> Option<String> {
        if version >= SAVE_VERSION {
            return None;
        }
        let kept = self.dir.join(format!("{}.v{version}", slot.file()));
        if !kept.exists() && fs::copy(self.path(slot), &kept).is_err() {
            return Some("Upgraded your save, but couldn't keep a copy of the original.".into());
        }
        Some(format!(
            "Upgraded your save. The original is kept as {}.",
            kept.file_name()?.to_string_lossy()
        ))
    }

    /// Loads the most recent save that works, trying every file.
    pub fn load_newest(&self) -> Result<Loaded, SaveError> {
        let mut order = [Slot::Manual, Slot::Auto];
        if self.newest().is_some_and(|(s, _)| s == Slot::Auto) {
            order.reverse();
        }
        let mut problem = SaveError::NotFound;
        for slot in order {
            match self.load(slot) {
                Ok(loaded) => return Ok(loaded),
                Err(SaveError::NotFound) => {}
                Err(e) => problem = e,
            }
        }
        Err(problem)
    }
}
