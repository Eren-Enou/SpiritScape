//! Centralized XP-to-level progression shared by every skill.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XpTable {
    /// `thresholds[i]` is the cumulative XP needed for level `i + 1`.
    thresholds: Vec<u64>,
}

impl XpTable {
    pub fn new(thresholds: Vec<u64>) -> Result<Self, String> {
        if thresholds.first() != Some(&0) {
            return Err("xp_table must start with 0 (level 1)".into());
        }
        if thresholds.windows(2).any(|w| w[1] <= w[0]) {
            return Err("xp_table must be strictly increasing".into());
        }
        Ok(Self { thresholds })
    }

    pub fn max_level(&self) -> u32 {
        self.thresholds.len() as u32
    }

    pub fn level_for_xp(&self, xp: u64) -> u32 {
        // Number of thresholds that are <= xp; always >= 1 because thresholds[0] == 0.
        self.thresholds.partition_point(|&t| t <= xp) as u32
    }

    /// Cumulative XP needed to reach `level` (clamped to the table).
    pub fn xp_for_level(&self, level: u32) -> u64 {
        let idx = (level.max(1) as usize - 1).min(self.thresholds.len() - 1);
        self.thresholds[idx]
    }

    /// XP threshold of the next level, or `None` at max level.
    pub fn next_level_xp(&self, xp: u64) -> Option<u64> {
        self.thresholds.get(self.level_for_xp(xp) as usize).copied()
    }
}
