//! Owns the simulation and advances it at a fixed tick rate scaled by the
//! chosen speed. Rendering never changes simulation state. Also owns the
//! player's saves: a world is written to and read from here, never by the UI.

use bevy::prelude::*;
use spirit_sim::defs::{BuildingKindId, SkillId};
use spirit_sim::save::{Loaded, SaveError, SaveStore, Slot};
use spirit_sim::{Command, ScenarioKind, World};
use std::collections::VecDeque;

pub const SPEEDS: [u32; 4] = [1, 2, 4, 8];
/// Upper bound on ticks per frame so a slow frame can't snowball.
const MAX_TICKS_PER_FRAME: u32 = 200;
/// How long a milestone message stays up, in seconds.
const NOTICE_SECONDS: f32 = 3.5;
/// Extra time for each further line of a longer message.
const NOTICE_LINE_SECONDS: f32 = 3.0;
/// How long the reply to a command stays up.
const MESSAGE_SECONDS: f32 = 6.0;
/// Messages waiting to be shown. Older ones are skipped; the log has them.
const MAX_WAITING_NOTICES: usize = 3;

#[derive(Resource)]
pub struct Sim {
    pub world: World,
    pub scenario: ScenarioKind,
    pub paused: bool,
    pub speed: u32,
    accumulator: f32,
    /// Skill and XP amount selected in the Soul Separation panel.
    pub soul_skill: SkillId,
    pub soul_amount: u64,
    /// The next press of Separate Soul performs the ritual.
    pub soul_armed: bool,
    /// Feedback from the last player command.
    pub message: String,
    /// The name being typed for the selected spirit.
    pub naming: Option<String>,
    /// Asking whether to call the spirits home.
    pub calling_home: bool,
    /// How the player likes the game to look and sound.
    pub settings: crate::settings::Settings,
    /// Bumped when the world is replaced so the renderer rebuilds its entities.
    pub generation: u32,
    /// Large on-screen notice and its remaining seconds.
    pub banner: Option<(String, f32)>,
    notices_seen: u64,
    waiting_notices: VecDeque<String>,
    /// Finished crossings when the map was last drawn.
    crossings_drawn: usize,
    /// The message being timed, and how long it has been up.
    message_shown: String,
    message_age: f32,
    pub store: SaveStore,
    /// Real seconds between autosaves; 0 turns autosave off.
    pub autosave_seconds: f32,
    autosave_timer: f32,
    /// The title menu is open; the world is on hold.
    pub menu: bool,
    /// A real game has begun, so there's something worth saving.
    pub started: bool,
    /// The profile whose New Game button is waiting for a second press.
    pub confirm_new: Option<&'static str>,
    /// What the title menu says about the save, refreshed when it opens.
    pub save_summary: Option<String>,
    /// Why the last load failed.
    pub menu_error: Option<String>,
    /// The building the next map click will place.
    pub placing: Option<BuildingKindId>,
    /// The spirit shown in the Spirits tab.
    pub selected_spirit: usize,
}

impl Sim {
    fn build(world: World, scenario: ScenarioKind) -> Self {
        let store = SaveStore::new(SaveStore::default_dir());
        Self {
            soul_skill: world
                .defs
                .skill_id("woodcutting")
                .expect("woodcutting skill"),
            soul_amount: world.defs.soul_min_xp,
            soul_armed: false,
            paused: false,
            speed: Self::default_speed(scenario, &world),
            accumulator: 0.0,
            message: String::new(),
            naming: None,
            calling_home: false,
            settings: Default::default(),
            generation: 0,
            banner: None,
            notices_seen: world.notices.total,
            waiting_notices: VecDeque::new(),
            crossings_drawn: 0,
            message_shown: String::new(),
            message_age: 0.0,
            autosave_seconds: world.defs.autosave_seconds,
            autosave_timer: 0.0,
            store,
            menu: false,
            started: true,
            confirm_new: None,
            save_summary: None,
            menu_error: None,
            placing: None,
            selected_spirit: 0,
            world,
            scenario,
        }
    }

    fn default_speed(scenario: ScenarioKind, world: &World) -> u32 {
        if world.journey.is_some() {
            return 1;
        }
        match scenario {
            ScenarioKind::Demo
            | ScenarioKind::GroveSustainable
            | ScenarioKind::GroveOversubscribed
            | ScenarioKind::WorkshopAutomated
            | ScenarioKind::WorkshopConstruction => 4,
            _ => 1,
        }
    }

    /// A development scenario.
    pub fn new(scenario: ScenarioKind) -> Self {
        Self::build(World::from_scenario(scenario), scenario)
    }

    /// A real playthrough under a progression profile.
    pub fn journey(profile: &str) -> Result<Self, String> {
        Ok(Self::build(
            World::new_journey(profile)?,
            ScenarioKind::NewGame,
        ))
    }

    /// The game as it opens with nothing chosen yet: the title menu over a
    /// world that isn't being played.
    pub fn title_screen(profile: &str) -> Result<Self, String> {
        let mut sim = Self::journey(profile)?;
        sim.started = false;
        Ok(sim)
    }

    pub fn is_journey(&self) -> bool {
        self.world.journey.is_some()
    }

    /// Replaces the world, keeping saves and settings.
    fn adopt(&mut self, world: World, scenario: ScenarioKind) {
        self.soul_skill = world
            .defs
            .skill_id("woodcutting")
            .expect("woodcutting skill");
        self.soul_amount = world.defs.soul_min_xp;
        self.soul_armed = false;
        self.speed = Self::default_speed(scenario, &world);
        self.paused = false;
        self.accumulator = 0.0;
        self.generation += 1;
        self.banner = None;
        self.notices_seen = world.notices.total;
        self.waiting_notices.clear();
        self.autosave_timer = 0.0;
        self.world = world;
        self.scenario = scenario;
        self.started = true;
        self.confirm_new = None;
        self.menu_error = None;
        self.menu = false;
        self.placing = None;
        self.selected_spirit = 0;
    }

    /// Going to another world: a playthrough is kept before it's replaced.
    fn keep_progress(&mut self) {
        if self.is_journey() && self.started {
            self.write_autosave();
        }
    }

    pub fn restart(&mut self, scenario: ScenarioKind) {
        self.keep_progress();
        self.adopt(World::from_scenario(scenario), scenario);
        self.message = "Simulation restarted".into();
    }

    pub fn new_journey(&mut self, profile: &str) {
        match World::new_journey(profile) {
            Ok(world) => {
                self.adopt(world, ScenarioKind::NewGame);
                self.message = String::new();
                // The old save stays until this game first saves over it.
            }
            Err(e) => self.menu_error = Some(e),
        }
    }

    fn use_save(&mut self, result: Result<Loaded, SaveError>) {
        match result {
            Ok(loaded) => {
                let scenario =
                    ScenarioKind::from_key(&loaded.meta.scenario).unwrap_or(ScenarioKind::NewGame);
                let note = loaded.warning;
                self.adopt(loaded.world, scenario);
                self.message = note.unwrap_or_else(|| "Game loaded".into());
            }
            Err(e) => {
                self.menu_error = Some(e.to_string());
                self.message = e.to_string();
            }
        }
    }

    /// Loads the most recent save.
    pub fn continue_game(&mut self) {
        let result = self.store.load_newest();
        self.use_save(result);
    }

    /// Goes back to the last manual save.
    pub fn load_manual(&mut self) {
        let result = self.manual_store().load(Slot::Manual);
        self.use_save(result);
    }

    pub fn save_game(&mut self) {
        let store = self.manual_store();
        self.message = match store.write(Slot::Manual, &self.world, self.scenario.key()) {
            Ok(()) => {
                self.autosave_timer = 0.0;
                if self.is_journey() {
                    "Game saved".into()
                } else {
                    "Scenario saved. Your own game's save is untouched.".into()
                }
            }
            Err(e) => e.to_string(),
        };
    }

    /// Where Save and Load go. A scenario keeps its saves in a folder of its
    /// own, so trying one can never overwrite the game you're playing.
    fn manual_store(&self) -> SaveStore {
        if self.is_journey() {
            self.store.clone()
        } else {
            let dir = self.store.dir().join("scenarios").join(self.scenario.key());
            SaveStore::new(dir)
        }
    }

    fn write_autosave(&mut self) {
        if let Err(e) = self
            .store
            .write(Slot::Auto, &self.world, self.scenario.key())
        {
            self.message = e.to_string();
        }
    }

    pub fn open_menu(&mut self) {
        self.keep_progress();
        self.confirm_new = None;
        self.refresh_save_summary();
        self.menu = true;
    }

    pub fn refresh_save_summary(&mut self) {
        let tps = self.world.defs.ticks_per_second as u64;
        self.save_summary = self.store.newest().map(|(_, m)| {
            let secs = m.tick / tps;
            format!(
                "Saved game: {:02}:{:02}:{:02} played ({} pace)",
                secs / 3600,
                secs / 60 % 60,
                secs % 60,
                m.profile
            )
        });
    }

    /// Performs a Soul Separation and, on success, shows what was given up.
    pub fn separate_soul(&mut self, skill: SkillId, xp: u64) {
        let preview = self.world.soul_preview(skill, xp);
        let spirits = self.world.spirits.len();
        self.command(Command::SoulSeparate { skill, xp });
        if self.world.spirits.len() == spirits {
            return;
        }
        // The chosen amount is spent; start the next one from the minimum.
        self.soul_amount = self.world.defs.soul_min_xp;
        let w = &self.world;
        let s = &w.spirits[spirits];
        let skill_name = &w.defs.skill(skill).name;
        let mut text = format!(
            "SOUL SEPARATION
{skill_name} level {} -> {}   (-{xp} XP)
{} awakens: permanent {skill_name} level {}",
            preview.current_level,
            preview.after_level,
            s.name,
            s.level()
        );
        if !preview.lost.is_empty() {
            let lost: Vec<String> = preview
                .lost
                .iter()
                .map(|k| {
                    let d = w.defs.tree(*k);
                    format!("{} (level {})", d.name, d.level_required)
                })
                .collect();
            text += &format!(
                "
You can no longer cut {} - retrain to regain it",
                lost.join(", ")
            );
        }
        if !preview.lost_recipes.is_empty() {
            let lost: Vec<String> = preview
                .lost_recipes
                .iter()
                .map(|r| {
                    let d = w.defs.recipe(*r);
                    format!("{} (level {})", d.name, d.level_required)
                })
                .collect();
            text += &format!(
                "
You can no longer make {} - retrain to regain it",
                lost.join(", ")
            );
        }
        self.banner = Some((text, 6.0));
    }

    /// Writes the play report beside the saves.
    pub fn write_report(&mut self) {
        let path = self.store.dir().join("play_report.txt");
        let dir = std::fs::create_dir_all(self.store.dir());
        self.message = match dir.and_then(|_| std::fs::write(&path, self.world.play_report())) {
            Ok(()) => format!("Play report written to {}", path.display()),
            Err(e) => format!("Couldn't write the play report: {e}"),
        };
    }

    pub fn command(&mut self, cmd: Command) {
        self.message = match self.world.apply(cmd) {
            Ok(msg) => msg,
            Err(err) => format!("Can't do that: {err}"),
        };
    }
}

pub struct SimRunnerPlugin;

impl Plugin for SimRunnerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                keyboard_controls,
                advance_simulation,
                redraw_changed_land,
                show_notices,
                expire_message,
                autosave,
            )
                .chain(),
        )
        .add_systems(Last, save_on_exit);
    }
}

/// A finished crossing changes the land itself, so the map is drawn again.
fn redraw_changed_land(mut sim: ResMut<Sim>) {
    let w = &sim.world;
    let open = w
        .buildings
        .iter()
        .filter(|b| {
            let def = w.defs.building(b.kind);
            b.is_complete()
                && !(def.opens.is_empty() && def.paves.is_empty() && def.plants.is_empty())
        })
        .count();
    if open != sim.crossings_drawn {
        sim.crossings_drawn = open;
        sim.generation += 1;
    }
}

fn advance_simulation(time: Res<Time>, mut sim: ResMut<Sim>) {
    if sim.paused || sim.menu {
        return;
    }
    let tps = sim.world.defs.ticks_per_second as f32;
    sim.accumulator += time.delta_secs() * tps * sim.speed as f32;
    let ticks = (sim.accumulator.floor() as u32).min(MAX_TICKS_PER_FRAME);
    sim.accumulator = (sim.accumulator - ticks as f32).min(MAX_TICKS_PER_FRAME as f32);
    for _ in 0..ticks {
        sim.world.step();
    }
}

/// Clears the reply to a command once it's had time to be read, so an old
/// answer never sits beside what's happening now.
fn expire_message(time: Res<Time>, mut sim: ResMut<Sim>) {
    if sim.message != sim.message_shown {
        sim.message_shown = sim.message.clone();
        sim.message_age = 0.0;
    } else if !sim.message.is_empty() {
        sim.message_age += time.delta_secs();
        if sim.message_age >= MESSAGE_SECONDS {
            sim.message.clear();
        }
    }
}

/// Puts new milestone messages up one at a time.
fn show_notices(mut sim: ResMut<Sim>) {
    let total = sim.world.notices.total;
    let fresh = (total - sim.notices_seen.min(total)) as usize;
    if fresh > 0 {
        sim.notices_seen = total;
        let texts: Vec<String> = sim
            .world
            .notices
            .recent(fresh)
            .map(|n| n.text.clone())
            .collect();
        sim.waiting_notices.extend(texts.into_iter().rev());
        while sim.waiting_notices.len() > MAX_WAITING_NOTICES {
            sim.waiting_notices.pop_front();
        }
    }
    if sim.banner.is_none()
        && let Some(text) = sim.waiting_notices.pop_front()
    {
        let lines = text.lines().count().max(1) as f32;
        sim.banner = Some((text, NOTICE_SECONDS + (lines - 1.0) * NOTICE_LINE_SECONDS));
    }
}

fn autosave(time: Res<Time>, mut sim: ResMut<Sim>) {
    if sim.menu || !sim.started || !sim.is_journey() || sim.autosave_seconds <= 0.0 {
        return;
    }
    sim.autosave_timer += time.delta_secs();
    if sim.autosave_timer >= sim.autosave_seconds {
        sim.autosave_timer = 0.0;
        sim.write_autosave();
    }
}

/// Closing the window keeps the playthrough, however recently it last saved.
fn save_on_exit(mut exits: MessageReader<AppExit>, mut sim: ResMut<Sim>) {
    if exits.read().next().is_some() {
        sim.keep_progress();
    }
}

fn keyboard_controls(keys: Res<ButtonInput<KeyCode>>, mut sim: ResMut<Sim>) {
    if sim.menu {
        return;
    }
    if keys.just_pressed(KeyCode::Space) {
        sim.paused = !sim.paused;
    }
    for (key, speed) in [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
    ]
    .into_iter()
    .zip(SPEEDS)
    {
        if keys.just_pressed(key) {
            sim.speed = speed;
        }
    }
    if keys.just_pressed(KeyCode::F5) {
        sim.save_game();
    }
    if keys.just_pressed(KeyCode::F9) {
        sim.load_manual();
    }
    if keys.just_pressed(KeyCode::F6) {
        sim.write_report();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("spirit_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_scenario_never_saves_over_your_game() {
        let dir = temp_dir("scenario_save");
        let mut sim = Sim::new(ScenarioKind::KilnYard);
        sim.store = SaveStore::new(&dir);
        sim.save_game();
        assert!(sim.message.contains("untouched"), "{}", sim.message);
        assert!(!dir.join("world.save").exists());
        assert!(
            dir.join("scenarios")
                .join("kiln_yard")
                .join("world.save")
                .exists()
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_real_game_still_saves_where_it_always_did() {
        let dir = temp_dir("journey_save");
        let mut sim = Sim::journey("normal").unwrap();
        sim.store = SaveStore::new(&dir);
        sim.save_game();
        assert_eq!(sim.message, "Game saved");
        assert!(dir.join("world.save").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
