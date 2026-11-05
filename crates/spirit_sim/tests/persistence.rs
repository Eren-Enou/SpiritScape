//! Saving and loading: a loaded world must be the saved world, damaged files
//! must be refused cleanly, and a save must never lose or invent anything.

use spirit_sim::defs::JobKind;
use spirit_sim::entities::{ProjectState, TreeState};
use spirit_sim::save::{self, SAVE_VERSION, SaveError, SaveStore, Slot};
use spirit_sim::tasks::Task;
use spirit_sim::{Command, ScenarioKind, World};
use std::path::PathBuf;

fn reload(w: &World) -> World {
    save::decode(&save::encode(w, "test"))
        .expect("save loads")
        .0
}

fn body(text: &str) -> &str {
    text.split_once('\n').unwrap().1
}

fn temp_store(name: &str) -> SaveStore {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("spiritscape_test_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    SaveStore::new(dir)
}

#[test]
fn every_scenario_round_trips_exactly() {
    for kind in ScenarioKind::ALL {
        let mut w = World::from_scenario(kind);
        w.run_ticks(1500);
        let loaded = reload(&w);
        assert_eq!(loaded.state_text(), w.state_text(), "{}", kind.key());
        assert_eq!(loaded.state_hash(), w.state_hash(), "{}", kind.key());
    }
}

#[test]
fn a_loaded_world_continues_exactly_like_the_original() {
    // Save at awkward moments: mid-step, mid-chop, mid-project.
    for kind in [
        ScenarioKind::Demo,
        ScenarioKind::WorkshopAutomated,
        ScenarioKind::WorkshopConstruction,
        ScenarioKind::GroveOversubscribed,
    ] {
        for save_at in [1, 137, 611, 1503] {
            let mut original = World::from_scenario(kind);
            original.run_ticks(save_at);
            let mut loaded = reload(&original);
            original.run_ticks(1200);
            loaded.run_ticks(1200);
            assert_eq!(
                loaded.state_text(),
                original.state_text(),
                "{} saved at {save_at}",
                kind.key()
            );
        }
    }
}

#[test]
fn saving_repeatedly_does_not_disturb_a_run() {
    let mut plain = World::from_scenario(ScenarioKind::WorkshopConstruction);
    let mut saved = plain.clone();
    for _ in 0..12 {
        plain.run_ticks(100);
        saved.run_ticks(100);
        saved = reload(&saved);
    }
    assert_eq!(saved.state_text(), plain.state_text());
    saved.check_conservation().unwrap();
}

#[test]
fn resources_and_xp_are_conserved_across_loading() {
    let mut w = World::from_scenario(ScenarioKind::WorkshopAutomated);
    w.run_ticks(900);
    let logs = w.defs.resource_id("logs").unwrap();
    let planks = w.defs.resource_id("planks").unwrap();
    let before = (w.resource_in_world(logs), w.resource_in_world(planks));
    let xp = (w.ledger.xp_earned, w.ledger.xp_separated);
    let mut l = reload(&w);
    assert_eq!(
        (l.resource_in_world(logs), l.resource_in_world(planks)),
        before
    );
    assert_eq!((l.ledger.xp_earned, l.ledger.xp_separated), xp);
    for _ in 0..1500 {
        l.step();
        l.check_conservation().unwrap();
    }
}

#[test]
fn spirits_keep_their_jobs_and_fixed_competency() {
    let mut w = World::from_scenario(ScenarioKind::WorkshopAutomated);
    w.run_ticks(400);
    let busy = w
        .spirits
        .iter()
        .filter(|s| !s.actor.queue.is_empty())
        .count();
    assert!(busy > 0);
    let mut l = reload(&w);
    for (a, b) in w.spirits.iter().zip(&l.spirits) {
        assert_eq!(a.actor.queue, b.actor.queue);
        assert_eq!(a.job, b.job);
        assert_eq!(
            (a.xp(), a.level(), a.skill()),
            (b.xp(), b.level(), b.skill())
        );
        assert_eq!(a.units_gathered, b.units_gathered);
    }
    let gathered: u32 = l.spirits.iter().map(|s| s.units_gathered).sum();
    l.apply(Command::SetObjectiveTarget(250)).unwrap();
    l.run_ticks(600);
    assert!(l.spirits.iter().map(|s| s.units_gathered).sum::<u32>() > gathered);
    for (a, b) in w.spirits.iter().zip(&l.spirits) {
        assert_eq!(a.xp(), b.xp(), "spirits never gain XP, loaded or not");
    }
}

#[test]
fn construction_and_its_reservation_survive_loading() {
    let mut w = World::from_scenario(ScenarioKind::WorkshopConstruction);
    for _ in 0..4000 {
        if w.projects[0].state == ProjectState::Supplying {
            break;
        }
        w.step();
    }
    assert_eq!(w.projects[0].state, ProjectState::Supplying);
    let logs = w.defs.resource_id("logs").unwrap();
    let mut l = reload(&w);
    assert_eq!(
        l.main().storage.reserved(logs),
        w.main().storage.reserved(logs)
    );
    for _ in 0..4000 {
        if l.projects[0].state == ProjectState::Complete {
            break;
        }
        l.step();
    }
    assert_eq!(l.projects[0].state, ProjectState::Complete);
    assert_eq!(l.main().storage.capacity, 450);
    l.check_conservation().unwrap();
}

#[test]
fn loading_does_not_simulate_time_away() {
    let mut w = World::from_scenario(ScenarioKind::GroveOversubscribed);
    w.run_ticks(800);
    let depleted = |w: &World| {
        w.trees
            .iter()
            .filter(|t| matches!(t.state, TreeState::Depleted { .. }))
            .count()
    };
    assert!(depleted(&w) > 0);
    let text = save::encode(&w, "test");
    std::thread::sleep(std::time::Duration::from_millis(20));
    let (l, meta) = save::decode(&text).unwrap();
    assert_eq!(l.tick, w.tick);
    assert_eq!(meta.tick, w.tick);
    assert_eq!(depleted(&l), depleted(&w));
    assert_eq!(l.ledger.harvested, w.ledger.harvested);
}

#[test]
fn settings_survive_loading() {
    let mut w = World::from_scenario(ScenarioKind::WorkshopAutomated);
    let grove = w.defs.site_id("starting_grove").unwrap();
    w.apply(Command::SetProductionTarget(77)).unwrap();
    w.apply(Command::SetInputReserve(13)).unwrap();
    w.apply(Command::SetProductionEnabled(false)).unwrap();
    w.apply(Command::SetTrainingArea(Some(grove))).unwrap();
    w.apply(Command::SetTrainingFirst(true)).unwrap();
    let l = reload(&w);
    let o = &l.settlement.order;
    assert_eq!((o.target, o.input_reserve, o.enabled), (77, 13, false));
    assert_eq!(l.settlement.training, w.settlement.training);
    assert!(!l.settlement.site_open[grove.index()]);
}

#[test]
fn player_progress_survives_loading() {
    let mut w = World::from_scenario(ScenarioKind::Regrind);
    let skill = w.defs.skill_id("woodcutting").unwrap();
    w.soul_separate(skill, 200).unwrap();
    let tree = w.trees.iter().find(|t| w.player_can_cut(t.kind)).unwrap().pos;
    w.apply(Command::PlayerInteract(tree)).unwrap();
    w.run_ticks(120);
    let l = reload(&w);
    assert_eq!(l.player.xp(skill), w.player.xp(skill));
    assert_eq!(l.player.history(skill), w.player.history(skill));
    assert_eq!(l.player.actor.pos(), w.player.actor.pos());
    assert_eq!(
        l.player.actor.inventory.items(),
        w.player.actor.inventory.items()
    );
    assert_eq!(l.player.actor.queue, w.player.actor.queue);
}

#[test]
fn profile_and_guidance_come_back_with_a_journey() {
    let mut w = World::new_journey("normal").unwrap();
    w.run_ticks(50);
    let (l, meta) = save::decode(&save::encode(&w, "new_game")).unwrap();
    assert_eq!(meta.profile, "normal");
    assert_eq!(meta.scenario, "new_game");
    assert_eq!(l.defs.profile.id, "normal");
    assert_eq!(l.defs.soul_min_xp, w.defs.soul_min_xp);
    assert_eq!(l.journey, w.journey);
}

#[test]
fn damaged_files_are_refused_without_panicking() {
    let mut w = World::from_scenario(ScenarioKind::Demo);
    w.run_ticks(300);
    let good = save::encode(&w, "demo");
    let corrupt = |text: &str| matches!(save::decode(text), Err(SaveError::Corrupt(_)));

    assert!(corrupt(""));
    assert!(corrupt("not a save"));
    assert!(corrupt("garbage header\n{}"));
    assert!(corrupt(&good[..good.len() / 2]), "a truncated file");
    let flipped = good.replacen("\"tick\":300", "\"tick\":301", 1);
    assert_ne!(flipped, good);
    assert!(corrupt(&flipped), "a changed byte");
    assert!(
        corrupt(&save::seal(SAVE_VERSION, "{\"meta\":1}")),
        "wrong shape"
    );
}

#[test]
fn saves_from_other_versions_are_incompatible_not_corrupt() {
    let w = World::from_scenario(ScenarioKind::Demo);
    let good = save::encode(&w, "demo");
    let newer = save::seal(SAVE_VERSION + 1, body(&good));
    let e = save::decode(&newer).err().unwrap();
    assert!(matches!(e, SaveError::Incompatible(_)), "{e}");
    assert!(e.to_string().contains("format"));
}

#[test]
fn a_save_from_different_game_data_is_refused() {
    let w = World::from_scenario(ScenarioKind::Demo);
    let good = save::encode(&w, "demo");
    let moved = body(&good).replacen("\"logs\",\"planks\"", "\"planks\",\"logs\"", 1);
    assert_ne!(moved, body(&good));
    let e = save::decode(&save::seal(SAVE_VERSION, &moved))
        .err()
        .unwrap();
    assert!(matches!(e, SaveError::Incompatible(_)), "{e}");

    let unknown = body(&good).replacen("\"profile\":\"development\"", "\"profile\":\"hard\"", 1);
    let e = save::decode(&save::seal(SAVE_VERSION, &unknown))
        .err()
        .unwrap();
    assert!(matches!(e, SaveError::Incompatible(_)), "{e}");
}

#[test]
fn a_well_formed_save_with_impossible_contents_is_refused() {
    let mut w = World::from_scenario(ScenarioKind::WorkshopAutomated);
    w.run_ticks(100);
    let good = save::encode(&w, "x");
    // The checksum is fine, but this points at a building that doesn't exist.
    let bad_ref = body(&good).replacen("\"main_storage\":", "\"main_storage\":9", 1);
    let e = save::decode(&save::seal(SAVE_VERSION, &bad_ref))
        .err()
        .unwrap();
    assert!(matches!(e, SaveError::Corrupt(_)), "{e}");

    // Logs from nowhere fail the conservation check.
    let mut duped = w.clone();
    let main = duped.main_storage;
    let logs = duped.defs.resource_id("logs").unwrap();
    duped.buildings[main].storage.capacity += 500;
    duped.buildings[main].storage.deposit(logs, 500);
    let e = save::decode(&save::encode(&duped, "x")).err().unwrap();
    assert!(matches!(e, SaveError::Corrupt(_)), "{e}");
}

#[test]
fn the_store_replaces_saves_and_keeps_a_backup() {
    let store = temp_store("replace");
    let mut w = World::from_scenario(ScenarioKind::Demo);
    assert!(matches!(store.load(Slot::Manual), Err(SaveError::NotFound)));
    assert!(store.newest().is_none());

    store.write(Slot::Manual, &w, "demo").unwrap();
    let first_tick = w.tick;
    w.run_ticks(50);
    store.write(Slot::Manual, &w, "demo").unwrap();
    assert_eq!(store.load(Slot::Manual).unwrap().world.tick, w.tick);
    let names: Vec<String> = std::fs::read_dir(store.dir())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(names.contains(&"world.save".to_string()));
    assert!(names.contains(&"world.save.bak".to_string()));
    assert!(!names.iter().any(|n| n.ends_with(".tmp")), "{names:?}");

    // A damaged save falls back to the previous one and says so.
    std::fs::write(store.dir().join("world.save"), "half a fi").unwrap();
    let loaded = store.load(Slot::Manual).unwrap();
    assert_eq!(loaded.world.tick, first_tick);
    assert!(loaded.warning.unwrap().contains("previous save"));
    let _ = std::fs::remove_dir_all(store.dir());
}

#[test]
fn an_interrupted_write_leaves_the_last_save_usable() {
    let store = temp_store("interrupted");
    let w = World::from_scenario(ScenarioKind::Demo);
    store.write(Slot::Auto, &w, "demo").unwrap();
    // What a crash mid-write leaves behind: a partial temporary file.
    std::fs::write(store.dir().join("autosave.save.tmp"), "SPIRITSCAPE-SA").unwrap();
    assert_eq!(store.load(Slot::Auto).unwrap().world.tick, w.tick);
    // The next write simply replaces it.
    store.write(Slot::Auto, &w, "demo").unwrap();
    assert!(!store.dir().join("autosave.save.tmp").exists());
    let _ = std::fs::remove_dir_all(store.dir());
}

#[test]
fn continue_finds_the_most_recent_usable_save() {
    let store = temp_store("newest");
    let mut w = World::from_scenario(ScenarioKind::Demo);
    store.write(Slot::Manual, &w, "demo").unwrap();
    w.run_ticks(40);
    store.write(Slot::Auto, &w, "demo").unwrap();
    assert_eq!(store.load_newest().unwrap().world.tick, 40);

    // Both the autosave and its backup gone bad: the manual save still works.
    std::fs::write(store.dir().join("autosave.save"), "junk").unwrap();
    let _ = std::fs::remove_file(store.dir().join("autosave.save.bak"));
    assert_eq!(store.load_newest().unwrap().world.tick, 0);
    let _ = std::fs::remove_dir_all(store.dir());
}

#[test]
fn a_hauler_mid_trip_finishes_its_delivery_after_loading() {
    let mut w = World::from_scenario(ScenarioKind::WorkshopAutomated);
    let hauler = w
        .spirits
        .iter()
        .position(|s| w.defs.role(s.role).has_job(JobKind::Supply))
        .unwrap();
    for _ in 0..3000 {
        let s = &w.spirits[hauler];
        if s.actor.inventory.total() > 0 && matches!(s.actor.queue.front(), Some(Task::Move { .. }))
        {
            break;
        }
        w.step();
    }
    let carried = w.spirits[hauler].actor.inventory.total();
    assert!(carried > 0);
    let mut l = reload(&w);
    let delivered = l.spirits[hauler].units_delivered;
    l.run_ticks(300);
    assert!(l.spirits[hauler].units_delivered >= delivered + carried);
    l.check_conservation().unwrap();
}
