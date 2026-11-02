# Architecture and decisions

## Overview

```
          data/defs.toml, data/scenarios.toml, data/maps/forest.txt
                                   |
                                   v
 +-----------------------  spirit_sim (pure Rust)  -----------------------+
 | defs      parse and resolve TOML into indexed ids and ticks             |
 | grid      tile map, breadth-first pathfinding, distance fields          |
 | skills    shared XP table (xp -> level) for player and spirits          |
 | entities  trees, buildings, player, spirits, projects                   |
 | tasks     primitive tasks; the only code that moves resources           |
 | behavior  spirit job rules: condition -> plan -> task queue             |
 | planner   settlement planner and construction project upkeep           |
 | world     World::step() fixed tick, commands, conservation ledger       |
 | scenario  new game and demo setup from scenarios.toml                   |
 +-------------------------------------------------------------------------+
                  ^ Command                       | read-only state
                  |                               v
 +--------------  spirit_game (Bevy 0.20.1)  ------------------------------+
 | sim_runner  real-time pacing, pause/speed/restart                       |
 | render      sprites per tile, tree, building and actor; path gizmos      |
 | ui          panel text, buttons (bevy_ui_widgets Button + Activate)     |
 | capture     dev-only screenshot capture                                 |
 +-------------------------------------------------------------------------+
```

### Decisions

- **The simulation has no engine dependency.** `spirit_sim` doesn't use Bevy at all. This keeps tests fast (the full suite runs in under a second), makes determinism easy to guarantee, and protects game rules from Bevy's frequent API changes. Bevy only reads `World` and submits `Command`s.
- **Fixed integer ticks** (10 per second, set in data). All durations are converted from seconds to ticks at load time. Planner thresholds are integer percentages. Iteration order is always by id, and the simulation uses no randomness, so runs replay exactly. A `state_hash` test checks this.
- **Commands are applied between ticks.** Player input becomes a `Command` (`PlayerInteract`, `SoulSeparate`, `SetObjectiveTarget`, `SetAutonomousConstruction`), so a game is fully described by its scenario plus a list of commands.
- **Data-driven definitions.** Resources, skills, the XP table, tree types, spirit roles (including their prioritized job rules), buildings (cost, capacity, whether the planner may build them, count limit), objective and planner thresholds all live in `data/defs.toml`. The map is ASCII. Scenarios are data. Everything is embedded with `include_str!` so builds and tests don't depend on the working directory.
- **The behavior system is deliberately small.** Each role has a list of `{ job, priority, when }` rules. When a spirit's task queue is empty, it picks the highest-priority rule whose condition holds and which can actually be planned now (a tree is reachable, a storage has room, and so on). That job expands into a task queue (`Move`, `Chop`, `Deposit`, `Withdraw`, `DeliverToSite`, `Build`). If a task fails, the queue is cleared, the spirit is marked Blocked with a reason, and it re-plans shortly after. Jobs change because conditions change, not because of a fixed script.
- **The objective counts stock in transit.** "Maintain N logs in Main Storage" is measured as unreserved stock in Main Storage, plus outpost stock, plus logs carried by gatherers and haulers. Without this, a woodcutter would overfill the field storage while a hauler was already bringing logs in. Logs reserved for construction don't count, so a project draws stock down and the woodcutter goes back to work.
- **How the planner decides** (`planner.rs`):
  1. **Detect.** Every gatherer delivery records its delivery leg (tiles walked since the last log was cut), its chopping time and where the logs were cut. Over the last 3 recent deliveries, the planner flags a problem when walking takes at least 40% of the trip and the average leg is at least 12 tiles.
  2. **Evaluate.** Candidate sites are buildable tiles near the gathering spots of the *long* deliveries. The planner picks the site with the shortest average walk from those spots. A site must have at least two open sides and must not cut off any tree or building that is currently reachable. It must shorten the delivery leg by at least 50%.
  3. **Check.** Autonomous construction must be permitted (a UI toggle), the building's count limit must not be reached, and both a Construction Spirit and a Hauling Spirit must exist. A field storage is useless without a hauler to empty it.
  4. **Create.** A construction site is placed (it blocks the tile immediately) along with a project.
  5. **Upkeep, every tick.** The planner reserves the full cost in Main Storage all at once (all-or-nothing) and assigns a free builder. The builder physically carries the reserved logs to the site, then builds. On completion the logs are moved to the ledger's `consumed` total.
  6. **Adapt.** Gatherers deliver to the nearest completed storage that has room, so they switch routes on their own. Haulers move outpost stock to Main Storage.
- **Guards against duplicates, loops and circular dependencies:**
  - Only one active project at a time.
  - `max_count` per building type.
  - Sites are scored against the storage the gatherer actually used, so a new storage leaves no savings for a second one.
  - Delivery samples are cleared when a building completes.
  - Rejections start a cooldown and are logged once per kind of rejection.
  - Materials come only from completed storages, never from a building still under construction.
- **Rendering** uses plain sprites for tiles, trees, buildings and actors, and gizmos for paths, claimed trees and construction sites.
- **UI.** Bevy 0.20 removed the `Interaction` component, so buttons are `bevy_ui_widgets::Button` entities with `On<Activate>` observers.

## Resource and XP conservation

`Ledger` records logs placed by the scenario (`seeded`), logs cut (`harvested`)
and logs used by construction (`consumed`). `World::check_conservation` checks
`seeded + harvested == storages + inventories + construction sites + consumed`.
The headless demo and a test both check it every tick. Soul Separation moves an
exact amount of XP from the player to the new spirit, and there is no code path
that adds XP to a spirit.

## Known limitations

- **Hauling and Construction aren't trainable skills.** Those spirits are founding spirits seeded by the scenario, in both the normal game and the demo.
- **Only one autonomous building type (Field Storage) and one resource (Logs).** The definitions support more, but the planner's "detect" step only measures gatherer delivery distance.
- **Simplified movement.** Spirits move tile to tile on a 4-connected grid with BFS pathfinding and pass through each other. The return leg is assumed to equal the delivery leg when measuring travel share.
- **No save or load.** Restarting reproduces a scenario exactly; there are no mid-game saves.
- **Fixed camera and layout.** The view is tuned for the default 1600x920 window. Smaller windows clip the map under the panel. There is no zoom or pan.
- **Not verified by interacting with the running game.** The UI buttons (Soul Separation, objective, toggles) and map clicks call the same `Command`s that the tests exercise. The game was checked with automated screenshots of the demo, not by clicking through the UI.
- **Field Storage capacity.** If the hauler falls behind and the Field Storage fills up (40 logs), gatherers fall back to Main Storage. This isn't tuned.
- **Planner thresholds are tuned for this map.** They are configurable in `[planner]`.

## Questions for playtesting

- Is the moment the planner decides to build *legible*? Do players understand why it happened from the event log and the planner panel alone?
- Does Soul Separation feel like a meaningful trade-off? Losing levels to gain a permanent worker: is the 100 XP minimum right, and does a spirit's fixed level feel consequential (chopping speed)?
- Pacing: grove regrowth (4 min), objective defaults and the speeds needed before the planner acts in the normal game.
- Does the player want to direct the planner (approve or veto sites, choose placement) or just watch it?
- Is the gather → deliver → idle rhythm readable from the map (paths, claimed-tree rings) without reading the panel?
- What's the next problem to solve autonomously after field storage (for example roads, a second gatherer type, or storage full)?
