//! Side panel and event log. The panel is a stack of cards (you, Soul
//! Separation, then a Settlement or Gathering tab); each card body is built
//! from bars, chips and short lines by `cards`, so state can be scanned
//! rather than read.

use crate::cards::{
    self, BAD, GOLD, GOOD, INFO, Item, Line, MUTED, Rendered, TEXT, WARN, bar, big, chip, colored,
    muted, text,
};
use crate::script::Cursor;
use crate::sim_runner::{SPEEDS, Sim};
use crate::status::{Status, spirit_status};
use bevy::input::mouse::{AccumulatedMouseScroll, MouseScrollUnit};
use bevy::picking::Pickable;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button};
use spirit_sim::defs::{JobKind, SiteId, TreeKindId};
use spirit_sim::entities::ProjectState;
use spirit_sim::events::EventKind;
use spirit_sim::metrics::Bottleneck;
use spirit_sim::{Command, ScenarioKind, World};

const PANEL_BG: Color = Color::srgba(0.06, 0.07, 0.09, 0.96);
const CARD_BG: Color = Color::srgb(0.11, 0.13, 0.16);
const BUTTON_BG: Color = Color::srgb(0.20, 0.24, 0.30);
const BUTTON_ACTIVE: Color = Color::srgb(0.25, 0.45, 0.30);
const HEADING: Color = Color::srgb(0.95, 0.85, 0.55);
const XP: Color = Color::srgb(0.70, 0.55, 1.0);
const LOG_LINES: usize = 11;
/// Width of the main bars in the panel.
const BAR: f32 = 220.0;
/// Gatherer time needed before the panel names a bottleneck.
const MIN_MEASURE_SECS: u64 = 20;

/// Screen space covered by the UI: right-hand panel and bottom event log.
pub const PANEL_WIDTH: f32 = 430.0;
pub const LOG_HEIGHT: f32 = 230.0;

/// Whether the panels are shown (`H` toggles them).
#[derive(Resource)]
pub struct UiVisible(pub bool);

#[derive(Component)]
struct PanelRoot;

/// The right-hand panel, which scrolls with the mouse wheel.
#[derive(Component)]
struct ScrollPanel;

/// Whether the cursor is over the right-hand panel (so the wheel scrolls it
/// instead of zooming the camera).
pub fn cursor_over_panel(window: &Window, cursor: &Cursor, ui_visible: bool) -> bool {
    ui_visible && cursor.0.is_some_and(|c| c.x > window.width() - PANEL_WIDTH)
}

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
    ToggleAllEvents,
}

/// Which half of the lower right panel is shown.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug)]
enum Tab {
    Settlement,
    Gathering,
}

/// Show routine spirit events in the log too.
#[derive(Resource)]
struct AllEvents(bool);

/// Development flag: open the Gathering tab at start (`--gathering`).
#[derive(Resource)]
pub struct StartOnGatheringTab(pub bool);

/// Root node of a tab's content.
#[derive(Component)]
struct TabPanel(Tab);

/// The large Soul Separation notice.
#[derive(Component)]
struct Banner;

/// A card body and what it shows.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Card {
    Header,
    Player,
    Soul,
    Objective,
    Spirits,
    Planner,
    Site(usize),
    Bottleneck,
    TimeUse,
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
            .insert_resource(AllEvents(false))
            .add_systems(Startup, build_ui)
            .add_systems(
                Update,
                (
                    update_cards,
                    update_log,
                    fit_log,
                    update_buttons,
                    toggle_panels,
                    show_tab,
                    update_banner,
                    scroll_panel,
                ),
            );
    }
}

/// A titled card; `content` adds its body and buttons.
fn card(
    p: &mut ChildSpawnerCommands,
    title: &str,
    content: impl FnOnce(&mut ChildSpawnerCommands),
) {
    p.spawn((
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(5.0),
            padding: UiRect::all(Val::Px(8.0)),
            border_radius: BorderRadius::all(Val::Px(6.0)),
            ..default()
        },
        BackgroundColor(CARD_BG),
    ))
    .with_children(|c| {
        if !title.is_empty() {
            c.spawn((
                Text::new(title),
                TextFont::from_font_size(12.0),
                TextColor(HEADING),
            ));
        }
        content(c);
    });
}

fn body(p: &mut ChildSpawnerCommands, kind: Card) {
    p.spawn((
        kind,
        Rendered::default(),
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(4.0),
            ..default()
        },
        Pickable::IGNORE,
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
                    border_radius: BorderRadius::all(Val::Px(4.0)),
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
                    TextColor(TEXT),
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
    let site_count = sim.world.defs.sites.len();
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Px(PANEL_WIDTH),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(8.0)),
                row_gap: Val::Px(6.0),
                overflow: Overflow::scroll_y(),
                ..default()
            },
            BackgroundColor(PANEL_BG),
            GlobalZIndex(1),
            PanelRoot,
            ScrollPanel,
            ScrollPosition::default(),
        ))
        .with_children(|p| {
            body(p, Card::Header);
            button_row(
                p,
                &[
                    ("Pause", UiAction::TogglePause),
                    ("1x", UiAction::Speed(SPEEDS[0])),
                    ("2x", UiAction::Speed(SPEEDS[1])),
                    ("4x", UiAction::Speed(SPEEDS[2])),
                    ("8x", UiAction::Speed(SPEEDS[3])),
                    ("Restart", UiAction::Restart),
                ],
            );
            button_row(
                p,
                &[
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

            card(p, "YOU", |c| body(c, Card::Player));

            card(p, "SOUL SEPARATION", |c| {
                body(c, Card::Soul);
                button_row(
                    c,
                    &[
                        ("-100", UiAction::SoulAdjust(-100)),
                        ("+100", UiAction::SoulAdjust(100)),
                        ("Half", UiAction::SoulHalf),
                        ("All", UiAction::SoulAll),
                        ("Separate Soul", UiAction::SoulSeparate),
                    ],
                );
            });

            button_row(
                p,
                &[
                    ("Settlement", UiAction::ShowTab(Tab::Settlement)),
                    ("Gathering (G)", UiAction::ShowTab(Tab::Gathering)),
                ],
            );

            tab_panel(p, Tab::Settlement, |p| {
                card(p, "STOCK", |c| {
                    body(c, Card::Objective);
                    button_row(
                        c,
                        &[
                            ("Target -10", UiAction::ObjectiveAdjust(-10)),
                            ("Target +10", UiAction::ObjectiveAdjust(10)),
                            ("Autonomous building", UiAction::ToggleConstruction),
                        ],
                    );
                });
                card(p, "SPIRITS", |c| body(c, Card::Spirits));
                card(p, "SETTLEMENT PLANNER", |c| body(c, Card::Planner));
            });

            tab_panel(p, Tab::Gathering, |p| {
                card(p, "WHAT LIMITS OUTPUT", |c| body(c, Card::Bottleneck));
                for i in 0..site_count {
                    card(p, "", |c| {
                        body(c, Card::Site(i));
                        button_row(c, &[("", UiAction::ToggleSite(i))]);
                    });
                }
                card(p, "WHERE GATHERERS' TIME GOES (LAST 2 MIN)", |c| {
                    body(c, Card::TimeUse)
                });
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
                    border_radius: BorderRadius::all(Val::Px(8.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.10, 0.06, 0.16, 0.92)),
                GlobalZIndex(5),
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
                row_gap: Val::Px(4.0),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(PANEL_BG),
            GlobalZIndex(1),
            PanelRoot,
        ))
        .with_children(|p| {
            p.spawn(Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(10.0),
                ..default()
            })
            .with_children(|row| {
                row.spawn((
                    Text::new("EVENT LOG - why things happen"),
                    TextFont::from_font_size(12.0),
                    TextColor(HEADING),
                ));
                button_row(row, &[("", UiAction::ToggleAllEvents)]);
            });
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
                        TextColor(TEXT),
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
            row_gap: Val::Px(6.0),
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

fn scroll_panel(
    scroll: Res<AccumulatedMouseScroll>,
    visible: Res<UiVisible>,
    cursor: Res<Cursor>,
    windows: Query<&Window>,
    mut panel: Query<(&mut ScrollPosition, &ComputedNode), With<ScrollPanel>>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    if scroll.delta.y == 0.0 || !cursor_over_panel(window, &cursor, visible.0) {
        return;
    }
    let step = match scroll.unit {
        MouseScrollUnit::Line => 40.0,
        MouseScrollUnit::Pixel => 1.0,
    };
    for (mut pos, node) in &mut panel {
        let max = ((node.content_size().y - node.size().y) * node.inverse_scale_factor()).max(0.0);
        pos.y = (pos.y - scroll.delta.y * step).clamp(0.0, max);
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
    mut all_events: ResMut<AllEvents>,
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
        UiAction::ToggleAllEvents => all_events.0 = !all_events.0,
    }
}

fn update_buttons(
    sim: Res<Sim>,
    tab: Res<Tab>,
    all_events: Res<AllEvents>,
    mut buttons: Query<(&UiAction, &mut BackgroundColor), With<Button>>,
    mut labels: Query<(&UiAction, &mut Text)>,
) {
    for (action, mut bg) in &mut buttons {
        let active = match action {
            UiAction::Speed(s) => *s == sim.speed && !sim.paused,
            UiAction::TogglePause => sim.paused,
            UiAction::ToggleConstruction => sim.world.settlement.allow_autonomous_construction,
            UiAction::ShowTab(t) => *t == *tab,
            UiAction::Load(s) => *s == sim.scenario,
            UiAction::ToggleSite(i) => sim.world.settlement.site_open[*i],
            UiAction::ToggleAllEvents => all_events.0,
            _ => false,
        };
        let color = if active { BUTTON_ACTIVE } else { BUTTON_BG };
        if bg.0 != color {
            bg.0 = color;
        }
    }
    for (action, mut text) in &mut labels {
        let content = match action {
            UiAction::TogglePause => {
                if sim.paused {
                    "Resume".into()
                } else {
                    "Pause".into()
                }
            }
            UiAction::ToggleConstruction => {
                let on = sim.world.settlement.allow_autonomous_construction;
                format!("Autonomous building: {}", if on { "ON" } else { "OFF" })
            }
            UiAction::ToggleSite(i) => {
                if sim.world.settlement.site_open[*i] {
                    "Reserve for me".into()
                } else {
                    "Allow spirits".into()
                }
            }
            UiAction::ToggleAllEvents => {
                if all_events.0 {
                    "Showing all events".into()
                } else {
                    "Showing important events".into()
                }
            }
            _ => continue,
        };
        if text.0 != content {
            text.0 = content;
        }
    }
}

fn update_cards(
    mut commands: Commands,
    sim: Res<Sim>,
    mut cards: Query<(Entity, &Card, &mut Rendered)>,
) {
    for (entity, card, mut rendered) in &mut cards {
        let lines = match card {
            Card::Header => header_lines(&sim),
            Card::Player => player_lines(&sim),
            Card::Soul => soul_lines(&sim),
            Card::Objective => objective_lines(&sim.world),
            Card::Spirits => spirit_lines(&sim.world),
            Card::Planner => planner_lines(&sim.world),
            Card::Site(i) => site_lines(&sim.world, *i),
            Card::Bottleneck => bottleneck_lines(&sim.world),
            Card::TimeUse => time_use_lines(&sim.world),
        };
        if rendered.changed(&lines) {
            cards::rebuild(&mut commands, entity, &lines);
        }
    }
}

fn header_lines(sim: &Sim) -> Vec<Line> {
    let w = &sim.world;
    let secs = w.tick / w.defs.ticks_per_second as u64;
    let scenario = match sim.scenario {
        ScenarioKind::NewGame => "New Game",
        ScenarioKind::Demo => "Demo",
        ScenarioKind::Regrind => "Regrind",
        ScenarioKind::GroveSustainable => "Grove: sustainable",
        ScenarioKind::GroveOversubscribed => "Grove: oversubscribed",
    };
    let speed = if sim.paused {
        colored("PAUSED", WARN)
    } else {
        muted(format!("speed {}x", sim.speed))
    };
    vec![
        vec![
            big("SpiritScape", HEADING),
            muted(scenario),
            Item::Spacer,
            text(format!("{:02}:{:02}", secs / 60, secs % 60)),
            speed,
        ],
        vec![muted(
            "WASD/click move  E use  C map  H hide  G tab  Space pause",
        )],
    ]
}

fn woodcutting(w: &World) -> spirit_sim::defs::SkillId {
    w.defs.skill_id("woodcutting").unwrap()
}

/// Progress through the current level, 0..=1.
fn level_progress(w: &World, xp: u64) -> (f32, Option<u64>) {
    let t = &w.defs.xp_table;
    let base = t.xp_for_level(t.level_for_xp(xp));
    match t.next_level_xp(xp) {
        Some(next) => ((xp - base) as f32 / (next - base).max(1) as f32, Some(next)),
        None => (1.0, None),
    }
}

/// One chip per tree tier: green if `can(kind)`, otherwise grey with its level.
fn tier_chips(w: &World, can: impl Fn(TreeKindId) -> bool) -> Vec<Item> {
    w.defs
        .tree_tiers(woodcutting(w))
        .into_iter()
        .map(|k| {
            let d = w.defs.tree(k);
            if can(k) {
                chip(&d.name, GOOD)
            } else {
                chip(format!("{} - needs Lv {}", d.name, d.level_required), MUTED)
            }
        })
        .collect()
}

fn player_lines(sim: &Sim) -> Vec<Line> {
    let w = &sim.world;
    let skill = woodcutting(w);
    let xp = w.player.xp(skill);
    let level = w.player.level(skill, &w.defs.xp_table);
    let (progress, next) = level_progress(w, xp);
    let h = w.player.history(skill);
    let inv = &w.player.actor.inventory;
    let logs = inv.get(w.defs.objective.resource);

    let mut trees = vec![muted("Can cut")];
    trees.extend(tier_chips(w, |k| w.player_can_cut(k)));
    let mut lines = vec![
        vec![
            big("Woodcutting", TEXT),
            Item::Spacer,
            big(format!("Lv {level}"), GOLD),
        ],
        vec![
            bar(progress, XP, BAR + 60.0),
            muted(match next {
                Some(n) => format!("{xp} / {n} XP"),
                None => format!("{xp} XP (max)"),
            }),
        ],
        trees,
        vec![
            muted("Bag"),
            bar(logs as f32 / inv.capacity.max(1) as f32, GOLD, 90.0),
            muted(format!("{logs}/{} logs", inv.capacity)),
            Item::Spacer,
            text(w.player.activity.clone()),
        ],
    ];
    if h.spirits_created > 0 {
        lines.push(vec![muted(format!(
            "Best ever Lv {}  |  {} XP given to {} spirit(s)",
            h.highest_level, h.xp_sacrificed, h.spirits_created
        ))]);
    }
    let message = if sim.message.is_empty() {
        " ".to_string()
    } else {
        sim.message.clone()
    };
    lines.push(vec![colored(message, INFO)]);
    lines
}

fn soul_lines(sim: &Sim) -> Vec<Line> {
    let w = &sim.world;
    let skill = woodcutting(w);
    let p = w.soul_preview(skill, sim.soul_amount);
    let mut lines = vec![vec![
        muted("Give"),
        big(format!("{} XP", p.transfer), XP),
        muted(format!(
            "of your {} (min {})",
            p.current_xp, w.defs.soul_min_xp
        )),
    ]];
    if let Some(e) = &p.error {
        lines.push(vec![colored(format!("Can't separate: {e}"), BAD)]);
        return lines;
    }
    let you_color = if p.lost.is_empty() { TEXT } else { BAD };
    lines.push(vec![
        muted("You"),
        colored(
            format!("Lv {} -> Lv {}", p.current_level, p.after_level),
            you_color,
        ),
        Item::Spacer,
        muted("New spirit"),
        colored(format!("Lv {} forever", p.spirit_level), GOLD),
    ]);
    let mut spirit = vec![muted("Spirit can cut")];
    if p.spirit_can_cut.is_empty() {
        spirit.push(colored("nothing", BAD));
    } else {
        spirit.extend(
            p.spirit_can_cut
                .iter()
                .map(|k| chip(&w.defs.tree(*k).name, GOOD)),
        );
    }
    lines.push(spirit);
    if p.lost.is_empty() {
        lines.push(vec![muted("You keep every tree you can cut now.")]);
    } else {
        let mut lost = vec![colored("You lose", BAD)];
        lost.extend(p.lost.iter().map(|k| chip(&w.defs.tree(*k).name, BAD)));
        lost.push(muted("until you retrain"));
        lines.push(lost);
    }
    lines
}

fn objective_lines(w: &World) -> Vec<Line> {
    let o = &w.settlement.objective;
    let main = &w.main().storage;
    let stock = main.get(o.resource);
    let projected = w.projected_stock(o.resource);
    let met = projected >= o.target;
    let mut lines = vec![vec![
        text("Main Storage"),
        bar(
            stock as f32 / o.target.max(1) as f32,
            if met { GOOD } else { WARN },
            150.0,
        ),
        text(format!("{stock} / {} logs", o.target)),
        Item::Spacer,
        if met {
            chip("target met", GOOD)
        } else {
            chip("gathering", WARN)
        },
    ]];
    if main.reserved(o.resource) > 0 || projected != stock {
        lines.push(vec![muted(format!(
            "{} reserved for building  |  {projected} counting logs on the way",
            main.reserved(o.resource)
        ))]);
    }
    for b in w
        .buildings
        .iter()
        .filter(|b| b.id != w.main_storage && b.is_complete())
    {
        lines.push(vec![
            text(b.name.clone()),
            muted(format!(
                "{} logs waiting to be hauled",
                b.storage.get(o.resource)
            )),
        ]);
    }
    lines
}

fn spirit_lines(w: &World) -> Vec<Line> {
    if w.spirits.is_empty() {
        return vec![vec![muted(
            "No spirits yet. Earn Woodcutting XP, then use Soul Separation.",
        )]];
    }
    let mut lines = Vec::new();
    for s in &w.spirits {
        let (status, word) = spirit_status(s);
        let role = &w.defs.role(s.role).name;
        let carried = s.actor.inventory.total();
        let mut line = vec![
            Item::Dot(status.color()),
            text(s.name.clone()),
            muted(format!("{role} Lv {}", s.level())),
            Item::Spacer,
            colored(word, status.color()),
        ];
        if carried > 0 && status != Status::Carrying {
            line.push(muted(format!("[{carried}]")));
        }
        lines.push(line);
        // The reason, when it isn't obvious from the status word.
        if matches!(status, Status::Waiting | Status::Blocked | Status::Idle) {
            lines.push(vec![muted(format!("     {}", s.activity.text()))]);
        }
    }
    lines
}

fn planner_lines(w: &World) -> Vec<Line> {
    let planner = &w.settlement.planner;
    let mut lines = vec![vec![muted(planner.status.clone())]];
    if let Some(report) = w.measure_logistics() {
        let limit = w.defs.planner.travel_share_threshold_pct;
        let walking = report.travel_share_pct;
        lines.push(vec![
            text("Trips"),
            muted(format!("{} tiles", report.avg_leg_tiles)),
            text("walking"),
            bar(
                walking as f32 / 100.0,
                if walking >= limit { WARN } else { GOOD },
                100.0,
            ),
            muted(format!("{walking}% (builds storage at {limit}%)")),
        ]);
    }
    for p in w
        .projects
        .iter()
        .filter(|p| p.state != ProjectState::Complete)
    {
        let def = w.defs.building(p.kind);
        let (frac, state) = match p.state {
            ProjectState::AwaitingMaterials => (0.0, "awaiting materials".to_string()),
            ProjectState::Supplying => {
                let cost: u32 = def.cost.iter().map(|c| c.1).sum();
                let have: u32 = p.delivered.values().sum();
                (
                    have as f32 / cost.max(1) as f32,
                    format!("materials {have}/{cost}"),
                )
            }
            ProjectState::Building => (
                p.build_progress as f32 / p.build_required.max(1) as f32,
                "building".into(),
            ),
            ProjectState::Complete => (1.0, "complete".into()),
        };
        let worker = p
            .assigned
            .map(|id| w.spirits[id].name.clone())
            .unwrap_or_else(|| "no builder".into());
        lines.push(vec![
            text(def.name.clone()),
            bar(frac, GOLD, 90.0),
            muted(format!("{state}, {worker}")),
        ]);
        if let Some(b) = &p.blocked {
            lines.push(vec![colored(format!("Blocked: {b}"), BAD)]);
        }
    }
    lines
}

fn availability_color(frac: f32) -> Color {
    if frac >= 0.6 {
        GOOD
    } else if frac >= 0.25 {
        WARN
    } else {
        BAD
    }
}

fn site_lines(w: &World, i: usize) -> Vec<Line> {
    let r = w.gathering_report();
    let Some(site) = r.sites.get(i) else {
        return vec![];
    };
    let frac = site.trees_grown as f32 / site.trees_total.max(1) as f32;
    let access = if site.spirits_allowed {
        chip("spirits allowed", GOOD)
    } else {
        chip("reserved for you", INFO)
    };
    let workers = match site.workers {
        0 => "no spirits".to_string(),
        1 => "1 spirit".to_string(),
        n => format!("{n} spirits"),
    };
    let rate = site.measured_per_min / site.sustainable_per_min.max(0.1);
    vec![
        vec![
            big(site.name.clone(), TEXT),
            muted(workers),
            Item::Spacer,
            access,
        ],
        vec![
            text("Trees   "),
            bar(frac, availability_color(frac), 120.0),
            text(format!(
                "{}/{} standing",
                site.trees_grown, site.trees_total
            )),
        ],
        vec![
            text("Logs/min"),
            bar(rate.min(1.0), if rate > 1.0 { WARN } else { INFO }, 120.0),
            text(format!("{:.0}", site.measured_per_min)),
            muted(format!("(sustains ~{:.0})", site.sustainable_per_min)),
        ],
    ]
}

fn bottleneck_lines(w: &World) -> Vec<Line> {
    let r = w.gathering_report();
    if r.gatherer_count == 0 {
        return vec![
            vec![big(r.bottleneck.label(), MUTED)],
            vec![text(r.summary.clone())],
        ];
    }
    // Shares from the first few seconds (one walk, one chop) aren't meaningful.
    let measured_secs =
        r.gatherers.total() / (r.gatherer_count as u64 * w.defs.ticks_per_second as u64).max(1);
    if measured_secs < MIN_MEASURE_SECS {
        return vec![
            vec![big("Still measuring", MUTED)],
            vec![muted(format!(
                "The limit is named after {MIN_MEASURE_SECS}s of work ({measured_secs}s so far)."
            ))],
        ];
    }
    let color = match r.bottleneck {
        Bottleneck::DemandMet => GOOD,
        Bottleneck::GatheringSpeed => INFO,
        Bottleneck::NoWorkers => MUTED,
        _ => WARN,
    };
    vec![
        vec![big(r.bottleneck.label(), color)],
        vec![text(r.summary.clone())],
        vec![muted(format!(
            "Gatherers could cut up to ~{:.0} logs/min if trees were always ready.",
            r.chop_capacity_per_min
        ))],
    ]
}

fn time_use_lines(w: &World) -> Vec<Line> {
    let chop = Status::Chopping.color();
    let walk = Status::Walking.color();
    let wait = Status::Waiting.color();
    let other = Status::Carrying.color();
    let idle = Status::Idle.color();
    let mut lines = vec![vec![
        Item::Dot(chop),
        muted("chop"),
        Item::Dot(walk),
        muted("walk"),
        Item::Dot(other),
        muted("unload/build"),
        Item::Dot(wait),
        muted("wait for trees"),
        Item::Dot(idle),
        muted("idle"),
    ]];
    let gatherers: Vec<_> = w
        .spirits
        .iter()
        .filter(|s| w.defs.role(s.role).has_job(JobKind::Gather))
        .collect();
    if gatherers.is_empty() {
        lines.push(vec![muted("No gathering spirits.")]);
    }
    for g in gatherers {
        let t = g.time.recent();
        let total = t.total().max(1) as f32;
        let f = |n: u64| n as f32 / total;
        lines.push(vec![
            text(g.name.clone()),
            Item::Spacer,
            Item::Stack {
                parts: [
                    (f(t.gathering), chop),
                    (f(t.traveling), walk),
                    (f(t.handling + t.building), other),
                    (f(t.waiting_recovery), wait),
                    (f(t.blocked_storage + t.blocked_other), BAD),
                    (f(t.idle), idle),
                ]
                .into_iter()
                .map(|(x, c)| ((x * 50.0).round() / 50.0, c))
                .collect(),
                width: 260.0,
            },
        ]);
    }
    lines
}

fn kind_color(kind: EventKind) -> (&'static str, Color) {
    match kind {
        EventKind::Player => ("YOU", GOLD),
        EventKind::Spirit => ("SPIRIT", Color::srgb(0.80, 0.84, 0.90)),
        EventKind::Planner => ("PLANNER", INFO),
        EventKind::Construction => ("BUILD", Color::srgb(1.0, 0.85, 0.35)),
    }
}

fn update_log(
    sim: Res<Sim>,
    all: Res<AllEvents>,
    mut lines: Query<(&LogLine, &mut Text, &mut TextColor)>,
) {
    let w = &sim.world;
    let shown: Vec<_> = w
        .events
        .iter()
        .rev()
        .filter(|e| all.0 || !e.detail)
        .take(LOG_LINES)
        .collect();
    for (line, mut text, mut color) in &mut lines {
        let (content, c) = match shown.get(line.0) {
            Some(e) => {
                let secs = e.tick / w.defs.ticks_per_second as u64;
                let (tag, c) = kind_color(e.kind);
                (
                    format!("{:02}:{:02}  {tag:<8}{}", secs / 60, secs % 60, e.text),
                    c,
                )
            }
            None => (String::new(), TEXT),
        };
        if text.0 != content {
            text.0 = content;
        }
        if color.0 != c {
            color.0 = c;
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
