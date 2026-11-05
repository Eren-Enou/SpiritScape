# SpiritScape: Spirit Automation RPG (proof of concept)

A single-player 2.5D automation RPG prototype in Rust and Bevy 0.20.1. The game
rules run on a 2D tile grid; the world is drawn in 3D with free CC0 models from
Kenney (Nature Kit, Fantasy Town Kit) and KayKit (Adventurers).

You train Woodcutting yourself, then use **Soul Separation** to sacrifice XP and
create a permanent Woodcutting Spirit with that fixed competency. You give the
settlement a goal. As the spirit works, its delivery routes get long. A
settlement planner notices this and decides to build a **Field Storage** near
the forest. A Construction Spirit builds it from real logs, and the workers
change their routes to use it.

You can also train **Woodworking**: saw logs into planks at the Workshop, then
sacrifice that skill into a Woodworking Spirit. Give the settlement a plank
target and the whole chain runs itself, from gatherers through haulers to the
bench, and planks pay for a Storehouse that expands Main Storage. See
[`docs/V0.4_WORKSHOP.md`](docs/V0.4_WORKSHOP.md).

**v0.5** makes it a game you can play and return to: a title menu, saving and
loading, a normal pace of progression, light guidance through the first cycle,
and spirits that support your retraining by carrying your materials. See
[`docs/V0.5_PROGRESSION.md`](docs/V0.5_PROGRESSION.md), which also has a
checklist for playtesting.

**v0.6** adds a top tier to both skills (Ironwood and Structural Beams at level
10), limited places at each Workshop, further workshops you place yourself,
several production orders at once, simple instructions for each spirit, and
infrastructure that speeds up retraining. See
[`docs/V0.6_SPECIALIZATION.md`](docs/V0.6_SPECIALIZATION.md).

The question this prototype answers: *can a player create a spirit, give it a
goal, and watch the world physically change because the spirit's work causes a
problem that autonomous planning can solve?* See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)
for design decisions and known limitations.

**v0.3** adds Woodcutting tiers (Tree at level 1, Oak at level 5) and a Soul
Separation preview that shows exactly what you give up. It also adds two
gathering sites with limited, renewable output; measurements that name the
current bottleneck; and scenarios for sustainable and oversubscribed
gathering and for retraining after a sacrifice. See
[docs/V0.3_GATHERING.md](docs/V0.3_GATHERING.md) for the design, observed
production rates and open questions.

## Running

Requires a stable Rust toolchain (tested with 1.99) and Windows (other desktop
platforms should work but are untested).

```bash
cargo run --release                                     # normal game
cargo run --release -- --demo                           # field-storage demonstration
cargo run --release -- --scenario regrind               # train, sacrifice, retrain
cargo run --release -- --scenario grove_sustainable     # one spirit, renewable grove
cargo run --release -- --scenario grove_oversubscribed  # four spirits, shortages
```

The scenario buttons in the panel switch between these too.

Options:
- `--speed N` sets the starting simulation speed.
- `--gathering` opens the Gathering tab at start; `--production` opens the Production tab.
- `--scenario` also takes `workshop_manual`, `workshop_automated` and `workshop_construction` (the **Workshop A / B / C** buttons in game).
- `--separate XP` performs a Soul Separation at start.
- `--capture DIR --at 20,160` saves screenshots at those simulation seconds, then exits.
- `--script FILE` plays scripted input (timed in real seconds): mouse moves, clicks, wheel scrolls, key presses, button presses by label and tile clicks, plus `expect` checks on on-screen text. It saves screenshots into the `--capture` folder, doesn't move your real mouse, and exits with an error if a check fails. `crates/spirit_game/src/script.rs` explains the format. `scripts/ui_smoke.txt` covers hover, tabs, zoom, the log filter and Soul Separation; `scripts/ui_workshop.txt` covers training Woodworking through to ordering a Storehouse:

  ```bash
  cargo run -p spirit_game -- --scenario regrind --script scripts/ui_smoke.txt --capture target/ui_smoke
  ```

The last three are development aids.

### Controls

| Input | Action |
|---|---|
| `W` `A` `S` `D` / arrow keys | Walk one tile at a time (hold to keep walking) |
| `E` | Use what you're facing or standing next to: chop a tree, or deposit at a storage |
| Left click on a tree | Walk to it and chop until your inventory is full or the tree is depleted |
| Left click on a storage | Walk to it and deposit your logs |
| Left click on grass | Walk there |
| Mouse wheel / `C` | Zoom / toggle the whole-map overview (the demo starts in overview) |
| `H` | Hide or show the UI panels |
| `G` | Switch between the Settlement and Gathering tabs |
| `P` | Switch to and from the Production tab |
| `J` / `T` / `O` | Switch to and from the Progress / Training / Spirits tab |
| `F5` / `F9` | Save / go back to the last manual save |
| Click Main Storage with empty hands | Take logs |
| Click the Workshop | Saw planks until you run out of logs |
| Mouse wheel over the panel | Scroll the panel |
| Hover a tree or building | Describe it: tier, level needed, logs left, regrowth time |
| `Space` / `1`-`4` | Pause / speed |
| Panel buttons | Pause, speed, restart, load scenario, Soul Separation, objective target, autonomous building, and (Gathering tab) whether spirits may gather at each site |

## Playing the normal game

Run `cargo run --release` and choose **New Game**. The **What's next** card at the top of the panel always says what would make sense to do. Your world autosaves, and **Continue** brings it back.

1. Click the slender **Trees** in the Starting Grove (next to Main Storage) to chop. Each log gives 15 Woodcutting XP. The round **Oaks** need level 5 and give 30 XP per log.
2. Click Main Storage to deposit your logs.
3. Under **Soul Separation**, compare the options and read the preview: your level before and after, the spirit's permanent level, and anything you would lose. Press **Separate Soul**, then confirm. The XP leaves you, and a Woodcutting Spirit appears with that XP as its fixed level. A spirit needs level 5 to cut Oak.
4. The spirit works toward the settlement objective (default: maintain 50 logs in Main Storage). Raise the target with **Target +10** to create more demand.
5. Press `G` for the Gathering tab. It shows how much each site produces, how spirits spend their time and what limits output. Clicking **Reserve for me** under Starting Grove reserves it for your own training, which sends Oak-capable spirits across the bridge to the Distant Forest. Long delivery trips from there are what make the planner build a field storage.

The settlement starts with a Hauling Spirit and a Construction Spirit. Hauling
and Construction aren't trainable skills yet, so these two stand in for them.

## Demonstration scenario (`--demo`)

The demo starts with a level 10 Woodcutting Spirit, the two founding spirits,
20 logs in storage, a 150-log objective and a player with 1500 Woodcutting XP.
The Starting Grove is reserved for the player, so the spirit works the
Distant Forest. It runs at 4x speed. The same events happen every time, at
these simulation times:

| Time | What happens |
|---|---|
| 0:00 | Oakroot crosses the bridge to the forest (about 47 tiles each way). The Gathering tab shows "Travel distance" as the bottleneck, at about 12 logs/min |
| 2:35 | **Planner:** "Inefficiency detected" (walking is about 57% of trip time), then "Decision: build a Field Storage at (34, 11)". 15 logs are reserved and Ashwhisper is assigned |
| 2:35-3:17 | Ashwhisper carries the reserved logs from Main Storage to the site in two trips |
| 3:30 | Field Storage completes and its 15 logs are consumed |
| ~4:00 | Oakroot now delivers to the Field Storage (4 tiles away). Mossbell hauls its stock back to Main Storage. Output roughly doubles to about 23 logs/min |
| ~8:00 | Objective met. Oakroot goes idle and the planner builds nothing more |

To print the same run as text without a window (any scenario key works, and
a gathering report is printed every minute):

```bash
cargo run -p spirit_sim --example headless_demo -- 600 demo
cargo run -p spirit_sim --example regrind_playtest -- 400
```

## Tests

```bash
cargo test -p spirit_sim
```

162 tests:
- 22 end-to-end simulation tests in `crates/spirit_sim/tests/simulation.rs`;
- 13 gathering and progression tests in `crates/spirit_sim/tests/gathering.rs`;
- 31 workshop and production tests in `crates/spirit_sim/tests/workshop.rs`;
- 18 save and load tests in `crates/spirit_sim/tests/persistence.rs`;
- 30 progression, training support and full-cycle tests in `crates/spirit_sim/tests/progression.rs`;
- 43 specialization, workstation, order and save-upgrade tests in `crates/spirit_sim/tests/specialization.rs`;
- 5 unit tests for pathfinding and storage.

They cover:

- **XP and Soul Separation:** the XP curve, XP gained from chopping, exact XP conservation and level recalculation on separation, rejection of invalid requests, and that spirits never gain XP.
- **Resources:** a conservation ledger (seeded + harvested + crafted = present + consumed + processed) checked every tick across a full build cycle, inventory capacity, and deposits.
- **Job transitions:** gather to deliver to idle when the objective is met, and back to gathering when the target rises; blocked when no trees are grown.
- **Construction requirements:** no builder, no hauler, permission off, materials short (with a readable reason), savings too small (rejected once, then a cooldown).
- **Project lifecycle:** detect, decide, reserve, supply, build, consume, redirect workers, haul; no duplicate projects over 20,000+ ticks.
- **Absence of a problem:** short routes or no woodcutter means nothing is built.
- **Determinism:** two runs with the same commands give identical state hashes and event logs.
- **Normal-game loop:** the player trains, separates a spirit and sets a goal, and the settlement builds on its own.
- **Tiers and retraining:**
  - the player and spirits obey level requirements;
  - the Soul Separation preview;
  - losing access, even mid-chop, while the spirit keeps it permanently;
  - regaining access by retraining, with lifetime history kept.
- **Gathering sites:**
  - a site recovers fully after exhaustion;
  - one spirit gathers sustainably, while four cause measurable waiting;
  - waiting spirits resume;
  - spirits relocate without oscillating;
  - reserving a site never creates resources;
  - every scenario conserves resources and replays exactly.
- **Keyboard and mouse movement:** a held direction walks tile by tile without stalling; walking into a tree is refused; spam-clicking a destination never restarts the step in progress.

## Assets

The game loads models from `assets/models/`: a curated copy (about 3.5 MB) of
the pieces it uses from the three packs, with each pack's CC0 license next to
them. The raw downloaded packs in `assets/` are ignored by git. Which model
represents which game object is set in `data/visuals.toml`, so models can be
swapped without touching code.

## Layout

```
data/                      game definitions, visuals mapping (TOML) and the ASCII map
assets/models/             curated CC0 models used by the game
crates/spirit_sim/         engine-free deterministic simulation and tests
crates/spirit_game/        Bevy front end: 3D view, characters, camera/input, UI, pacing
docs/ARCHITECTURE.md       design, decisions, limitations, playtest focus
```
