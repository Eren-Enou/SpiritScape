# Playtest guide

Nobody has played anything since v0.6, and every balance choice since then comes from scripts. This guide covers it all in one session of about an hour. You don't need the per-version checklists.

## Getting the game

Every version is included on the newest branch, so one checkout is enough:

```bash
git fetch origin
```

```bash
git checkout v0.24-masonry
```

Press `F6` at any time to write a report of your session. Note the time whenever something surprises you.

## 1. A normal game (20 minutes)

The most important one. Play as if you'd just found the game.

```bash
cargo run --release -- --new-game --save-dir target/playtest
```

- [ ] Did you know what to do in the first five minutes?
- [ ] When did you first want to stop?
- [ ] Did making a spirit feel worth the XP it cost?
- [ ] Do mouse and keyboard work as you expect? This has never been checked with real input since v0.6.

## 2. Storage and planks (10 minutes)

```bash
cargo run --release -- --scenario plank_surplus
```

Read the message, then do what you'd naturally do.

- [ ] Did you read the whole message, or stop after the first choice?
- [ ] Would buttons be better than a paragraph?
- [ ] Did a road feel worth buying? Can you feel the speed on foot?

## 3. Fitted Panels (10 minutes)

```bash
cargo run --release -- --scenario panel_workshop
```

Walk to the Workshop, press `T` and choose Fitted Panels.

- [ ] Is it clear why you'd choose panels?
- [ ] Is 90 XP a batch too much?
- [ ] Is waiting at the bench for planks acceptable?

## 4. The crossing and the plan (10 minutes)

```bash
cargo run --release -- --scenario forest_access
```

Press `B`, plan the Forest Crossing, then go and chop.

- [ ] Could you forget about the plan and trust it?
- [ ] Did it feel like an accomplishment when it opened?

## 5. The quarry (5 minutes)

```bash
cargo run --release -- --scenario quarry_start
```

- [ ] Did you find the quarry?
- [ ] Does cutting stone feel different enough from cutting trees?
- [ ] Clicking an outcrop has never been tested in the window. Does it work?

## 6. The whole map (5 minutes)

```bash
cargo run --release -- --scenario full_map --fps
```

- [ ] Does it look like one settlement?
- [ ] Does a hauler waiting for a full load look sensible, or stuck?
- [ ] Is it smooth on your machine?

## 7. Trade Caravans (5 minutes)

```bash
cargo run --release -- --scenario caravan_yard
```

- [ ] Did a caravan feel like a choice, or a chore?
- [ ] Did what it brought back feel like a reward?

## 8. A spirit of your own (5 minutes)

```bash
cargo run --release -- --scenario quarry_start
```

Make a spirit from the Soul tab (`R`), then look at it in the Spirits tab (`O`).

- [ ] Did it feel like someone, or like a stat line?

## 9. The Mason's Yard (5 minutes)

```bash
cargo run --release -- --scenario mason_yard
```

- [ ] Could you work out what the yard is for?

## Four decisions only you can make

1. **Is the pace right?** The reference player reaches the Forest Crossing in under two hours, and panels give about 450 XP a minute.
2. **Should a full store let you throw goods away?** "Leave your load" keeps Woodcutting 20 reachable, but it's a patch. The alternative is something that uses goods repeatedly, such as road upkeep.
3. **Should you choose road routes yourself?** They're fixed for now.
4. **What should come next?** A new skill, more depth in these, or something else entirely.

## The pull requests

They're stacked, each targeting the one before, so they must be merged in this order, or reviewed together from the newest branch.

| PR | Version | What it adds |
|---|---|---|
| [#7](https://github.com/Eren-Enou/SpiritScape/pull/7) | v0.9 | Forest Crossing, construction plan, hauler scoring |
| [#8](https://github.com/Eren-Enou/SpiritScape/pull/8) | v0.10 | Plank roads |
| [#9](https://github.com/Eren-Enou/SpiritScape/pull/9) | v0.11 | Fitted Panels, Warehouse, Carrying Frames |
| [#10](https://github.com/Eren-Enou/SpiritScape/pull/10) | v0.12 | Guided player, storage message fix |
| [#11](https://github.com/Eren-Enou/SpiritScape/pull/11) | v0.13 | Rescue for anyone walled in |
| [#12](https://github.com/Eren-Enou/SpiritScape/pull/12) | v0.14 | Full loads on long trips |
| [#13](https://github.com/Eren-Enou/SpiritScape/pull/13) | v0.15 | Quarrying and the Stone Causeway |
| [#14](https://github.com/Eren-Enou/SpiritScape/pull/14) | v0.16 | Ambitions |
| [#15](https://github.com/Eren-Enou/SpiritScape/pull/15) | v0.17 | Long-run check, wording |
| [#16](https://github.com/Eren-Enou/SpiritScape/pull/16) | v0.18 | Demand chains, goal messages |
| [#17](https://github.com/Eren-Enou/SpiritScape/pull/17) | v0.19 | Every goal reachable, Leave your load |
| [#18](https://github.com/Eren-Enou/SpiritScape/pull/18) | v0.20 | Frame rate check and this guide |
