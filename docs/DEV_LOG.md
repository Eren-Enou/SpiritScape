# Development log

The handoff between development runs. Newest first.

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

## Standing notes

- Nobody has played v0.7 through v0.11.
- Run tests with `--no-fail-fast`. Without it, a failure in one test file hides the rest.
- To run the UI scripts, use the command in each script's first lines against the release build. Each needs a fresh save directory, except `ui_continue`, which follows `ui_journey` in the same one. `ui_damaged_save` needs a `world.save` containing garbage.
