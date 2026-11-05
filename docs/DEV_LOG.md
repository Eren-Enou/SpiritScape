# Development log

The handoff between development runs. Newest first.

## v0.10 Plank Roads

**Status:** on branch `v0.10-plank-roads`, with its PR targeting `v0.9-purposeful-expansion`. Not merged.

**Shipped:** four plank roads, quickest-path walking, storage advice that names a road, the `plank_surplus` scenario, the `road_report` example and save format 6.

**Measured:** the reference game spends 1,315 planks (was 565) and ends with 378 of 750 used (was full). Steady beams don't change. A central hauler walks 54% of the time instead of 71%.

**Decided:** fixed routes, not player-drawn, and no road wear. See `docs/V0.10_PLANK_ROADS.md`.

**Next:** training past Woodworking 10 still clogs the store, because roads are a one-off 760 planks. Add a recipe that uses planks as its input. Then a reference player that follows only the UI, then rescuing a walled-in builder, then what remote production is for.

## Questions for the user

- Should the player choose a road's route on the map? Fixed routes were the reversible choice.

## Standing notes

- Nobody has played v0.7, v0.8, v0.9 or v0.10.
- To run the UI scripts, use the command in each script's first lines against the release build. Each needs a fresh save directory, except `ui_continue`, which follows `ui_journey` in the same one. `ui_damaged_save` needs a `world.save` containing garbage.
