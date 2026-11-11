# Reviewing the stack

There are 21 open pull requests here, #38 to #58, each built on the one before. Nothing in them is merged, and only you can merge them. This page is meant to make that easier.

## The order

Each pull request targets the one before it, so its diff shows only that milestone.

| PR | Milestone | What it is | Changes rules? |
|---|---|---|---|
| #38 | v0.40 Release candidate | v0.9 to v0.39 in one place, with CI, a launcher and a playtest guide. Targets `main`. | yes (see its own doc) |
| #39 | v0.41 What they leave | Visitors teach a lesson and are remembered in the closing story, ported | small |
| #40 | v0.42 Our own models | Generated models for the Kiln, Still Boat and more | no |
| #41 | v0.43 Notes | A notes tab (`L`), an autumn tip, moments for older spirits | no |
| #42 | v0.44 Left behind | Four relics on the Far Shore, a reference style that sends caravans early | small |
| #43 | v0.45 Settings | Sound volume and text size | no |
| #44 | v0.46 Together | Spirits who notice each other, midwinter at the Hearth, the years | small |
| #45 | v0.47 Kept company | Spirits who share moments keep each other company | ending only |
| #46 | v0.48 Your stone | Carve a stone of your own after the ending | no |
| #47 | v0.49 Last time | Your stone stands in your next game | no |
| #48 | v0.50 One thing at a time | Panels only name what you've met | no |
| #49 | v0.51 What it's for | Each building says what it's for where you plan it | no |
| #50 | v0.52 What's stuck | The Production tab names the order that's actually stuck | no |
| #51 | v0.53 Room to read | A smaller You card, so tabs start higher | no |
| #52 | v0.54 One plank | "1 Plank", not "1 Planks" | no |
| #53 | v0.55 Tidy lines | Training tab lines that don't break mid-word | no |
| #54 | v0.56 The last helper | One of the old carver's helpers, by the Last Stone, and your choice | no |
| #55 | v0.57 The walk home | It walks home if asked | no |
| #56 | v0.58 Two games in a row | A long test of the stone across two games, and the helper made hard to miss | no |
| #57 | v0.59 What happened | The F6 play report says what happened | no |
| #58 | v0.60 Old and new | Your spirits notice the old helper by the fire | no |

"Changes rules" means something a player could measure: XP, speed, costs, what spirits do. The rest is words, panels, story and checks. The reference game meets all 19 goals by 16:57:21 from v0.47 on.

## Decisions waiting on you

These are in the pull requests, and in `docs/DEV_LOG.md` under "Questions for the user":

1. **The Still Boat gives sacrificed XP back** (#38). The game began with the rule that a sacrifice is permanent. This ending breaks it on purpose.
2. **A stone carries from one game to the next** (#47). It's the first thing that does, and it's only words.
3. **The last helper does no work** (#54). Asking it home is only about what it means.

## Assertions that changed

Every changed test or script expectation is explained in its own pull request:

- #44: `ui_moments2.txt` expects a different first moment, because new moments joined the pool.
- #49: `ui_journey.txt` expects the Progress tab's new card title. Two v0.50 unit tests were rewritten for the new card.
- #55: the helper starts at its stone and walks, instead of being at your fire straight away.
- #57: the ending now mentions a helper met but never answered.

## How I'd review it

- **To play it:** check out the newest branch and run `play.bat`. `docs/PLAYTEST.md` says what to try. Press `F6` before you stop, and send back `play_report.txt`.
- **To read it:** go through the pull requests in order. Most are small, and each has a doc in `docs/` that says what it's for, what was measured and what isn't verified.
- **To merge it:** merge from the bottom (#38) up. Each one's base becomes the branch it targets, so after merging #38 into `main`, #39 can be retargeted to `main`, and so on. Nothing here has been merged, and I won't.

Every pull request passed, before it was committed: Clippy with warnings as errors, every workspace test, every release-only long run, two identical reference games, and every UI script. CI confirms the same on each, apart from the UI scripts, which need a screen.
