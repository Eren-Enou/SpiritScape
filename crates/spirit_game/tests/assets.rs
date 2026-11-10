//! Everything the game draws has to be named in the visuals file, and every
//! model it names has to exist. A gap here fails nothing until the game tries
//! to draw it: Quarrying, Masonry and Kiln Spirits were invisible for a long
//! time because nothing checked.

use std::path::Path;

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the repository root")
}

fn visuals() -> String {
    std::fs::read_to_string(root().join("data/visuals.toml")).unwrap()
}

/// The ids of everything in a list in the game's definitions.
fn ids_of(list: &str) -> Vec<String> {
    let text = std::fs::read_to_string(root().join("data/defs.toml")).unwrap();
    let text = text.replace("\r\n", "\n");
    let head = format!("[[{list}]]\nid = \"");
    let found = text.split(&head).skip(1);
    found
        .map(|rest| rest.split('"').next().unwrap().to_string())
        .collect()
}

#[test]
fn every_model_in_the_visuals_file_exists() {
    let text = visuals();
    let models: Vec<&str> = text.split('"').filter(|s| s.ends_with(".glb")).collect();
    assert!(models.len() > 30, "found only {}", models.len());
    for model in models {
        let path = root().join("assets").join(model);
        assert!(path.exists(), "{model} is missing");
    }
}

#[test]
fn every_kind_of_spirit_has_a_body() {
    let roles = ids_of("roles");
    assert!(roles.len() >= 7, "{roles:?}");
    for id in roles {
        let section = format!("[characters.roles.{id}]");
        assert!(visuals().contains(&section), "{id} would be invisible");
    }
}

#[test]
fn every_kind_of_tree_can_be_seen() {
    let trees = ids_of("tree_types");
    assert!(trees.len() >= 6, "{trees:?}");
    for id in trees {
        assert!(visuals().contains(&format!("[trees.{id}]")), "{id}");
    }
}

#[test]
fn every_building_can_be_seen() {
    let buildings = ids_of("buildings");
    assert!(buildings.len() >= 30, "{buildings:?}");
    for id in buildings {
        assert!(visuals().contains(&format!("[buildings.{id}]")), "{id}");
    }
}

/// A duplicated section once stopped the game at launch while every other
/// test passed. This reads the file the way the game does.
#[test]
fn the_visuals_file_reads_as_the_game_reads_it() {
    if let Err(e) = toml::from_str::<toml::Table>(&visuals()) {
        panic!("data/visuals.toml can't be read: {e}");
    }
}

#[test]
fn the_games_own_models_are_in_use() {
    let text = visuals();
    for name in [
        "kiln",
        "still_boat",
        "warm_wall",
        "chimney",
        "clay_bank",
        "clay_dug",
    ] {
        let wanted = format!("models/own/{name}.glb");
        assert!(text.contains(&wanted), "{wanted} isn't used");
    }
}
