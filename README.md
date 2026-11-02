# SpiritScape: Spirit Automation RPG (proof of concept)

A single-player 2D automation RPG prototype in Rust and Bevy 0.20.1.

You train Woodcutting yourself, then use **Soul Separation** to sacrifice XP and
create a permanent Woodcutting Spirit with that fixed competency. You give the
settlement a goal. As the spirit works, its delivery routes get long. A
settlement planner notices this and decides to build a **Field Storage** near
the forest. A Construction Spirit builds it from real logs, and the workers
change their routes to use it.

The question this prototype answers: *can a player create a spirit, give it a
goal, and watch the world physically change because the spirit's work causes a
problem that autonomous planning can solve?* See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)
for design decisions and known limitations.

## Running

Requires a stable Rust toolchain (tested with 1.99) and Windows (other desktop
platforms should work but are untested).

```bash
cargo run --release              # normal game
cargo run --release -- --demo    # development demonstration scenario
```

Options: `--speed N` sets the starting simulation speed. `--capture DIR --at 20,160`
saves screenshots at those simulation seconds and then exits (a development aid).

### Controls

| Input | Action |
|---|---|
| Left click on a tree | Walk to it and chop until your inventory is full or the tree is depleted |
| Left click on a storage | Walk to it and deposit your logs |
| Left click on grass | Walk there |
| Panel buttons | Pause, speed (1x/2x/4x/8x), restart, load scenario, Soul Separation, objective target, autonomous-building permission |
| `Space` / `1`-`4` / `R` / `N` / `D` | Pause / speed / restart / new game / demo |

## Playing the normal game

1. Click the trees near Main Storage (the brown square on the left) to chop. Each log gives 25 Woodcutting XP.
2. Click Main Storage to deposit your logs.
3. Under **Soul Separation**, choose an amount (at least 100 XP) and press **Separate Soul**. The XP leaves you, and a Woodcutting Spirit appears with that XP as its fixed level.
4. The spirit works toward the settlement objective (default: maintain 50 logs in Main Storage). Raise the target with **Target +10** to create more demand.
5. Once the trees near town are used up (they regrow after 4 minutes), the spirit walks across the bridge to the eastern forest. Long delivery trips are what make the planner act.

The settlement starts with a Hauling Spirit and a Construction Spirit. Hauling
and Construction aren't trainable skills yet, so these two stand in for them.

## Demonstration scenario (`--demo`)

The demo starts with a level 10 Woodcutting Spirit, the two founding spirits,
20 logs in storage, a 150-log objective and a player with 1500 Woodcutting XP.
It runs at 4x speed. The same events happen every time, at these simulation times:

| Time | What happens |
|---|---|
| 0:00-1:30 | Oakroot chops the grove near town (short trips) |
| ~1:30 | The grove is depleted; Oakroot crosses the bridge to the forest (about 46 tiles each way) |
| 2:35 | **Planner:** "Inefficiency detected" (walking is about 49% of trip time), then "Decision: build a Field Storage at (34, 11)". 15 logs are reserved and Ashwhisper is assigned |
| 2:35-3:17 | Ashwhisper carries the reserved logs from Main Storage to the site in two trips |
| 3:30 | Field Storage completes and its 15 logs are consumed |
| ~4:00 | Oakroot now delivers to the Field Storage (5 tiles away). Mossbell hauls its stock back to Main Storage |
| ~8:00 | Objective met. Oakroot goes idle and the planner builds nothing more |

To print the same run as text without a window:

```bash
cargo run -p spirit_sim --example headless_demo -- 600
```

## Tests

```bash
cargo test -p spirit_sim
```

25 tests: 20 end-to-end simulation tests in `crates/spirit_sim/tests/simulation.rs` plus unit tests for pathfinding and storage. They cover:

- **XP and Soul Separation:** the XP curve, XP gained from chopping, exact XP conservation and level recalculation on separation, rejection of invalid requests, and that spirits never gain XP.
- **Resources:** a conservation ledger (seeded + harvested = present + consumed) checked every tick across a full build cycle, inventory capacity, and deposits.
- **Job transitions:** gather to deliver to idle when the objective is met, and back to gathering when the target rises; blocked when no trees are grown.
- **Construction requirements:** no builder, no hauler, permission off, materials short (with a readable reason), savings too small (rejected once, then a cooldown).
- **Project lifecycle:** detect, decide, reserve, supply, build, consume, redirect workers, haul; no duplicate projects over 20,000+ ticks.
- **Absence of a problem:** short routes or no woodcutter means nothing is built.
- **Determinism:** two runs with the same commands give identical state hashes and event logs.
- **Normal-game loop:** the player trains, separates a spirit and sets a goal, and the settlement builds on its own.

## Layout

```
data/                      game definitions (TOML) and the ASCII map
crates/spirit_sim/         engine-free deterministic simulation and tests
crates/spirit_game/        Bevy front end: rendering, UI, input, pacing
docs/ARCHITECTURE.md       design, decisions, limitations, playtest focus
```
