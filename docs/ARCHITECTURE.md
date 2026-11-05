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
 | metrics   time use, site output, bottleneck report                      |
 | production  plank order, upstream demand, workshop status and report      |
 | training    support for the player's skilling: cache, bench, priorities   |
 | soul        comparing sacrifices; milestone wording                       |
 | journey     new-game guidance and milestone times                         |
 | save        versioned files, validation, atomic replacement               |
 | autoplay    a reference player, for tests and pacing measurements         |
 | scenario  new game and demo setup from scenarios.toml                   |
 +-------------------------------------------------------------------------+
                  ^ Command                       | read-only state
                  |                               v
 +--------------  spirit_game (Bevy 0.20.1)  ------------------------------+
 | sim_runner  real-time pacing, pause/speed/restart                       |
 | visuals     data/visuals.toml: which glTF model shows which object      |
 | world_view  3D terrain, trees, buildings, labels, path gizmos           |
 | characters  KayKit characters, retargeted animations, spirit look       |
 | camera      tilted follow camera, lighting, WASD/E/click input          |
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
  1. **Detect.** Every gatherer delivery records its delivery leg (tiles walked since the last log was cut), its chopping time and where the logs were cut. Over the last 3 recent deliveries, the planner flags a problem when walking takes at least 40% of the trip and the average leg is at least 16 tiles.
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
- **2.5D: grid rules, 3D presentation.** The simulation stays a 2D tile grid; tile (x, y) is drawn at world (x, 0, y) with one tile per world unit, which matches the Kenney models' 1-unit footprint. A perspective camera looks down at about 50 degrees and follows the player (or shows the whole map in overview). Mouse clicks are turned into tiles by intersecting the camera ray with the ground plane.
- **Keyboard movement is a simulation command.** `Command::PlayerStep` moves one tile. The front end re-sends it every frame while a key is held; the sim ignores repeats and, mid-step, queues the next tile so walking is continuous. This keeps keyboard play deterministic and replayable like everything else.
- **Models are data.** `data/visuals.toml` maps terrain, tree types, buildings (as lists of rotated parts, a construction-site look and a log pile), the player and each spirit role to models under `assets/models/`. Buildings rise out of the ground while being built, log piles grow with stock, trees shrink as they're cut and turn into stumps.
- **Character animation.** The KayKit character files contain no animations, so the glTF loader doesn't mark their bones as animation targets. Once a character spawns, the game adds an `AnimationPlayer` to its `Rig_Medium` root and tags each bone with the same name-path `AnimationTargetId` the loader would have used; clips from the shared Rig_Medium animation files (idle, walk, interact) then drive every character. An axe is attached to the `handslot.r` bone of the player and the woodcutter.
- **Tiers are a level requirement on tree types.** `level_required` is checked when the player clicks a tree, inside the chop task (so a chop stops if a Soul Separation drops the player below the requirement), and when spirits choose a tree. There's one rule for everyone, so losing a level always means losing the same access a spirit of that level would lack.
- **Soul Separation is previewed by the simulation.** `World::soul_preview` is the single source for the UI text, the banner and validation. The player keeps a lifetime `SkillHistory` (highest level, XP sacrificed, spirits created) next to current XP, so the cost stays visible after retraining.
- **Sites are map areas, not new entities.** Trees keep their existing deplete-and-regrow rules; a site groups trees, scales their regrowth, and can be closed to spirits. A site's output is limited only by regrowth, and nothing is ever permanently destroyed.
- **Gatherers stay put and move only when it pays.** A spirit keeps working its current site while it has a free tree it can cut. It only moves to another site that has a full load of free logs; otherwise it waits for regrowth and records why. This stops spirits ping-ponging whenever a single tree regrows.
- **Measurements are integer counters; rates are derived on read.** Each spirit adds one tick per tick to a time-use bucket chosen from its current task or wait reason, with snapshots every 10 s for a two-minute rolling window. Sites record harvest ticks. `gathering_report` turns these into rates, shares and a bottleneck, using fixed rules in priority order:
  1. idle more than half the time → demand met;
  2. waiting for regrowth at least 25% → resource recovery;
  3. blocked by storage, or an outpost at least 80% full → hauling or storage capacity;
  4. walking at least 40% → travel distance;
  5. otherwise → gathering speed.
- **Spirits look like spirits.** Each spirit's materials are copied and made translucent, tinted and glowing in its role colour (from `defs.toml`), so they read differently from the player.
- **Overlays** are gizmos drawn just above the ground: walking paths, claimed trees, construction sites (yellow, red when blocked) and the tile under the mouse. Names and storage counts are screen-space labels that follow their objects.
- **UI.** Bevy 0.20 removed the `Interaction` component, so buttons are `bevy_ui_widgets::Button` entities with `On<Activate>` observers.

- **Stations are buildings, and recipes are data.** A recipe names its station, so `is_station` is derived from `[[recipes]]`. One `Craft` task serves the player and spirits; the only difference is where the output goes (the player's bag, or the station for collection).
- **Demand runs one step upstream and is bounded.** `production.rs` turns a plank shortfall into a log request for haulers and a capped raise of the gatherers' goal. It doesn't move anything itself. See `docs/V0.4_WORKSHOP.md`.
- **Trips reserve stock.** A `JobPlan` may carry a `reserve`, taken when the job starts, using the same `Storage` reservations as construction. If a trip fails its unused reservation is released. A project's reservation belongs to the project and survives a failed trip.
- **Stock counted toward a goal excludes what's committed.** `projected_stock` leaves out logs at a station or on their way to one, and construction materials, but counts finished planks at the station.

- **A world is its state plus definitions.** Saves hold the state and name a profile; definitions come from the game's data. A save records the order of definitions it refers to, so old saves survive additions but are refused if anything moves.
- **The pace lives in one place.** Progression profiles are blocks in `data/defs.toml`, resolved when definitions load. The simulation only sees the resulting table and costs. `World::from_scenario` stays on development rates, so scenarios and tests don't change.
- **Guidance watches and never gates.** `journey` reads the world and has no effect on it beyond messages, so a player can ignore it, and scenarios simply don't have one.
- **Materials set aside for the player use `Storage` reservations.** A station's reserved inputs belong to the player, and spirits can't touch them. Training support is hauler job rules, so its priority against production is data.

- **Places at a station are derived, not stored.** A spirit or player with a `Craft` task holds a place, so occupancy can't drift from what workers are doing.
- **Planning stays one step deep.** Every order turns its shortfall into input requests and a capped gather goal. Recipes that form a loop are refused when data loads, which is why no recursion is needed.
- **Instructions narrow, never widen.** `SpiritPolicy` filters jobs, trees and stations after role and level have decided what's possible.
- **Saves upgrade on read.** New fields have serde defaults, and `migrate_v1` adds what defaults can't. `tests/fixtures/` holds real older saves, and `SaveStore` keeps the original file.

## Resource and XP conservation

`Ledger` records logs placed by the scenario (`seeded`), logs cut (`harvested`)
and logs used by construction (`consumed`). `World::check_conservation` checks
`seeded + harvested + crafted == storages + inventories + construction sites + consumed + processed`, and recipe batches must match those totals.
The headless demo and a test both check it every tick. Soul Separation moves an
exact amount of XP from the player to the new spirit, and there is no code path
that adds XP to a spirit.

## Known limitations

- **One gathering skill and two tiers.** Higher levels mostly unlock Oak; chop speed improves only slightly per level. Retraining after a sacrifice currently takes under two minutes (see `docs/V0.3_GATHERING.md`).
- **Waiting spirits stand still** where they ran out of work. They don't fall back to lower tiers or help with other jobs.
- **Training stock lives at the first station only.** See the technical debt list in `docs/V0.6_SPECIALIZATION.md`.
- **Hauling and Construction aren't trainable skills.** Those spirits are founding spirits seeded by the scenario, in both the normal game and the demo.
- **Only one autonomous building type (Field Storage) and one resource (Logs).** The definitions support more, but the planner's "detect" step only measures gatherer delivery distance.
- **Simplified movement.** Spirits move tile to tile on a 4-connected grid with BFS pathfinding and pass through each other. The return leg is assumed to equal the delivery leg when measuring travel share.
- **No save or load.** Restarting reproduces a scenario exactly; there are no mid-game saves.
- **Camera and layout.** The camera follows the player or shows a fixed overview; it can zoom but not rotate or pan freely. The UI panels are tuned for the default 1600x920 window (press `H` to hide them).
- **3D presentation limits.** Characters walk through each other and through the bridge rails, and labels can overlap when characters stand together. The ground is one model per tile (about 1,350 tiles), which is fine at this map size but would need batching for much larger maps.
- **Input was playtested by hand in the 3D version.** WASD, `E`, clicking, zoom and the camera distance felt right. Spam-clicking used to snap the player back to the start of its current step; clicks now finish the step under way and ignore a repeat of the current order.
- **UI input is tested with scripted, not physical, input.** `--script` sends the same input messages the window backend sends, so picking, buttons, the wheel and keys run through the real code paths. It skips the OS and winit layer. `press`, `tile` and `expect` don't depend on the layout; raw `move` and `click` coordinates assume the default 1600x920 window.
- **Field Storage capacity.** If the hauler falls behind and the Field Storage fills up (40 logs), gatherers fall back to Main Storage. This isn't tuned.
- **Planner thresholds are tuned for this map.** They are configurable in `[planner]`.

## Questions for playtesting

- Is the moment the planner decides to build *legible*? Do players understand why it happened from the event log and the planner panel alone?
- Does Soul Separation feel like a meaningful trade-off? Losing levels to gain a permanent worker: is the 100 XP minimum right, and does a spirit's fixed level feel consequential (chopping speed)?
- Pacing: grove regrowth (4 min), objective defaults and the speeds needed before the planner acts in the normal game.
- Does the player want to direct the planner (approve or veto sites, choose placement) or just watch it?
- Is the gather → deliver → idle rhythm readable from the map (paths, claimed-tree rings) without reading the panel?
- What's the next problem to solve autonomously after field storage (for example roads, a second gatherer type, or storage full)?
