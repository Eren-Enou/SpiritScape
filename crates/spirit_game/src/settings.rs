//! What the player has chosen about how the game looks and sounds. It belongs
//! to the player rather than to a game, so it's kept beside the saves and
//! survives starting a new one.

use std::path::Path;

/// The file it's kept in, inside the save folder.
const FILE: &str = "settings.txt";

/// Loudness steps, as a percentage.
const VOLUMES: [u32; 4] = [100, 75, 50, 25];
/// Text and panel size steps, as a percentage.
const TEXT_SIZES: [u32; 3] = [100, 115, 130];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    pub volume: u32,
    pub text_size: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            volume: VOLUMES[0],
            text_size: TEXT_SIZES[0],
        }
    }
}

/// The step after `now`, going back to the first after the last.
fn next(steps: &[u32], now: u32) -> u32 {
    let at = steps.iter().position(|s| *s == now).unwrap_or(0);
    steps[(at + 1) % steps.len()]
}

impl Settings {
    pub fn next_volume(&mut self) {
        self.volume = next(&VOLUMES, self.volume);
    }

    pub fn next_text_size(&mut self) {
        self.text_size = next(&TEXT_SIZES, self.text_size);
    }

    pub fn loudness(&self) -> f32 {
        self.volume as f32 / 100.0
    }

    pub fn scale(&self) -> f32 {
        self.text_size as f32 / 100.0
    }

    /// Reads the file's text. Anything it can't make sense of stays as it was
    /// by default: a damaged settings file must never stop the game.
    fn parse(text: &str) -> Self {
        let mut settings = Self::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let Ok(value) = value.trim().parse::<u32>() else {
                continue;
            };
            match key.trim() {
                "volume" if VOLUMES.contains(&value) => settings.volume = value,
                "text_size" if TEXT_SIZES.contains(&value) => settings.text_size = value,
                _ => {}
            }
        }
        settings
    }

    fn text(&self) -> String {
        format!("volume={}\ntext_size={}\n", self.volume, self.text_size)
    }

    pub fn load(dir: &Path) -> Self {
        let text = std::fs::read_to_string(dir.join(FILE)).unwrap_or_default();
        Self::parse(&text)
    }

    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        std::fs::write(dir.join(FILE), self.text())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_was_written_reads_back() {
        let mut s = Settings::default();
        s.next_volume();
        s.next_text_size();
        assert_eq!(Settings::parse(&s.text()), s);
    }

    #[test]
    fn each_setting_comes_round_again() {
        let mut s = Settings::default();
        for _ in 0..VOLUMES.len() {
            s.next_volume();
        }
        for _ in 0..TEXT_SIZES.len() {
            s.next_text_size();
        }
        assert_eq!(s, Settings::default());
    }

    #[test]
    fn a_damaged_file_gives_the_defaults() {
        for text in [
            "",
            "rubbish",
            "volume=loud",
            "volume=3\ntext_size=9000",
            "=\n==",
        ] {
            assert_eq!(Settings::parse(text), Settings::default(), "{text:?}");
        }
    }

    #[test]
    fn a_half_damaged_file_keeps_the_good_half() {
        let s = Settings::parse("volume=50\ntext_size=huge\n");
        assert_eq!(s.volume, 50);
        assert_eq!(s.text_size, 100);
    }

    #[test]
    fn it_survives_on_disk() {
        let dir = std::env::temp_dir().join(format!("spirit_settings_{}", std::process::id()));
        let mut s = Settings::default();
        s.next_volume();
        s.save(&dir).unwrap();
        assert_eq!(Settings::load(&dir), s);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn no_file_at_all_gives_the_defaults() {
        let nowhere = Path::new("this/folder/does/not/exist");
        assert_eq!(Settings::load(nowhere), Settings::default());
    }
}
