//! Functional UI: status panel, Soul Separation, objective and simulation
//! controls, spirit roster, planner/project status and the event log.

use crate::sim_runner::{SPEEDS, Sim};
use bevy::picking::Pickable;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button};
use spirit_sim::defs::SiteId;
use spirit_sim::entities::{Activity, ProjectState};
use spirit_sim::events::EventKind;
use spirit_sim::metrics::TimeUse;
use spirit_sim::{Command, ScenarioKind, World};

const PANEL_BG: Color = Color::srgba(0.06, 0.07, 0.09, 0.96);
const BUTTON_BG: Color = Color::srgb(0.20, 0.24, 0.30);
const BUTTON_ACTIVE: Color = Color::srgb(0.25, 0.45, 0.30);
const HEADING: Color = Color::srgb(0.95, 0.85, 0.55);
const BODY: Color = Color::srgb(0.88, 0.90, 0.92);
const LOG_LINES: usize = 11;

/// Screen space covered by the UI: right-hand panel and bottom event log.
pub const PANEL_WIDTH: f32 = 430.0;
pub const LOG_HEIGHT: f32 = 230.0;

/// Whether the panels are shown (`H` toggles them).
#[derive(Resource)]
pub struct UiVisible(pub bool);

#[derive(Component)]
struct PanelRoot;

#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum UiAction {
    TogglePause,
    Speed(u32),
    Restart,
    Load(ScenarioKind),
    SoulAdjust(i64),
    SoulHalf,
    SoulAll,
    SoulSeparate,
    ObjectiveAdjust(i32),
    ToggleConstruction,
    ShowTab(Tab),
    ToggleSite(usize),
}

/// Which half of the lower right panel is shown.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug)]
enum Tab {
    Settlement,
    Gathering,
}

/// Development flag: open the Gathering tab at start (`--gathering`).
#[derive(Resource)]
pub struct StartOnGatheringTab(pub bool);

/// Root node of a tab's content.
#[derive(Component)]
struct TabPanel(Tab);

/// The large Soul Separation notice.
#[derive(Component)]
struct Banner;

#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Section {
    Header,
    Player,
    Soul,
    Settlement,
    Spirits,
    Planner,
    Gathering,
}

/// Clipping box that holds the event log lines.
#[derive(Component)]
struct LogBox;

/// One event log entry, newest first.
#[derive(Component)]
struct LogLine(usize);

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(UiVisible(true))
            .insert_resource(Tab::Settlement)
            .add_systems(Startup, build_ui)
            .add_systems(
                Update,
                (
                    update_texts,
                    update_log,
                    fit_log,
                    update_buttons,
                    toggle_panels,
                    show_tab,
                    update_banner,
                ),
            );
    }
}

fn heading(p: &mut ChildSpawnerCommands, text: &str) {
    p.spawn((
        Text::new(text),
        TextFont::from_font_size(15.0),
        TextColor(HEADING),
        Node {
            margin: UiRect::top(Val::Px(6.0)),
            ..default()
        },
    ));
}

fn section(p: &mut ChildSpawnerCommands, s: Section) {
    p.spawn((
        s,
        Text::new(""),
        TextFont::from_font_size(13.0),
        TextColor(BODY),
    ));
}

fn button_row(p: &mut ChildSpawnerCommands, buttons: &[(&str, UiAction)]) {
    p.spawn(Node {
        flex_direction: FlexDirection::Row,
        flex_wrap: FlexWrap::Wrap,
        column_gap: Val::Px(4.0),
        row_gap: Val::Px(4.0),
        ..default()
    })
    .with_children(|row| {
        for (text, action) in buttons {
            row.spawn((
                Button,
                *action,
                Node {
                    padding: UiRect::axes(Val::Px(8.0), Val::Px(3.0)),
                    ..default()
                },
                BackgroundColor(BUTTON_BG),
            ))
            .observe(on_button)
            .with_children(|b| {
                b.spawn((
                    *action,
                    Text::new(*text),
                    TextFont::from_font_size(13.0),
                    TextColor(BODY),
                    Pickable::IGNORE,
                ));
            });
        }
    });
}

fn build_ui(
    mut commands: Commands,
    sim: Res<Sim>,
    start: Res<StartOnGatheringTab>,
    mut tab: ResMut<Tab>,
) {
    if start.0 {
        *tab = Tab::Gathering;
    }
    let site_names: Vec<String> = sim
        .world
        .defs
        .sites
        .iter()
        .map(|s| s.name.clone())
        .collect();
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Px(PANEL_WIDTH),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(10.0)),
                row_gap: Val::Px(4.0),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(PANEL_BG),
            PanelRoot,
        ))
        .with_children(|p| {
            section(p, Section::Header);
            button_row(
                p,
                &[
                    ("Pause", UiAction::TogglePause),
                    ("1x", UiAction::Speed(SPEEDS[0])),
                    ("2x", UiAction::Speed(SPEEDS[1])),
                    ("4x", UiAction::Speed(SPEEDS[2])),
                    ("8x", UiAction::Speed(SPEEDS[3])),
                ],
            );
            button_row(
                p,
                &[
                    ("Restart", UiAction::Restart),
                    ("New Game", UiAction::Load(ScenarioKind::NewGame)),
                    ("Demo", UiAction::Load(ScenarioKind::Demo)),
                    ("Regrind", UiAction::Load(ScenarioKind::Regrind)),
                    ("Grove OK", UiAction::Load(ScenarioKind::GroveSustainable)),
                    (
                        "Grove Over",
                        UiAction::Load(ScenarioKind::GroveOversubscribed),
                    ),
                ],
            );

            heading(p, "Player");
            section(p, Section::Player);

            heading(p, "Soul Separation");
            section(p, Section::Soul);
            button_row(
                p,
                &[
                    ("-100", UiAction::SoulAdjust(-100)),
                    ("+100", UiAction::SoulAdjust(100)),
                    ("Half", UiAction::SoulHalf),
                    ("All", UiAction::SoulAll),
                    ("Separate Soul", UiAction::SoulSeparate),
                ],
            );

            button_row(
                p,
                &[
                    ("Settlement", UiAction::ShowTab(Tab::Settlement)),
                    ("Gathering (G)", UiAction::ShowTab(Tab::Gathering)),
                ],
            );

            tab_panel(p, Tab::Settlement, |p| {
                heading(p, "Settlement");
                section(p, Section::Settlement);
                button_row(
                    p,
                    &[
                        ("Target -10", UiAction::ObjectiveAdjust(-10)),
                        ("Target +10", UiAction::ObjectiveAdjust(10)),
                        ("Autonomous building", UiAction::ToggleConstruction),
                    ],
                );

                heading(p, "Settlement Planner");
                section(p, Section::Planner);

                heading(p, "Spirits");
                section(p, Section::Spirits);
            });

            tab_panel(p, Tab::Gathering, |p| {
                heading(p, "Gathering sites");
                let buttons: Vec<(&str, UiAction)> = site_names
                    .iter()
                    .enumerate()
                    .map(|(i, name)| (name.as_str(), UiAction::ToggleSite(i)))
                    .collect();
                button_row(p, &buttons);
                section(p, Section::Gathering);
            });
        });

    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(PANEL_WIDTH),
                top: Val::Px(70.0),
                justify_content: JustifyContent::Center,
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|p| {
            p.spawn((
                Banner,
                Text::new(""),
                TextFont::from_font_size(20.0),
                TextColor(HEADING),
                TextLayout::justify(Justify::Center),
                Node {
                    padding: UiRect::axes(Val::Px(24.0), Val::Px(14.0)),
                    display: Display::None,
                    ..default()
                },
                BackgroundColor(Color::srgba(0.10, 0.06, 0.16, 0.92)),
                Pickable::IGNORE,
            ));
        });

    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(PANEL_WIDTH),
                bottom: Val::Px(0.0),
                height: Val::Px(LOG_HEIGHT),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(8.0)),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(PANEL_BG),
            PanelRoot,
        ))
        .with_children(|p| {
            heading(p, "Event Log - why things happen");
            p.spawn((
                LogBox,
                Node {
                    flex_direction: FlexDirection::Column,
                    flex_grow: 1.0,
                    min_height: Val::Px(0.0),
                    overflow: Overflow::clip(),
                    ..default()
                },
            ))
            .with_children(|log| {
                for i in 0..LOG_LINES {
                    log.spawn((
                        LogLine(i),
                        Text::new(""),
                        TextFont::from_font_size(13.0),
                        TextColor(BODY),
                    ));
                }
            });
        });
}

fn tab_panel(
    p: &mut ChildSpawnerCommands,
    tab: Tab,
    content: impl FnOnce(&mut ChildSpawnerCommands),
) {
    p.spawn((
        TabPanel(tab),
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(4.0),
            ..default()
        },
    ))
    .with_children(content);
}

fn show_tab(
    keys: Res<ButtonInput<KeyCode>>,
    mut tab: ResMut<Tab>,
    mut panels: Query<(&TabPanel, &mut Node)>,
) {
    if keys.just_pressed(KeyCode::KeyG) {
        *tab = match *tab {
            Tab::Settlement => Tab::Gathering,
            Tab::Gathering => Tab::Settlement,
        };
    }
    for (panel, mut node) in &mut panels {
        let display = if panel.0 == *tab {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
}

fn update_banner(
    time: Res<Time>,
    mut sim: ResMut<Sim>,
    mut banner: Query<(&mut Text, &mut Node), With<Banner>>,
) {
    let Ok((mut text, mut node)) = banner.single_mut() else {
        return;
    };
    let remaining = sim.banner.as_ref().map(|(_, t)| *t - time.delta_secs());
    match (remaining, sim.banner.as_mut()) {
        (Some(t), Some(b)) if t > 0.0 => {
            b.1 = t;
            if text.0 != b.0 {
                text.0 = b.0.clone();
            }
            node.display = Display::Flex;
        }
        _ => {
            sim.banner = None;
            if node.display != Display::None {
                node.display = Display::None;
            }
        }
    }
}

fn toggle_panels(
    keys: Res<ButtonInput<KeyCode>>,
    mut visible: ResMut<UiVisible>,
    mut panels: Query<&mut Node, With<PanelRoot>>,
) {
    if !keys.just_pressed(KeyCode::KeyH) {
        return;
    }
    visible.0 = !visible.0;
    for mut node in &mut panels {
        node.display = if visible.0 {
            Display::Flex
        } else {
            Display::None
        };
    }
}

fn on_button(
    ev: On<Activate>,
    actions: Query<&UiAction>,
    mut sim: ResMut<Sim>,
    mut tab: ResMut<Tab>,
) {
    let Ok(&action) = actions.get(ev.entity) else {
        return;
    };
    let skill = sim
        .world
        .defs
        .skill_id("woodcutting")
        .expect("woodcutting skill");
    let available = sim.world.player.xp(skill);
    let min = sim.world.defs.soul_min_xp;
    match action {
        UiAction::TogglePause => sim.paused = !sim.paused,
        UiAction::Speed(s) => {
            sim.speed = s;
            sim.paused = false;
        }
        UiAction::Restart => {
            let s = sim.scenario;
            sim.restart(s);
        }
        UiAction::Load(s) => sim.restart(s),
        UiAction::SoulAdjust(d) => {
            sim.soul_amount = (sim.soul_amount as i64 + d).max(min as i64) as u64;
        }
        UiAction::SoulHalf => sim.soul_amount = (available / 2).max(min),
        UiAction::SoulAll => sim.soul_amount = available.max(min),
        UiAction::SoulSeparate => {
            let xp = sim.soul_amount;
            sim.separate_soul(skill, xp);
        }
        UiAction::ObjectiveAdjust(d) => {
            let target = (sim.world.settlement.objective.target as i32 + d).max(0) as u32;
            sim.command(Command::SetObjectiveTarget(target));
        }
        UiAction::ToggleConstruction => {
            let allow = !sim.world.settlement.allow_autonomous_construction;
            sim.command(Command::SetAutonomousConstruction(allow));
        }
        UiAction::ShowTab(t) => *tab = t,
        UiAction::ToggleSite(i) => {
            let open = !sim.world.settlement.site_open[i];
            sim.command(Command::SetSiteOpen {
                site: SiteId(i as u16),
                open,
            });
        }
    }
}

fn update_buttons(
    sim: Res<Sim>,
    tab: Res<Tab>,
    mut buttons: Query<(&UiAction, &mut BackgroundColor), With<Button>>,
    mut labels: Query<(&UiAction, &mut Text), Without<Section>>,
) {
    for (action, mut bg) in &mut buttons {
        let active = match action {
            UiAction::Speed(s) => *s == sim.speed && !sim.paused,
            UiAction::TogglePause => sim.paused,
            UiAction::ToggleConstruction => sim.world.settlement.allow_autonomous_construction,
            UiAction::ShowTab(t) => *t == *tab,
            UiAction::Load(s) => *s == sim.scenario,
            UiAction::ToggleSite(i) => sim.world.settlement.site_open[*i],
            _ => false,
        };
        bg.0 = if active { BUTTON_ACTIVE } else { BUTTON_BG };
    }
    for (action, mut text) in &mut labels {
        match action {
            UiAction::TogglePause => {
                text.0 = if sim.paused {
                    "Resume".into()
                } else {
                    "Pause".into()
                }
            }
            UiAction::ToggleConstruction => {
                let on = sim.world.settlement.allow_autonomous_construction;
                text.0 = format!("Autonomous building: {}", if on { "ON" } else { "OFF" });
            }
            UiAction::ToggleSite(i) => {
                let name = &sim.world.defs.sites[*i].name;
                let content = if sim.world.settlement.site_open[*i] {
                    format!("{name}: spirits allowed")
                } else {
                    format!("{name}: reserved for you")
                };
                if text.0 != content {
                    text.0 = content;
                }
            }
            _ => {}
        }
    }
}

fn update_texts(sim: Res<Sim>, mut texts: Query<(&Section, &mut Text)>) {
    let w = &sim.world;
    for (section, mut text) in &mut texts {
        let content = match section {
            Section::Header => header_text(&sim),
            Section::Player => player_text(&sim),
            Section::Soul => soul_text(&sim),
            Section::Settlement => settlement_text(w),
            Section::Spirits => spirits_text(w),
            Section::Planner => planner_text(w),
            Section::Gathering => gathering_text(w),
        };
        if text.0 != content {
            text.0 = content;
        }
    }
}

fn header_text(sim: &Sim) -> String {
    let w = &sim.world;
    let secs = w.tick / w.defs.ticks_per_second as u64;
    let scenario = match sim.scenario {
        ScenarioKind::NewGame => "New Game",
        ScenarioKind::Demo => "Demo",
        ScenarioKind::Regrind => "Regrind",
        ScenarioKind::GroveSustainable => "Grove: sustainable",
        ScenarioKind::GroveOversubscribed => "Grove: oversubscribed",
    };
    format!(
        "SpiritScape - {scenario}  |  Time {:02}:{:02}  |  {}\nWASD/click move, E use, C overview, wheel zoom, H hide UI\nSpace pause, 1-4 speed, F5 restart, G gathering tab",
        secs / 60,
        secs % 60,
        if sim.paused {
            "PAUSED".to_string()
        } else {
            format!("speed {}x", sim.speed)
        }
    )
}

fn player_text(sim: &Sim) -> String {
    let w = &sim.world;
    let skill = w.defs.skill_id("woodcutting").unwrap();
    let xp = w.player.xp(skill);
    let level = w.player.level(skill, &w.defs.xp_table);
    let next = match w.defs.xp_table.next_level_xp(xp) {
        Some(n) => format!("{n}"),
        None => "max".into(),
    };
    let logs = w.player.actor.inventory.get(w.defs.objective.resource);
    let h = w.player.history(skill);
    let (can, locked): (Vec<_>, Vec<_>) = w
        .defs
        .tree_tiers(skill)
        .into_iter()
        .partition(|k| w.player_can_cut(*k));
    let names = |ks: &[spirit_sim::defs::TreeKindId], with_level: bool| {
        ks.iter()
            .map(|k| {
                let d = w.defs.tree(*k);
                if with_level {
                    format!("{} (lvl {})", d.name, d.level_required)
                } else {
                    d.name.clone()
                }
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut access = format!("Can cut: {}", names(&can, false));
    if !locked.is_empty() {
        access += &format!("  |  Locked: {}", names(&locked, true));
    }
    let mut s = format!(
        "Woodcutting level {level}  (XP {xp} / next {next})  |  highest ever {}\nSacrificed {} XP into {} spirit(s)\n{access}\nCarrying {logs}/{} logs  |  {}",
        h.highest_level,
        h.xp_sacrificed,
        h.spirits_created,
        w.player.actor.inventory.capacity,
        w.player.activity
    );
    if !sim.message.is_empty() {
        s.push_str(&format!("\n> {}", sim.message));
    }
    s
}

fn soul_text(sim: &Sim) -> String {
    let w = &sim.world;
    let skill = w.defs.skill_id("woodcutting").unwrap();
    let p = w.soul_preview(skill, sim.soul_amount);
    let mut s = format!(
        "Transfer {} of your {} Woodcutting XP (min {})",
        p.transfer, p.current_xp, w.defs.soul_min_xp
    );
    if let Some(e) = &p.error {
        s += &format!("\nCan't separate: {e}");
        return s;
    }
    s += &format!(
        "\nYou: level {} -> {} ({} XP left)\nSpirit: permanent level {}, can cut {}",
        p.current_level,
        p.after_level,
        p.after_xp,
        p.spirit_level,
        if p.spirit_can_cut.is_empty() {
            "nothing".to_string()
        } else {
            tree_list(w, &p.spirit_can_cut)
        }
    );
    if p.lost.is_empty() {
        s += "\nYou keep access to every tree you can cut now.";
    } else {
        s += &format!(
            "\n!! You lose access to {} until you retrain.",
            tree_list(w, &p.lost)
        );
    }
    s
}

fn tree_list(w: &World, kinds: &[spirit_sim::defs::TreeKindId]) -> String {
    kinds
        .iter()
        .map(|k| w.defs.tree(*k).name.clone())
        .collect::<Vec<_>>()
        .join(", ")
}

fn time_use_line(t: &TimeUse) -> String {
    format!(
        "chop {}%  walk {}%  wait {}%  storage {}%",
        t.pct_of_working(t.gathering),
        t.pct_of_working(t.traveling),
        t.pct_of_working(t.waiting_recovery),
        t.pct_of_working(t.blocked_storage),
    )
}

fn gathering_text(w: &World) -> String {
    let r = w.gathering_report();
    let window = w.defs.gathering.window_ticks / w.defs.ticks_per_second as u64;
    let mut s = String::new();
    for site in &r.sites {
        s += &format!(
            "{}: {}/{} trees grown, {}/{} logs ready\n   measured {:.1} logs/min (last {window}s), sustainable ~{:.1}/min, {} spirit(s)\n",
            site.name,
            site.trees_grown,
            site.trees_total,
            site.logs_available,
            site.logs_max,
            site.measured_per_min,
            site.sustainable_per_min,
            site.workers
        );
    }
    s += &format!(
        "\nBottleneck: {}\n{}\nGatherers could chop up to {:.0} logs/min (estimate).",
        r.bottleneck.label(),
        r.summary,
        r.chop_capacity_per_min
    );
    let gatherers: Vec<_> = w
        .spirits
        .iter()
        .filter(|s| {
            w.defs
                .role(s.role)
                .has_job(spirit_sim::defs::JobKind::Gather)
        })
        .collect();
    if !gatherers.is_empty() {
        s += &format!("\n\nTime use (last {window}s):");
        for g in gatherers {
            let t = g.time.recent();
            let idle = (t.idle * 100).checked_div(t.total()).unwrap_or(0);
            s += &format!("\n{}: {}  |  idle {idle}%", g.name, time_use_line(&t));
        }
    }
    s
}

fn settlement_text(w: &World) -> String {
    let o = &w.settlement.objective;
    let res = &w.defs.resource(o.resource).name;
    let main = &w.main().storage;
    let projected = w.projected_stock(o.resource);
    let state = if projected >= o.target {
        "met"
    } else {
        "unmet"
    };
    let mut s = format!(
        "Objective: maintain {} {res} in Main Storage ({state})\nMain Storage: {} {res} ({} reserved), {} incl. stock en route",
        o.target,
        main.get(o.resource),
        main.reserved(o.resource),
        projected
    );
    for b in w
        .buildings
        .iter()
        .filter(|b| b.id != w.main_storage && b.is_complete())
    {
        s.push_str(&format!(
            "\n{} {}: {} {res}",
            b.name,
            b.pos,
            b.storage.get(o.resource)
        ));
    }
    s
}

fn spirits_text(w: &World) -> String {
    if w.spirits.is_empty() {
        return "No spirits yet. Earn Woodcutting XP, then use Soul Separation.".into();
    }
    w.spirits
        .iter()
        .map(|s| {
            let role = w.defs.role(s.role);
            let status = match &s.activity {
                Activity::Working(t) => t.clone(),
                Activity::Idle(t) => format!("Idle: {t}"),
                Activity::Blocked(t) => format!("BLOCKED: {t}"),
            };
            format!(
                "{} - {} (fixed {} lvl {}, {} XP)\n   {} | carrying {}/{}",
                s.name,
                role.name,
                w.defs.skill(s.skill()).name,
                s.level(),
                s.xp(),
                status,
                s.actor.inventory.total(),
                s.actor.inventory.capacity
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn planner_text(w: &World) -> String {
    let planner = &w.settlement.planner;
    let mut s = planner.status.clone();
    if let Some(report) = w.measure_logistics() {
        s.push_str(&format!(
            "\nRecent deliveries: avg {} tiles, walking {}% of trip time (limits {} tiles / {}%)",
            report.avg_leg_tiles,
            report.travel_share_pct,
            w.defs.planner.min_leg_tiles,
            w.defs.planner.travel_share_threshold_pct
        ));
    }
    for p in &w.projects {
        let def = w.defs.building(p.kind);
        let state = match p.state {
            ProjectState::AwaitingMaterials => "awaiting materials".to_string(),
            ProjectState::Supplying => format!(
                "supplying ({} on site)",
                w.defs.describe_amounts(&p.delivered)
            ),
            ProjectState::Building => format!(
                "building {}%",
                p.build_progress * 100 / p.build_required.max(1)
            ),
            ProjectState::Complete => "complete".to_string(),
        };
        let worker = p
            .assigned
            .map(|id| w.spirits[id].name.clone())
            .unwrap_or_else(|| "none".into());
        s.push_str(&format!(
            "\nProject #{}: {} at {} - {state}, builder: {worker}",
            p.id, def.name, p.site
        ));
        if let Some(b) = &p.blocked {
            s.push_str(&format!("\n   BLOCKED: {b}"));
        }
    }
    s
}

fn log_line(w: &World, newest_first: usize) -> String {
    let Some(e) = w.events.iter().rev().nth(newest_first) else {
        return String::new();
    };
    let secs = e.tick / w.defs.ticks_per_second as u64;
    let tag = match e.kind {
        EventKind::Player => "PLAYER",
        EventKind::Spirit => "SPIRIT",
        EventKind::Planner => "PLANNER",
        EventKind::Construction => "BUILD",
    };
    format!("[{:02}:{:02}] {tag:<7} {}", secs / 60, secs % 60, e.text)
}

fn update_log(sim: Res<Sim>, mut lines: Query<(&LogLine, &mut Text)>) {
    for (line, mut text) in &mut lines {
        let content = log_line(&sim.world, line.0);
        if text.0 != content {
            text.0 = content;
        }
    }
}

/// Hides log entries that don't fully fit, so long (wrapped) entries never
/// leave a half-cut line at the bottom. Uses the previous frame's layout.
fn fit_log(
    log_box: Query<(&ComputedNode, &UiGlobalTransform), With<LogBox>>,
    mut lines: Query<(&ComputedNode, &UiGlobalTransform, &mut Visibility), With<LogLine>>,
) {
    let Ok((box_node, box_t)) = log_box.single() else {
        return;
    };
    let box_bottom = box_t.translation.y + box_node.size().y / 2.0;
    for (node, t, mut vis) in &mut lines {
        let fits = t.translation.y + node.size().y / 2.0 <= box_bottom + 0.5;
        vis.set_if_neq(if fits {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
}
