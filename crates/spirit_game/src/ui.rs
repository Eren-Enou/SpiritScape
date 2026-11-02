//! Functional UI: status panel, Soul Separation, objective and simulation
//! controls, spirit roster, planner/project status and the event log.

use crate::sim_runner::{SPEEDS, Sim};
use bevy::picking::Pickable;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button};
use spirit_sim::entities::{Activity, ProjectState};
use spirit_sim::events::EventKind;
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
}

#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Section {
    Header,
    Player,
    Soul,
    Settlement,
    Spirits,
    Planner,
    Log,
}

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(UiVisible(true))
            .add_systems(Startup, build_ui)
            .add_systems(Update, (update_texts, update_buttons, toggle_panels));
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

fn build_ui(mut commands: Commands) {
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
                    ("Demo Scenario", UiAction::Load(ScenarioKind::Demo)),
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
            section(p, Section::Log);
        });
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

fn on_button(ev: On<Activate>, actions: Query<&UiAction>, mut sim: ResMut<Sim>) {
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
            sim.command(Command::SoulSeparate { skill, xp });
        }
        UiAction::ObjectiveAdjust(d) => {
            let target = (sim.world.settlement.objective.target as i32 + d).max(0) as u32;
            sim.command(Command::SetObjectiveTarget(target));
        }
        UiAction::ToggleConstruction => {
            let allow = !sim.world.settlement.allow_autonomous_construction;
            sim.command(Command::SetAutonomousConstruction(allow));
        }
    }
}

fn update_buttons(
    sim: Res<Sim>,
    mut buttons: Query<(&UiAction, &mut BackgroundColor), With<Button>>,
    mut labels: Query<(&UiAction, &mut Text), Without<Section>>,
) {
    for (action, mut bg) in &mut buttons {
        let active = match action {
            UiAction::Speed(s) => *s == sim.speed && !sim.paused,
            UiAction::TogglePause => sim.paused,
            UiAction::ToggleConstruction => sim.world.settlement.allow_autonomous_construction,
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
            Section::Log => log_text(w),
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
        ScenarioKind::Demo => "Demo Scenario",
    };
    format!(
        "SpiritScape - {scenario}  |  Time {:02}:{:02}  |  {}\nWASD/click move, E use, C overview, wheel zoom, H hide UI\nSpace pause, 1-4 speed, F5 restart",
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
    let mut s = format!(
        "Woodcutting level {level}  (XP {xp} / next {next})\nCarrying {logs}/{} logs  |  {}\nClick or face a tree and press E to chop; a storage to deposit.",
        w.player.actor.inventory.capacity, w.player.activity
    );
    if !sim.message.is_empty() {
        s.push_str(&format!("\n> {}", sim.message));
    }
    s
}

fn soul_text(sim: &Sim) -> String {
    let w = &sim.world;
    let skill = w.defs.skill_id("woodcutting").unwrap();
    let have = w.player.xp(skill);
    let amount = sim.soul_amount;
    let table = &w.defs.xp_table;
    let preview = if amount > have {
        format!("Not enough XP (you have {have}).")
    } else {
        format!(
            "Spirit would be Woodcutting level {} (permanent).\nYou would drop from level {} to {} ({} XP left).",
            table.level_for_xp(amount),
            table.level_for_xp(have),
            table.level_for_xp(have - amount),
            have - amount
        )
    };
    format!(
        "Sacrifice {amount} Woodcutting XP (min {}).\n{preview}",
        w.defs.soul_min_xp
    )
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

fn log_text(w: &World) -> String {
    let tps = w.defs.ticks_per_second as u64;
    let lines: Vec<String> = w
        .events
        .iter()
        .rev()
        .take(LOG_LINES)
        .map(|e| {
            let secs = e.tick / tps;
            let tag = match e.kind {
                EventKind::Player => "PLAYER",
                EventKind::Spirit => "SPIRIT",
                EventKind::Planner => "PLANNER",
                EventKind::Construction => "BUILD",
            };
            format!("[{:02}:{:02}] {tag:<7} {}", secs / 60, secs % 60, e.text)
        })
        .collect();
    lines.join("\n")
}
