//! Tile map, map parsing and breadth-first grid pathfinding.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap, VecDeque};

pub const BUILTIN_MAP: &str = include_str!("../../../data/maps/forest.txt");

#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Terrain {
    Grass,
    Water,
    Rock,
    Bridge,
    /// Plank road: quicker to walk on, and nothing can be built on it.
    Road,
}

impl Terrain {
    pub fn walkable(self) -> bool {
        matches!(self, Terrain::Grass | Terrain::Bridge | Terrain::Road)
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

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Grid {
    pub width: i32,
    pub height: i32,
    terrain: Vec<Terrain>,
    /// Tiles occupied by trees or buildings.
    occupied: Vec<bool>,
    /// How long a road tile takes to cross, as a percentage of any other tile.
    /// 0 until the first road is laid, and paths are plain shortest paths
    /// until then.
    #[serde(default)]
    road_pct: u32,
}

impl Grid {
    pub fn new(layout: &MapLayout) -> Self {
        Self {
            width: layout.width,
            height: layout.height,
            terrain: layout.terrain.clone(),
            occupied: vec![false; layout.terrain.len()],
            road_pct: 0,
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

    /// Turns a tile into a walkable crossing.
    pub fn open_crossing(&mut self, p: TilePos) {
        if self.in_bounds(p) {
            let i = self.idx(p);
            self.terrain[i] = Terrain::Bridge;
            self.occupied[i] = false;
        }
    }

    /// Lays road on open ground. Water, rock and bridges stay as they are, and
    /// anything standing on a tile stays too, so a road runs around it.
    pub fn pave(&mut self, p: TilePos, road_pct: u32) {
        if !self.in_bounds(p) {
            return;
        }
        let i = self.idx(p);
        if self.terrain[i] == Terrain::Grass && !self.occupied[i] {
            self.terrain[i] = Terrain::Road;
            self.road_pct = road_pct;
        }
    }

    /// Ticks to enter `p` for a walker who takes `ticks` on open ground.
    pub fn step_ticks(&self, p: TilePos, ticks: u32) -> u32 {
        if self.terrain(p) == Some(Terrain::Road) {
            ((ticks * self.road_pct + 50) / 100).max(1)
        } else {
            ticks
        }
    }

    /// This grid on a larger map, for older saves. Everything already on the
    /// old map stays exactly where it was.
    pub fn grown(&self, layout: &MapLayout) -> Grid {
        let mut bigger = Grid::new(layout);
        for y in 0..self.height.min(bigger.height) {
            for x in 0..self.width.min(bigger.width) {
                let p = TilePos::new(x, y);
                let (from, to) = (self.idx(p), bigger.idx(p));
                bigger.terrain[to] = self.terrain[from];
                bigger.occupied[to] = self.occupied[from];
            }
        }
        bigger.road_pct = self.road_pct;
        bigger
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

    /// The largest stretch of connected open ground, by tile. Ties go to
    /// whichever comes first on the map.
    pub fn mainland(&self) -> Vec<bool> {
        let mut seen = vec![false; self.terrain.len()];
        let mut best: Vec<usize> = Vec::new();
        for y in 0..self.height {
            for x in 0..self.width {
                let start = TilePos::new(x, y);
                if seen[self.idx(start)] || !self.is_walkable(start) {
                    continue;
                }
                seen[self.idx(start)] = true;
                let mut region = vec![self.idx(start)];
                let mut queue = VecDeque::from([start]);
                while let Some(p) = queue.pop_front() {
                    for n in self.walkable_neighbors(p).collect::<Vec<_>>() {
                        let i = self.idx(n);
                        if !seen[i] {
                            seen[i] = true;
                            region.push(i);
                            queue.push_back(n);
                        }
                    }
                }
                if region.len() > best.len() {
                    best = region;
                }
            }
        }
        let mut land = vec![false; self.terrain.len()];
        for i in best {
            land[i] = true;
        }
        land
    }

    /// Whether `p` is on that ground.
    pub fn on_land(&self, land: &[bool], p: TilePos) -> bool {
        self.in_bounds(p) && land[self.idx(p)]
    }

    /// The tile of that ground closest to `p` in a straight line.
    pub fn nearest_land(&self, land: &[bool], p: TilePos) -> Option<TilePos> {
        (0..self.height)
            .flat_map(|y| (0..self.width).map(move |x| TilePos::new(x, y)))
            .filter(|t| land[self.idx(*t)])
            .min_by_key(|t| (t.manhattan(p), t.y, t.x))
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
        if self.road_pct > 0 {
            return self.quickest_path(from, goal);
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

    /// The quickest route once roads exist, which can be longer in tiles than
    /// the shortest one. Ties go to whichever was found first, so it repeats.
    fn quickest_path(&self, from: TilePos, goal: impl Fn(TilePos) -> bool) -> Option<Vec<TilePos>> {
        if !self.in_bounds(from) {
            return None;
        }
        let mut best = vec![u32::MAX; self.terrain.len()];
        let mut prev: Vec<Option<TilePos>> = vec![None; self.terrain.len()];
        let mut open = BinaryHeap::new();
        let mut found = 0u32;
        best[self.idx(from)] = 0;
        open.push(Reverse((0u32, found, from)));
        while let Some(Reverse((cost, _, p))) = open.pop() {
            if cost > best[self.idx(p)] {
                continue;
            }
            if p != from && goal(p) {
                let mut path = vec![p];
                let mut cur = p;
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
            for n in p.neighbors4() {
                if !self.is_walkable(n) {
                    continue;
                }
                let step = if self.terrain[self.idx(n)] == Terrain::Road {
                    self.road_pct
                } else {
                    100
                };
                let i = self.idx(n);
                if cost + step < best[i] {
                    best[i] = cost + step;
                    prev[i] = Some(p);
                    found += 1;
                    open.push(Reverse((cost + step, found, n)));
                }
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
