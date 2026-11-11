# Playtest guide

Nobody has played SpiritScape since v0.6. Every balance choice since then comes from automated players, which can show the game works but can't show whether it's any good. This guide is how to find out.

There are two stages. Stage A is the one that matters most.

## Getting the game

```bash
git fetch origin
```

```bash
git checkout v0.56-last-helper
```

Then double-click **`play.bat`**. The first time, it builds the game, which takes several minutes. After that it starts straight away.

- **Your game is saved in `%APPDATA%\SpiritScape`.** It autosaves, and saves when you close the window. Run `play.bat` again to carry on.
- **If something goes wrong,** the window stays open with the error, and your save is untouched.
- **Press `F6` at any time** to write a report of your session. It goes in the save folder as `play_report.txt`.
- **Note the time whenever something surprises you.** The clock is at the top right.

If you'd rather not install Rust, someone with it can run `tools\package_windows.ps1` to make a folder you can copy and play from.

## Stage A: a normal new game (30 to 60 minutes)

Run `play.bat`, choose **New Game**, and play. Don't read anything else first. The game should teach you as you go.

Write down your answers as you play:

**Getting started**

- [ ] How long before you knew what to do? What told you?
- [ ] Were the controls easy to find? Anything you looked for and couldn't find?
- [ ] Did cutting trees feel worth doing?

**Your first spirit**

- [ ] Did you understand what Soul Separation would cost you before you did it?
- [ ] Did giving up your levels feel like a real choice?
- [ ] Did you care about the spirit afterwards? Did you rename it?
- [ ] Did two of your spirits come to keep each other company? Did you notice, and did you care?

**After that**

- [ ] Did the spirits seem to know what they were doing? Did you ever wonder why one was idle?
- [ ] Was it clear what to build next, and why?
- [ ] When you planned a building, did you know what it would do for you?
- [ ] Could you read the panels at a glance, or did you have to study them?
- [ ] Did you ever look something up in your notes (`L`)? Did you find it?
- [ ] Did sounds and sparks help, or get in the way? (`M` turns sound off.)
- [ ] Did you change the sound or text size in the Menu? Did it stay changed next time?
- [ ] When did you first want to stop? When did you first not want to?
- [ ] When a visitor came, did you know what saying yes or no would mean?

## Stage B: one system at a time

Double-click **`scenarios.bat`** and pick a number. Each one starts right where that system matters, so you don't have to play for hours to reach it.

**Scenarios keep their own saves,** in `%APPDATA%\SpiritScape\scenario-tests`. Nothing you do in one can overwrite your Stage A game.

| # | Scenario | What to try | What to look for |
|---|---|---|---|
| 1 | The quarry | Cut stone, then make a Quarrying Spirit | Was it clear stone is a separate skill? |
| 2 | The Mason's Yard | Dress stone and cut Keystones | Did joining stone and timber make sense? |
| 3 | A remote outpost | Set up work far from home | Could you tell what the outpost was doing? |
| 4 | Production chains | Ask for panels and beams in Production (`P`) | Could you see why something wasn't being made? |
| 5 | Trade Caravans | Plan one in Projects (`B`) and send it | Did what came back feel like a reward? |
| 6 | The Kiln | Ask for Bricks | Is a chain this long interesting, or just waiting? |
| 7 | A quiet hour | Watch for a while, then press `O` | Did a spirit's small moments make you feel anything? |
| 8 | Winter | Walk south across the frozen river, and keep going | Did you feel any risk? Did you find anything on the far side without being told where? |
| 9 | The Hearth | Plan the Hearth of Spirits, then press `J` | Did it feel like an ending? |
| 10 | The Still Boat | Press `J`, and decide | Did you hesitate? Did the closing words feel earned? |
| 11 | The whole map | Walk around | Does it look like one place? |
| 12 | Brickyard | Look at the Kiln, Warm Walls, Hearth Chimney and Still Boat | Can you tell what each one is without reading its label? |
| 13 | Midwinter | Watch for a minute as winter starts. Then press `J` and carve a stone of your own | Did the spirits going to the Hearth feel like something? Did choosing your stone's last line feel like yours? |
| 14 | Old friends | Press `O` and read both their stories | Did knowing two of them keep company change how you see them? |
| 15 | Last time | Walk up past the first old stone, top left | Did finding your own words there mean anything? |
| 16 | The Last Stone | Walk up to who is standing there, and answer it | Did the choice feel like it mattered, though it changes nothing you can count? |

## Four decisions only you can make

1. **Is the pace right?** The automated player meets all 19 goals in about 17 hours. A person will be slower.
2. **The Still Boat gives sacrificed XP back.** The game began with the rule that a sacrifice is permanent, and this ending breaks that rule on purpose. Keep it, change it, or cut it? See `docs/V0.40_RELEASE_CANDIDATE.md`.
3. **Winter costs about a fifth of the game's time.** Is that the right size?
4. **Visitors' offers barely change the economy.** Is being remembered enough of a reward?

## What to send back

Your notes from Stage A, the scenarios you tried, and any `play_report.txt`. Screenshots help where something looked wrong.
