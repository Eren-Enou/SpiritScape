//! Tile map, map parsing and breadth-first grid pathfinding.

use std::collections::{BTreeMap, VecDeque};

pub const BUILTIN_MAP: &str = include_str!("../../../data/maps/forest.txt");

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TilePos {
    pub x: i32,
    pub y: i32,
}

impl TilePos {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    pub fn manhattan(self, other: TilePos) -> u32 {
        self.x.abs_diff(other.x) + self.y.abs_diff(other.y)
    }

    pub fn is_adjacent(self, other: TilePos) -> bool {
        self.manhattan(other) == 1
    }

    /// 4-neighbours in a fixed order (N, E, S, W) for deterministic search.
    pub fn neighbors4(self) -> [TilePos; 4] {
        [
            TilePos::new(self.x, self.y - 1),
            TilePos::new(self.x + 1, self.y),
            TilePos::new(self.x, self.y + 1),
            TilePos::new(self.x - 1, self.y),
        ]
    }
}

impl std::fmt::Display for TilePos {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "({}, {})", self.x, self.y)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Terrain {
    Grass,
    Water,
    Rock,
    Bridge,
}

impl Terrain {
    pub fn walkable(self) -> bool {
        matches!(self, Terrain::Grass | Terrain::Bridge)
    }

    pub fn buildable(self) -> bool {
        matches!(self, Terrain::Grass)
    }
}

/// A parsed map: terrain plus the positions of every non-terrain marker.
///
/// Legend: `.` grass, `~` water, `#` rock, `=` bridge. Any other character is
/// a marker placed on grass (trees, buildings and spawn points are resolved
/// by the scenario using the definitions).
#[derive(Clone, Debug)]
pub struct MapLayout {
    pub width: i32,
    pub height: i32,
    pub terrain: Vec<Terrain>,
    pub markers: BTreeMap<char, Vec<TilePos>>,
}

impl MapLayout {
    pub fn parse(text: &str) -> Result<Self, String> {
        let rows: Vec<&str> = text
            .lines()
            .map(str::trim_end)
            .filter(|l| !l.is_empty())
            .collect();
        let height = rows.len() as i32;
        let width = rows.first().map(|r| r.chars().count()).unwrap_or(0) as i32;
        if width == 0 || height == 0 {
            return Err("map is empty".into());
        }
        let mut terrain = Vec::with_capacity((width * height) as usize);
        let mut markers: BTreeMap<char, Vec<TilePos>> = BTreeMap::new();
        for (y, row) in rows.iter().enumerate() {
            if row.chars().count() as i32 != width {
                return Err(format!("map row {y} has a different width"));
            }
            for (x, c) in row.chars().enumerate() {
                let t = match c {
                    '.' => Terrain::Grass,
                    '~' => Terrain::Water,
                    '#' => Terrain::Rock,
                    '=' => Terrain::Bridge,
                    other => {
                        markers
                            .entry(other)
                            .or_default()
                            .push(TilePos::new(x as i32, y as i32));
                        Terrain::Grass
                    }
                };
                terrain.push(t);
            }
        }
        Ok(Self {
            width,
            height,
            terrain,
            markers,
        })
    }

    pub fn marker(&self, c: char) -> Option<TilePos> {
        self.markers.get(&c).and_then(|v| v.first().copied())
    }
}

/// Shortest-path distances (in tiles) from a set of source tiles.
#[derive(Clone, Debug)]
pub struct DistanceField {
    width: i32,
    height: i32,
    dist: Vec<u32>,
}

impl DistanceField {
    pub fn get(&self, p: TilePos) -> Option<u32> {
        if p.x < 0 || p.y < 0 || p.x >= self.width || p.y >= self.height {
            return None;
        }
        let d = self.dist[(p.y * self.width + p.x) as usize];
        (d != u32::MAX).then_some(d)
    }
}

#[derive(Clone, Debug)]
pub struct Grid {
    pub width: i32,
    pub height: i32,
    terrain: Vec<Terrain>,
    /// Tiles occupied by trees or buildings.
    occupied: Vec<bool>,
}

impl Grid {
    pub fn new(layout: &MapLayout) -> Self {
        Self {
            width: layout.width,
            height: layout.height,
            terrain: layout.terrain.clone(),
            occupied: vec![false; layout.terrain.len()],
        }
    }

    pub fn in_bounds(&self, p: TilePos) -> bool {
        p.x >= 0 && p.y >= 0 && p.x < self.width && p.y < self.height
    }

    fn idx(&self, p: TilePos) -> usize {
        (p.y * self.width + p.x) as usize
    }

    pub fn terrain(&self, p: TilePos) -> Option<Terrain> {
        self.in_bounds(p).then(|| self.terrain[self.idx(p)])
    }

    pub fn is_occupied(&self, p: TilePos) -> bool {
        self.in_bounds(p) && self.occupied[self.idx(p)]
    }

    pub fn set_occupied(&mut self, p: TilePos, occupied: bool) {
        if self.in_bounds(p) {
            let i = self.idx(p);
            self.occupied[i] = occupied;
        }
    }

    pub fn is_walkable(&self, p: TilePos) -> bool {
        self.in_bounds(p) && self.terrain[self.idx(p)].walkable() && !self.occupied[self.idx(p)]
    }

    pub fn is_buildable(&self, p: TilePos) -> bool {
        self.in_bounds(p) && self.terrain[self.idx(p)].buildable() && !self.occupied[self.idx(p)]
    }

    pub fn walkable_neighbors(&self, p: TilePos) -> impl Iterator<Item = TilePos> + '_ {
        p.neighbors4().into_iter().filter(|n| self.is_walkable(*n))
    }

    /// Multi-source BFS over walkable tiles. Sources are always expanded,
    /// even if they are not walkable themselves.
    pub fn distance_field(&self, sources: &[TilePos]) -> DistanceField {
        self.distance_field_with_block(sources, None)
    }

    /// Like [`Grid::distance_field`] but treats `extra_block` as occupied.
    pub fn distance_field_with_block(
        &self,
        sources: &[TilePos],
        extra_block: Option<TilePos>,
    ) -> DistanceField {
        let mut dist = vec![u32::MAX; self.terrain.len()];
        let mut queue = VecDeque::new();
        for &s in sources {
            if self.in_bounds(s) && dist[self.idx(s)] == u32::MAX {
                dist[self.idx(s)] = 0;
                queue.push_back(s);
            }
        }
        while let Some(p) = queue.pop_front() {
            let d = dist[self.idx(p)];
            for n in p.neighbors4() {
                if !self.is_walkable(n) || Some(n) == extra_block {
                    continue;
                }
                let i = self.idx(n);
                if dist[i] == u32::MAX {
                    dist[i] = d + 1;
                    queue.push_back(n);
                }
            }
        }
        DistanceField {
            width: self.width,
            height: self.height,
            dist,
        }
    }

    /// Distance field seeded from the walkable tiles around `target`
    /// (where an actor stands to interact with it).
    pub fn approach_field(&self, target: TilePos) -> DistanceField {
        let sources: Vec<TilePos> = self.walkable_neighbors(target).collect();
        self.distance_field(&sources)
    }

    /// Shortest distance from `field`'s sources to any interaction tile of `target`.
    pub fn approach_distance(&self, field: &DistanceField, target: TilePos) -> Option<u32> {
        self.walkable_neighbors(target)
            .filter_map(|n| field.get(n))
            .min()
    }

    /// BFS path from `from` to any tile satisfying `goal`. The returned path
    /// excludes `from` and is empty if `from` already satisfies the goal.
    pub fn find_path(&self, from: TilePos, goal: impl Fn(TilePos) -> bool) -> Option<Vec<TilePos>> {
        if goal(from) {
            return Some(Vec::new());
        }
        let mut prev: Vec<Option<TilePos>> = vec![None; self.terrain.len()];
        let mut seen = vec![false; self.terrain.len()];
        let mut queue = VecDeque::new();
        if !self.in_bounds(from) {
            return None;
        }
        seen[self.idx(from)] = true;
        queue.push_back(from);
        while let Some(p) = queue.pop_front() {
            for n in p.neighbors4() {
                if !self.is_walkable(n) || seen[self.idx(n)] {
                    continue;
                }
                seen[self.idx(n)] = true;
                prev[self.idx(n)] = Some(p);
                if goal(n) {
                    let mut path = vec![n];
                    let mut cur = n;
                    while let Some(pp) = prev[self.idx(cur)] {
                        if pp == from {
                            break;
                        }
                        path.push(pp);
                        cur = pp;
                    }
                    path.reverse();
                    return Some(path);
                }
                queue.push_back(n);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(text: &str) -> Grid {
        Grid::new(&MapLayout::parse(text).unwrap())
    }

    #[test]
    fn path_goes_around_water() {
        let g = grid("...\n.~.\n...\n");
        let path = g
            .find_path(TilePos::new(0, 1), |p| p == TilePos::new(2, 1))
            .unwrap();
        assert_eq!(path.len(), 4);
        assert_eq!(*path.last().unwrap(), TilePos::new(2, 1));
    }

    #[test]
    fn unreachable_returns_none() {
        let g = grid(".~.\n.~.\n");
        assert!(
            g.find_path(TilePos::new(0, 0), |p| p == TilePos::new(2, 0))
                .is_none()
        );
    }

    #[test]
    fn approach_distance_uses_adjacent_tiles() {
        let mut g = grid(".....\n");
        g.set_occupied(TilePos::new(4, 0), true);
        let f = g.distance_field(&[TilePos::new(0, 0)]);
        assert_eq!(g.approach_distance(&f, TilePos::new(4, 0)), Some(3));
    }
}
