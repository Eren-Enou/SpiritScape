//! Stones the player has carved, kept beside the saves so the next new game
//! can find them. Like the settings, they belong to the player, not to a game.
//! A damaged file only ever means fewer stones.

use std::path::Path;

/// The file they're kept in, inside the save folder.
const FILE: &str = "stones.txt";
/// How many games' stones are kept.
const KEEP: usize = 3;
/// What separates one stone from the next in the file.
const BREAK: &str = "---";

/// Every stone kept, most recent first.
pub fn load(dir: &Path) -> Vec<Vec<String>> {
    let text = std::fs::read_to_string(dir.join(FILE)).unwrap_or_default();
    parse(&text)
}

fn parse(text: &str) -> Vec<Vec<String>> {
    let mut stones = Vec::new();
    let mut lines = Vec::new();
    for line in text.lines().map(str::trim) {
        if line == BREAK {
            if !lines.is_empty() {
                stones.push(std::mem::take(&mut lines));
            }
        } else if !line.is_empty() {
            lines.push(line.to_string());
        }
    }
    if !lines.is_empty() {
        stones.push(lines);
    }
    stones.truncate(KEEP);
    stones
}

fn write(stones: &[Vec<String>]) -> String {
    let blocks: Vec<String> = stones.iter().map(|s| s.join("\n")).collect();
    blocks.join(&format!("\n{BREAK}\n")) + "\n"
}

/// Keeps a newly carved stone, in front of the others. Carving the same
/// words again, as reloading a save could, keeps it once.
pub fn remember(dir: &Path, lines: &[String]) -> std::io::Result<()> {
    let mut stones = load(dir);
    if stones.first().is_some_and(|s| s == lines) {
        return Ok(());
    }
    stones.insert(0, lines.to_vec());
    stones.truncate(KEEP);
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join(FILE), write(&stones))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("spirit_stones_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn words(n: u32) -> Vec<String> {
        vec![format!("I gave {n} XP."), "I'm still here.".into()]
    }

    #[test]
    fn no_file_means_no_stones() {
        assert!(load(&dir("none")).is_empty());
    }

    #[test]
    fn the_newest_comes_first_and_only_three_are_kept() {
        let d = dir("order");
        for n in 1..=4 {
            remember(&d, &words(n)).unwrap();
        }
        assert_eq!(load(&d), vec![words(4), words(3), words(2)]);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn the_same_stone_twice_is_kept_once() {
        let d = dir("twice");
        remember(&d, &words(1)).unwrap();
        remember(&d, &words(1)).unwrap();
        assert_eq!(load(&d), vec![words(1)]);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_damaged_file_only_means_fewer_stones() {
        assert!(parse("---\n---\n\n").is_empty());
        assert_eq!(
            parse("one\n---\n\n---\ntwo\r\nthree"),
            vec![
                vec!["one".to_string()],
                vec!["two".to_string(), "three".to_string()],
            ]
        );
    }
}
