# Development log

The handoff between development runs. Newest first.

## v0.39 The frozen river

**Status:** on branch `v0.39-ice`, with its PR targeting `v0.38-moments`. Not merged.

**Shipped:** the south river freezes in winter. Only the player can walk on it, and the thaw takes your load if it catches you.

**Next:** making offers matter, then models for the Kiln and the Still Boat.

## v0.38 Small moments

**Status:** on branch `v0.38-moments`, with its PR targeting `v0.37-players`. Not merged.

**Shipped:** 26 small moments that spirits have for no reason, and naming now affects them.

**Measured:** 71 moments in the 17-hour reference game, which is otherwise unchanged.

**Next:** a river that freezes in winter, then making offers matter.

## v0.37 Playing styles

**Status:** on branch `v0.37-players`, with its PR targeting `v0.36-second-ending`. Not merged.

**Shipped:** six ways to play the reference game, and a fix for a deadlock where planks set aside for training counted as stock.

**Measured:** winter costs about 3½ hours of 17. Holding more logs is what helps, not planning around winter. Offers save about 2 minutes.

**Next, now the first backlog is done:**

1. Small moments for spirits, so there's more to care about.
2. A river that freezes in winter, so something new happens in the first winter.
3. Making offers matter more, because v0.37 showed they barely register.
4. Models for the Kiln and the Still Boat, which are stand-ins.
5. Warm Walls come very late for what winter costs. Look at opening them sooner.

## v0.36 The Still Boat

**Status:** on branch `v0.36-second-ending`, with its PR targeting `v0.35-the-kiln`. Not merged.

**Shipped:** a second ending. Build a Still Boat, call the spirits home, and take back all you gave them. The closing story now reads four ways.

**Measured:** 9 spirits home in 22 seconds at the end of the reference game, with 47,700 XP returned.

**Next:** better reference players, then more depth in whatever a playtest shows is thin.

## v0.35 The Kiln

**Status:** on branch `v0.35-the-kiln`, with its PR targeting `v0.34-coming-back`. Not merged.

**Shipped:** Kilnwork (clay, charcoal, Bricks), Warm Walls, a Hearth Chimney, Keep in Stock for Stone and Clay, a 19th goal, and `ending_line` for buildings.

**Measured:** all 19 goals by 17:00:32.

**Next:** a second ending, then better reference players.

## v0.34 Coming back

**Status:** on branch `v0.34-coming-back`, with its PR targeting `v0.33-naming`. Not merged.

**Shipped:** seven more visitors, three who only come because of an earlier answer and two who've heard how it went; three more caravan finds; and visitors no longer queue behind someone waiting for a building.

**Next:** a fourth trade and a standing Stone target, a second ending, better reference players.

## v0.33 Names

**Status:** on branch `v0.33-naming`, with its PR targeting `v0.32-the-far-shore`. Not merged.

**Shipped:** naming spirits, and traits that come round in order with the next one shown.

**Next:** more caravan finds and visitors, a fourth trade, a second ending.

## v0.32 The Far Shore

**Status:** on branch `v0.32-the-far-shore`, with its PR targeting `v0.31-the-moment`. Not merged.

**Shipped:** the South Crossing, the Far Shore, Stilltrees that only the player can cut, a Still Lantern, a seventh stone, an 18th goal, and save format 9.

**Measured:** all 18 goals by 16:49.

**Next:** naming spirits, more finds and visitors, a fourth trade, a second ending.

## v0.31 The Moment

**Status:** on branch `v0.31-the-moment`, with its PR targeting `v0.30-feedback`. Not merged.

**Shipped:** spirits gather at the lit Hearth, a closing screen, season colours and snow, and a fix for every spirit being spawned twice.

**Next:** a new region, naming spirits, more finds and visitors.

## v0.30 Feedback

**Status:** on branch `v0.30-feedback`, with its PR targeting `v0.29-onboarding`. Not merged.

**Shipped:** eight generated sounds, sparks, and `M` for sound.

**Next:** a real ending moment, and seasons on the map.

## v0.29 Onboarding

**Status:** on branch `v0.29-onboarding`, with its PR targeting `v0.28-visitors`. Not merged.

**Shipped:** twelve tips that open tabs and skills as they first matter.

**Next:** sound and visual feedback for levels, unlocks, seasons and the ending.

## v0.28 Visitors

**Status:** on branch `v0.28-visitors`, with its PR targeting `v0.27-seasons`. Not merged.

**Shipped:** five visitors with offers, and an On the Road card.

**Next:** onboarding, then sound and visual feedback: seasons on the map, a better ending moment, level-ups.

## v0.27 Seasons

**Status:** on branch `v0.27-seasons`, with its PR targeting `v0.26-old-markers`. Not merged.

**Shipped:** a four-season year in real games, and fixes for a bench lock-up and an oversized-load wait.

**Measured:** every goal takes 16:20, up from 13:12.

**Next:** events and visitors, onboarding, then sound and visual feedback, including seasons on the map and a better ending moment.

## v0.26 The Old Stones

**Status:** on branch `v0.26-old-markers`, with its PR targeting `v0.25-the-long-work`. Not merged.

**Shipped:** six stones with a story, a card to keep them, a changed ending, and a 17th goal.

**Next:** seasons or events, onboarding, then sound and visual feedback. Check that stones stand out on screen.

## v0.25 The Hearth

**Status:** on branch `v0.25-the-long-work`, with its PR targeting `v0.24-masonry`. Not merged.

**Shipped:** the three-stage Hearth of Spirits, a Your Story card, and a 16th goal.

**Measured:** the reference player lights it at 11:03.

**Next:** a new region to discover, seasons, events and visitors, onboarding, then sound and visual feedback for big moments, including the ending.

## v0.24 Masonry

**Status:** on branch `v0.24-masonry`, with its PR targeting `v0.23-spirit-stories`. Not merged.

**Shipped:** Masonry, the Mason's Yard, Dressed Stone, Keystones, a Masonry Spirit and a 15th goal.

**Measured:** all 15 goals are met by 13:04 in one game.

**Next:** the ending. Keystones exist for a final project. Also consider a standing Stone target.

## v0.23 Spirit Stories

**Status:** on branch `v0.23-spirit-stories`, with its PR targeting `v0.22-caravan-finds`. Not merged.

**Shipped:** traits, deeds, titles, a Their Story card, and 24 spirit names.

**Decided:** traits can't be chosen, and only made spirits have them.

**Next:** a third skill and a longer chain joining wood and stone. Then a new region, seasons, events and an ending.

## v0.22 Caravan Finds

**Status:** on branch `v0.22-caravan-finds`, with its PR targeting `v0.21-trade-caravans`. Not merged.

**Shipped:** caravans that open buildings, `plants` for adding trees to the map, the Ironwood Nursery, the Quarry Extension and a 14th goal.

**Measured:** Hardwood 15.0 to 18.1 a minute for gatherers free to roam. A long game now leaves behind 476 units.

**Next:** spirits as characters.

## v0.21 Trade Caravans

**Status:** on branch `v0.21-trade-caravans`, with its PR targeting `v0.20-playtest-ready`. Not merged.

**Shipped:** repeatable Trade Caravans, a 13th goal, and the `caravan_yard` scenario.

**Measured:** a long game throws away 2,240 units instead of 6,160.

**Next:** what caravans bring back, then spirits as characters. The user set a new goal: a game with a whole shape, from beginning to ending.

## v0.20 Ready to Play

**Status:** on branch `v0.20-playtest-ready`, with its PR targeting `v0.19-every-goal`. Not merged.

**Shipped:** `docs/PLAYTEST.md`, the `full_map` scenario and a `--fps` flag.

**Measured:** 60 frames a second, capped by vertical sync, on the busiest map on this machine.

**Next:** the backlog is finished. Everything left needs the user: the playtest, physical input, and the four decisions listed in `docs/PLAYTEST.md`. Don't add systems before then.

## v0.19 Every Goal

**Status:** on branch `v0.19-every-goal`, with its PR targeting `v0.18-demand-and-goals`. Not merged.

**Shipped:** a run that chases all twelve goals, six Warehouses instead of two, a "Leave your load" last resort, and the `no_room_left` scenario.

**Measured:** all twelve goals are met by 12:55:39 by a player who never hesitates.

**Decided:** leaving a load behind is reversible and only offered when nowhere will take it. See the question below.

**Next:** the known gaps are closed. The remaining items need a person: playtests, physical input, frame rate, and art and audio judgement.

## v0.18 Demand and Goals

**Status:** on branch `v0.18-demand-and-goals`, with its PR targeting `v0.17-long-runs`. Not merged.

**Shipped:** orders now ask for what other orders need of them, capped, and playthroughs announce goals once.

**Measured:** earlier reference results match exactly. The long-run speed reading fell from about 25,000 to 19,000 ticks a second, which may be noise.

**Next:** the known gaps are closed. The remaining items need a person: playtests, physical input, frame rate, and art and audio judgement.

## v0.17 Long Runs

**Status:** on branch `v0.17-long-runs`, with its PR targeting `v0.16-long-goals`. Not merged.

**Shipped:** the `long_run_report` example and a wording cleanup for gathering.

**Measured:** 24 more game hours left speed flat at about 25,000 ticks a second and the save flat at 90 KB. Frame rate isn't measured.

**Next:** the backlog is nearly done, and what's left needs a person: playtests, physical input and art judgement. Stop adding systems until there's playtest feedback.

## v0.16 Ambitions

**Status:** on branch `v0.16-long-goals`, with its PR targeting `v0.15-quarrying`. Not merged.

**Shipped:** twelve long-term goals in data, an Ambitions card in the Progress tab, and a settled game pointing at the next goal.

**Measured:** the guided route meets 7 of 12 by about 2:30. No test reaches the level 20 goals.

**Decided:** no rewards, no saved state.

**Next:** the remaining backlog is polish and performance on long saves. All balance so far comes from scripts, so more systems add risk without a playtest. Prefer small fixes: wording that says "trees" for all gathering, and a long-save performance check.

## v0.15 Quarrying

**Status:** on branch `v0.15-quarrying`, with its PR targeting `v0.14-full-loads`. Not merged.

**Shipped:** the Quarrying skill, Stone, the South Quarry, the Quarrying Spirit, the Stone Causeway, the `quarry_start` scenario, the `quarry_report` example and save format 8.

**Measured:** the walk to the Distant Forest drops from 16.2 to 9.0 seconds. The causeway takes about 6.5 minutes at development rates, and helping saves only 11 seconds.

**Physical input:** I asked for computer control and it was declined or timed out. It stays unverified. Don't retry unless the user offers.

**Next:** long-term goals and onboarding. The reference route doesn't use Quarrying yet, and the guidance doesn't mention it. Wording that says "trees" for all gathering needs a pass.

## v0.14 Full Loads

**Status:** on branch `v0.14-full-loads`, with its PR targeting `v0.13-stranded-rescue`. Not merged.

**Shipped:** one data change. Long trips now wait for a full load, not half.

**Measured:** remote walks about half what central does for the same beams (454 tiles against 902), and now wins with a lone shared hauler too. First beams arrive about 50 seconds later.

**Decided:** remote's purpose is saving hauler time, and that's now large enough to matter. I built and removed a rule letting bench workers fetch their own materials, because it did worse. See `docs/V0.14_FULL_LOADS.md`.

**Next:** physical input, then the first new skill or resource chain.

## v0.13 Stranded Rescue

**Status:** on branch `v0.13-stranded-rescue`, with its PR targeting `v0.12-guided-player`. Not merged.

**Shipped:** a check every 10 seconds that moves anyone cut off from the main open ground to its nearest edge, and the `walled_in` scenario.

**Decided:** a short jump, not removing buildings, and it runs all the time, not only on load.

**Next:** what remote production is for, then physical input.

## v0.12 Guided Player

**Status:** on branch `v0.12-guided-player`, with its PR targeting `v0.11-fitted-panels`. Not merged.

**Shipped:** `storage_fixes()` as the one source for the storage message, a guided reference player that knows only that list, the `guided_report` example, and a fix for the message staying silent while Main Storage refused planks.

**Measured:** the guided player reaches the Forest Crossing at 1:46:54, against 1:57:22 for hand-written habits. It took 14 actions from the message.

**Decided:** the old route stays as it is, so earlier numbers still compare.

**Next:** rescue a walled-in builder from old saves, then what remote production is for, then physical input.

## v0.11 Fitted Panels

**Status:** on branch `v0.11-fitted-panels`, with its PR targeting `v0.10-plank-roads`. Not merged.

**Shipped:** the Fitted Panels recipe (chosen only, level 7), a panel order, Panelled Warehouse, Carrying Frames, the `panel_workshop` scenario, the `panel_report` example and save format 7.

**Measured:** two more hours of hand training after the route gives 53,725 XP with Main Storage at 722 of 1,350, against 10,350 XP and a full store on planks alone.

**Decided:** panels never get made automatically by the player. Beams stay the top XP recipe.

**Next:** a reference player that follows only what the UI tells it, because the storage advice now has a lot of options. Then rescuing a walled-in builder, then what remote production is for. Panels will eventually pile up, and plank demand doesn't chain from a panel plan.

## v0.10 Plank Roads

**Status:** on branch `v0.10-plank-roads`, with its PR targeting `v0.9-purposeful-expansion`. Not merged.

**Shipped:** four plank roads, quickest-path walking, storage advice that names a road, the `plank_surplus` scenario, the `road_report` example and save format 6.

**Measured:** the reference game spends 1,315 planks (was 565) and ends with 378 of 750 used (was full). Steady beams don't change. A central hauler walks 54% of the time instead of 71%.

**Decided:** fixed routes, not player-drawn, and no road wear. See `docs/V0.10_PLANK_ROADS.md`.

**Next:** training past Woodworking 10 still clogs the store, because roads are a one-off 760 planks. Add a recipe that uses planks as its input. Then a reference player that follows only the UI, then rescuing a walled-in builder, then what remote production is for.

## Questions for the user

- Should the player choose a road's route on the map? Fixed routes were the reversible choice.
- Is 447 XP a minute from panels too fast? Only a playtest can say.
- Should the thaw take your whole load, or is that too harsh?
- Winter costs the reference game about a fifth of its time. Is that the right size?
- Offers save about 2 minutes in 17 hours. Should they matter more, or is being remembered reward enough?
- Is one tip enough of a hint that the Still Boat exists? It's deliberately not a goal.
- Should letting a visitor leave count as turning them down? For now it counts as neither.
- Should a full store really let you throw goods away? It's the only way Woodcutting 20 stays reachable, because nothing uses logs and panels without limit.

## Standing notes

- Nobody has played v0.7 through v0.11.
- Run tests with `--no-fail-fast`. Without it, a failure in one test file hides the rest.
- To run the UI scripts, use the command in each script's first lines against the release build. Each needs a fresh save directory, except `ui_continue`, which follows `ui_journey` in the same one. `ui_damaged_save` needs a `world.save` containing garbage.
