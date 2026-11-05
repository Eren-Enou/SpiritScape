//! Title menu, side panel and event log. The panel is a stack of cards (goal,
//! you, Soul Separation, then a tab); each card body is built from bars,
//! chips and short lines by `cards`, so state can be scanned rather than read.

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
use spirit_sim::defs::{BuildingKindId, JobKind, SiteId, SkillId};
use spirit_sim::entities::ProjectState;
use spirit_sim::events::EventKind;
use spirit_sim::metrics::{Bottleneck, TimeUse};
use spirit_sim::production::{ProductionBottleneck, WorkshopStatus};
use spirit_sim::session::ActivityState;
use spirit_sim::training::TrainingMode;
use spirit_sim::{Command, ScenarioKind, World};

const PANEL_BG: Color = Color::srgba(0.06, 0.07, 0.09, 0.96);
const CARD_BG: Color = Color::srgb(0.11, 0.13, 0.16);
const BUTTON_BG: Color = Color::srgb(0.20, 0.24, 0.30);
const BUTTON_ACTIVE: Color = Color::srgb(0.25, 0.45, 0.30);
const HEADING: Color = Color::srgb(0.95, 0.85, 0.55);
const XP: Color = Color::srgb(0.70, 0.55, 1.0);
const LOG_LINES: usize = 11;
/// Gatherer time needed before the panel names a bottleneck.
const MIN_MEASURE_SECS: u64 = 20;
/// Worker assignment buttons in the Production tab.
const WORKER_BUTTONS: usize = 3;
/// Sacrifice options compared in the Soul Separation card.
const SOUL_OPTIONS: usize = 5;
/// Recent milestones listed in the Progress tab.
const RECENT_NOTICES: usize = 6;
/// Planned buildings with buttons in the Projects tab.
const PLAN_ROWS: usize = 4;
/// Further orders with buttons in the Production tab.
const EXTRA_ORDER_BUTTONS: usize = 2;
const EXTRA_ORDER_LESS: [&str; 2] = ["Beams -5", "Panels -5"];
const EXTRA_ORDER_MORE: [&str; 2] = ["Beams +5", "Panels +5"];

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
    SoulSkill(SkillId),
    PlankAdjust(i32),
    ReserveAdjust(i32),
    ToggleProduction,
    /// The nth spirit that can work at a station.
    ToggleWorker(usize),
    OrderBuilding,
    SaveGame,
    OpenMenu,
    MenuContinue,
    MenuNew(&'static str),
    MenuBack,
    /// Choose the nth option in the Soul Separation comparison.
    SoulOption(usize),
    /// Make the nth site the training area, or stop if it already is.
    TrainingArea(usize),
    BuildCache,
    /// Adjust the nth further order's target.
    ExtraTarget(usize, i32),
    /// Put the nth further order first, or back level with the rest.
    ExtraPriority(usize),
    PlaceWorkshop,
    BuildRack,
    ToggleKeepPlace,
    SpiritPrev,
    SpiritNext,
    CycleFocus,
    CycleSite,
    CycleStation,
    ToggleAdvanced,
    ToggleCollect,
    ToggleSupply,
    ToggleTrainingFirst,
    /// Stop what the player is doing.
    StopActivity,
    ToggleAutoGather,
    /// Step through the recipes the player can use.
    CycleRecipe,
    /// Make the best recipe available.
    RecipeAuto,
    /// Choose the nth recipe at the bench.
    ChooseRecipe(usize),
    SetMode(TrainingMode),
    /// Step through the benches to train at.
    CycleBench,
    PlaceStore,
    /// Send everyone who can to the outpost, or release them.
    ToggleOutpost,
    /// Change what the outpost store keeps.
    LocalStock(i32),
    /// Train at the outpost's bench.
    TrainHere,
    /// Add a building to the plan, by its id.
    Plan(&'static str),
    /// Move the nth planned building up.
    PlanUp(usize),
    /// Take the nth planned building off the plan.
    PlanCancel(usize),
    /// Call off the current project.
    PlanCancelActive,
    /// Show or hide the detail cards.
    ToggleDetails,
    /// Show or hide the development scenario buttons.
    ToggleScenarios,
    WriteReport,
}

/// Which half of the lower right panel is shown.
#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug)]
enum Tab {
    Settlement,
    Gathering,
    Production,
    Progress,
    Training,
    Spirits,
    Soul,
    Outpost,
    Projects,
}

/// Show routine spirit events in the log too.
#[derive(Resource)]
struct AllEvents(bool);

/// Show the cards meant for tuning and diagnosis, not everyday play.
#[derive(Resource)]
struct Details(bool);

/// Show the buttons that load development scenarios.
#[derive(Resource)]
struct ScenarioButtons(bool);

/// A card shown only with details on.
#[derive(Component)]
struct DetailCard;

/// Summary cards that make way for the tall Soul and Training tabs.
#[derive(Component)]
struct OverviewCard;

/// Development flags: open a tab at start (`--gathering`, `--production`).
#[derive(Resource)]
pub struct StartTab {
    pub gathering: bool,
    pub production: bool,
}

/// Root node of a tab's content.
#[derive(Component)]
struct TabPanel(Tab);

/// The large Soul Separation notice.
#[derive(Component)]
struct Banner;

/// The full-screen title menu.
#[derive(Component)]
struct MenuRoot;

/// The goal card, which only playthroughs show.
#[derive(Component)]
struct GoalRoot;

/// A card body and what it shows.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Card {
    Ambitions,
    Header,
    Player,
    Soul,
    Objective,
    Spirits,
    Planner,
    Site(usize),
    Bottleneck,
    TimeUse,
    Production,
    ProductionLimit,
    ChainTime,
    Goal,
    Menu,
    Skills,
    Workforce,
    Improvements,
    Recent,
    TrainingStatus,
    PlayerTime,
    Orders,
    Stations,
    SpiritOrders,
    Activity,
    Glance,
    Recipes,
    TrainingPlan,
    PlayReport,
    Outpost,
    ProjectNeeds,
    Projects,
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
            .insert_resource(Details(false))
            .insert_resource(ScenarioButtons(false))
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
                    show_details,
                    update_banner,
                    scroll_panel,
                    update_menu,
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

/// A card for tuning and diagnosis, hidden until details are on.
fn detail_card(
    p: &mut ChildSpawnerCommands,
    title: &str,
    content: impl FnOnce(&mut ChildSpawnerCommands),
) {
    p.spawn((
        DetailCard,
        Node {
            flex_direction: FlexDirection::Column,
            display: Display::None,
            ..default()
        },
    ))
    .with_children(|c| card(c, title, content));
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

fn build_ui(mut commands: Commands, sim: Res<Sim>, start: Res<StartTab>, mut tab: ResMut<Tab>) {
    if start.gathering {
        *tab = Tab::Gathering;
    }
    if start.production {
        *tab = Tab::Production;
    }
    let site_count = sim.world.defs.sites.len();
    let area_buttons: Vec<(&str, UiAction)> = (0..site_count)
        .map(|i| ("", UiAction::TrainingArea(i)))
        .collect();
    let soul_skills: Vec<(String, UiAction)> = soul_skills(&sim.world)
        .into_iter()
        .map(|s| {
            let name = sim.world.defs.skill(s).name.clone();
            (name, UiAction::SoulSkill(s))
        })
        .collect();
    let soul_buttons: Vec<(&str, UiAction)> =
        soul_skills.iter().map(|(n, a)| (n.as_str(), *a)).collect();
    let recipe_count = sim.world.recipe_options().len();
    let mut recipe_buttons = vec![("Best available", UiAction::RecipeAuto)];
    recipe_buttons.extend((0..recipe_count).map(|i| ("", UiAction::ChooseRecipe(i))));
    let mode_buttons: Vec<(&str, UiAction)> = TrainingMode::ALL
        .into_iter()
        .map(|m| (m.label(), UiAction::SetMode(m)))
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
                    ("Save", UiAction::SaveGame),
                    ("Menu", UiAction::OpenMenu),
                    ("Scenarios", UiAction::ToggleScenarios),
                ],
            );
            button_row(
                p,
                &[
                    ("Blank", UiAction::Load(ScenarioKind::NewGame)),
                    ("Demo", UiAction::Load(ScenarioKind::Demo)),
                    ("Regrind", UiAction::Load(ScenarioKind::Regrind)),
                    ("Grove OK", UiAction::Load(ScenarioKind::GroveSustainable)),
                    (
                        "Grove Over",
                        UiAction::Load(ScenarioKind::GroveOversubscribed),
                    ),
                ],
            );
            button_row(
                p,
                &[
                    ("Workshop A", UiAction::Load(ScenarioKind::WorkshopManual)),
                    (
                        "Workshop B",
                        UiAction::Load(ScenarioKind::WorkshopAutomated),
                    ),
                    (
                        "Workshop C",
                        UiAction::Load(ScenarioKind::WorkshopConstruction),
                    ),
                    (
                        "Special",
                        UiAction::Load(ScenarioKind::AdvancedSpecialization),
                    ),
                    (
                        "Bottleneck",
                        UiAction::Load(ScenarioKind::ProductionBottleneck),
                    ),
                    (
                        "Expansion",
                        UiAction::Load(ScenarioKind::ExpandedProduction),
                    ),
                    (
                        "Retraining",
                        UiAction::Load(ScenarioKind::SupportedRetraining),
                    ),
                    ("Skilling", UiAction::Load(ScenarioKind::SkillingSession)),
                    ("Recipes", UiAction::Load(ScenarioKind::WorkshopRecipes)),
                    ("Full storage", UiAction::Load(ScenarioKind::FullStorage)),
                    ("Central", UiAction::Load(ScenarioKind::CentralProduction)),
                    ("Remote", UiAction::Load(ScenarioKind::RemoteProduction)),
                    (
                        "Outpost",
                        UiAction::Load(ScenarioKind::OutpostEstablishment),
                    ),
                ],
            );

            card(p, "NOW", |c| {
                body(c, Card::Activity);
                button_row(
                    c,
                    &[
                        ("Stop (X)", UiAction::StopActivity),
                        ("", UiAction::ToggleAutoGather),
                        ("", UiAction::CycleRecipe),
                    ],
                );
            });

            p.spawn((
                GoalRoot,
                Node {
                    flex_direction: FlexDirection::Column,
                    ..default()
                },
            ))
            .with_children(|g| card(g, "WHAT'S NEXT", |c| body(c, Card::Goal)));

            p.spawn((
                OverviewCard,
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(6.0),
                    ..default()
                },
            ))
            .with_children(|o| {
                card(o, "YOU", |c| body(c, Card::Player));
                card(o, "SETTLEMENT", |c| body(c, Card::Glance));
            });

            button_row(
                p,
                &[
                    ("Settlement", UiAction::ShowTab(Tab::Settlement)),
                    ("Gathering (G)", UiAction::ShowTab(Tab::Gathering)),
                    ("Production (P)", UiAction::ShowTab(Tab::Production)),
                    ("Progress (J)", UiAction::ShowTab(Tab::Progress)),
                    ("Training (T)", UiAction::ShowTab(Tab::Training)),
                    ("Spirits (O)", UiAction::ShowTab(Tab::Spirits)),
                    ("Soul (R)", UiAction::ShowTab(Tab::Soul)),
                    ("Outpost (U)", UiAction::ShowTab(Tab::Outpost)),
                    ("Projects (B)", UiAction::ShowTab(Tab::Projects)),
                    ("", UiAction::ToggleDetails),
                ],
            );

            tab_panel(p, Tab::Projects, |p| {
                card(p, "SETTLEMENT PLAN", |c| {
                    body(c, Card::Projects);
                    button_row(c, &[("", UiAction::PlanCancelActive)]);
                    for i in 0..PLAN_ROWS {
                        button_row(
                            c,
                            &[("", UiAction::PlanUp(i)), ("", UiAction::PlanCancel(i))],
                        );
                    }
                });
                card(p, "ADD TO THE PLAN", |c| {
                    button_row(
                        c,
                        &[
                            ("", UiAction::Plan("outpost_storage")),
                            ("", UiAction::Plan("forest_crossing")),
                            ("", UiAction::Plan("workshop_annex")),
                            ("", UiAction::Plan("storehouse")),
                            ("", UiAction::Plan("grove_lane")),
                            ("", UiAction::Plan("north_road")),
                            ("", UiAction::Plan("old_growth_road")),
                            ("", UiAction::Plan("forest_road")),
                            ("", UiAction::Plan("stone_causeway")),
                            ("", UiAction::Plan("panelled_warehouse")),
                            ("", UiAction::Plan("carrying_frames")),
                        ],
                    );
                });
            });

            tab_panel(p, Tab::Outpost, |p| {
                card(p, "OUTPOST", |c| {
                    body(c, Card::Outpost);
                    button_row(
                        c,
                        &[("", UiAction::PlaceStore), ("", UiAction::PlaceWorkshop)],
                    );
                    button_row(
                        c,
                        &[
                            ("", UiAction::ToggleOutpost),
                            ("Train here", UiAction::TrainHere),
                        ],
                    );
                    button_row(
                        c,
                        &[
                            ("Keep -5", UiAction::LocalStock(-5)),
                            ("Keep +5", UiAction::LocalStock(5)),
                            ("Beams -5", UiAction::ExtraTarget(0, -5)),
                            ("Beams +5", UiAction::ExtraTarget(0, 5)),
                        ],
                    );
                });
                card(p, "WHAT THE PROJECT NEEDS", |c| body(c, Card::ProjectNeeds));
            });

            tab_panel(p, Tab::Soul, |p| {
                card(p, "SOUL SEPARATION", |c| {
                    button_row(c, &soul_buttons);
                    body(c, Card::Soul);
                    let options: Vec<_> = (0..SOUL_OPTIONS)
                        .map(|i| ("", UiAction::SoulOption(i)))
                        .collect();
                    button_row(c, &options);
                    button_row(
                        c,
                        &[
                            ("-100", UiAction::SoulAdjust(-100)),
                            ("+100", UiAction::SoulAdjust(100)),
                            ("Half", UiAction::SoulHalf),
                            ("All", UiAction::SoulAll),
                        ],
                    );
                    button_row(c, &[("Separate Soul", UiAction::SoulSeparate)]);
                });
            });

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
                detail_card(p, "SETTLEMENT PLANNER", |c| body(c, Card::Planner));
            });

            tab_panel(p, Tab::Gathering, |p| {
                card(p, "WHAT LIMITS OUTPUT", |c| body(c, Card::Bottleneck));
                for i in 0..site_count {
                    card(p, "", |c| {
                        body(c, Card::Site(i));
                        button_row(c, &[("", UiAction::ToggleSite(i))]);
                    });
                }
                detail_card(p, "WHERE GATHERERS' TIME GOES (LAST 2 MIN)", |c| {
                    body(c, Card::TimeUse)
                });
            });

            tab_panel(p, Tab::Production, |p| {
                card(p, "WHAT LIMITS PLANKS", |c| body(c, Card::ProductionLimit));
                card(p, "ORDERS", |c| {
                    body(c, Card::Orders);
                    for i in 0..EXTRA_ORDER_BUTTONS {
                        button_row(
                            c,
                            &[
                                (EXTRA_ORDER_LESS[i], UiAction::ExtraTarget(i, -5)),
                                (EXTRA_ORDER_MORE[i], UiAction::ExtraTarget(i, 5)),
                                ("", UiAction::ExtraPriority(i)),
                            ],
                        );
                    }
                });
                card(p, "WORKSHOPS", |c| {
                    body(c, Card::Stations);
                    button_row(c, &[("", UiAction::PlaceWorkshop)]);
                });
                card(p, "PRODUCTION LINE", |c| {
                    body(c, Card::Production);
                    button_row(
                        c,
                        &[
                            ("Planks -10", UiAction::PlankAdjust(-10)),
                            ("Planks +10", UiAction::PlankAdjust(10)),
                            ("Reserve -5", UiAction::ReserveAdjust(-5)),
                            ("Reserve +5", UiAction::ReserveAdjust(5)),
                        ],
                    );
                    button_row(c, &[("", UiAction::ToggleProduction)]);
                    let workers: Vec<_> = (0..WORKER_BUTTONS)
                        .map(|i| ("", UiAction::ToggleWorker(i)))
                        .collect();
                    button_row(c, &workers);
                    button_row(c, &[("", UiAction::OrderBuilding)]);
                });
                detail_card(p, "WHERE THE CHAIN'S TIME GOES (LAST 2 MIN)", |c| {
                    body(c, Card::ChainTime)
                });
            });

            tab_panel(p, Tab::Progress, |p| {
                card(p, "AMBITIONS", |c| body(c, Card::Ambitions));
                card(p, "YOUR SKILLS", |c| body(c, Card::Skills));
                card(p, "YOUR WORKFORCE", |c| body(c, Card::Workforce));
                card(p, "SETTLEMENT IMPROVEMENTS", |c| {
                    body(c, Card::Improvements)
                });
                card(p, "RECENTLY", |c| body(c, Card::Recent));
                detail_card(p, "PLAY REPORT", |c| {
                    body(c, Card::PlayReport);
                    button_row(c, &[("Write report file (F6)", UiAction::WriteReport)]);
                });
            });

            tab_panel(p, Tab::Spirits, |p| {
                card(p, "INSTRUCTIONS", |c| {
                    button_row(
                        c,
                        &[
                            ("Previous spirit", UiAction::SpiritPrev),
                            ("Next spirit", UiAction::SpiritNext),
                        ],
                    );
                    body(c, Card::SpiritOrders);
                    button_row(
                        c,
                        &[
                            ("", UiAction::CycleFocus),
                            ("", UiAction::CycleSite),
                            ("", UiAction::CycleStation),
                            ("", UiAction::ToggleAdvanced),
                        ],
                    );
                });
            });

            tab_panel(p, Tab::Training, |p| {
                card(p, "HOW YOU TRAIN", |c| {
                    button_row(c, &mode_buttons);
                    body(c, Card::TrainingPlan);
                });
                card(p, "WHAT YOU MAKE AT THE BENCH", |c| {
                    button_row(c, &[("", UiAction::CycleBench)]);
                    button_row(c, &recipe_buttons);
                    body(c, Card::Recipes);
                });
                card(p, "SPIRIT-SUPPORTED TRAINING", |c| {
                    body(c, Card::TrainingStatus);
                    button_row(c, &area_buttons);
                    button_row(c, &[("", UiAction::BuildCache)]);
                    button_row(
                        c,
                        &[
                            ("", UiAction::ToggleCollect),
                            ("", UiAction::ToggleSupply),
                            ("", UiAction::ToggleTrainingFirst),
                            ("", UiAction::ToggleKeepPlace),
                            ("", UiAction::BuildRack),
                        ],
                    );
                });
                detail_card(p, "WHERE YOUR TIME GOES (WHOLE GAME)", |c| {
                    body(c, Card::PlayerTime)
                });
            });
        });

    commands
        .spawn((
            MenuRoot,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                top: Val::Px(0.0),
                bottom: Val::Px(0.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                display: Display::None,
                ..default()
            },
            BackgroundColor(Color::srgba(0.03, 0.04, 0.06, 0.94)),
            GlobalZIndex(20),
        ))
        .with_children(|p| {
            p.spawn((
                Node {
                    width: Val::Px(560.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(14.0),
                    padding: UiRect::all(Val::Px(24.0)),
                    border_radius: BorderRadius::all(Val::Px(8.0)),
                    ..default()
                },
                BackgroundColor(CARD_BG),
            ))
            .with_children(|c| {
                body(c, Card::Menu);
                button_row(
                    c,
                    &[
                        ("Continue", UiAction::MenuContinue),
                        ("New Game", UiAction::MenuNew("normal")),
                        ("Fast Game", UiAction::MenuNew("development")),
                        ("Back", UiAction::MenuBack),
                    ],
                );
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
    mut panels: Query<(&TabPanel, &mut Node), Without<OverviewCard>>,
    mut overview: Query<&mut Node, With<OverviewCard>>,
) {
    if keys.just_pressed(KeyCode::KeyG) {
        *tab = match *tab {
            Tab::Gathering => Tab::Settlement,
            _ => Tab::Gathering,
        };
    }
    if keys.just_pressed(KeyCode::KeyP) {
        *tab = match *tab {
            Tab::Production => Tab::Settlement,
            _ => Tab::Production,
        };
    }
    if keys.just_pressed(KeyCode::KeyJ) {
        *tab = match *tab {
            Tab::Progress => Tab::Settlement,
            _ => Tab::Progress,
        };
    }
    if keys.just_pressed(KeyCode::KeyT) {
        *tab = match *tab {
            Tab::Training => Tab::Settlement,
            _ => Tab::Training,
        };
    }
    if keys.just_pressed(KeyCode::KeyO) {
        *tab = match *tab {
            Tab::Spirits => Tab::Settlement,
            _ => Tab::Spirits,
        };
    }
    if keys.just_pressed(KeyCode::KeyR) {
        *tab = match *tab {
            Tab::Soul => Tab::Settlement,
            _ => Tab::Soul,
        };
    }
    if keys.just_pressed(KeyCode::KeyU) {
        *tab = match *tab {
            Tab::Outpost => Tab::Settlement,
            _ => Tab::Outpost,
        };
    }
    if keys.just_pressed(KeyCode::KeyB) {
        *tab = match *tab {
            Tab::Projects => Tab::Settlement,
            _ => Tab::Projects,
        };
    }
    let tall = matches!(
        *tab,
        Tab::Soul | Tab::Training | Tab::Outpost | Tab::Projects
    );
    let display = if tall { Display::None } else { Display::Flex };
    for mut node in &mut overview {
        if node.display != display {
            node.display = display;
        }
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

fn show_details(
    keys: Res<ButtonInput<KeyCode>>,
    mut details: ResMut<Details>,
    mut cards: Query<&mut Node, With<DetailCard>>,
) {
    if keys.just_pressed(KeyCode::KeyV) {
        details.0 = !details.0;
    }
    let display = if details.0 {
        Display::Flex
    } else {
        Display::None
    };
    for mut node in &mut cards {
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

fn update_menu(
    sim: Res<Sim>,
    mut menu: Query<&mut Node, (With<MenuRoot>, Without<GoalRoot>)>,
    mut goal: Query<&mut Node, (With<GoalRoot>, Without<MenuRoot>)>,
) {
    let shown = |on: bool| if on { Display::Flex } else { Display::None };
    for mut node in &mut menu {
        if node.display != shown(sim.menu) {
            node.display = shown(sim.menu);
        }
    }
    for mut node in &mut goal {
        if node.display != shown(sim.is_journey()) {
            node.display = shown(sim.is_journey());
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
    mut details: ResMut<Details>,
    mut scenario_buttons: ResMut<ScenarioButtons>,
) {
    let Ok(&action) = actions.get(ev.entity) else {
        return;
    };
    let skill = sim.soul_skill;
    let available = sim.world.player.xp(skill);
    let min = sim.world.defs.soul_min_xp;
    // Anything but the second press calls the ritual off.
    if action != UiAction::SoulSeparate {
        sim.soul_armed = false;
    }
    match action {
        UiAction::SaveGame => sim.save_game(),
        UiAction::StopActivity => sim.command(Command::PlayerStop),
        UiAction::Plan(id) => {
            if let Some(kind) = sim.world.defs.building_kind_id(id) {
                sim.command(Command::QueueBuilding { kind, site: None });
            }
        }
        UiAction::PlanUp(i) => {
            if let Some(q) = sim.world.settlement.queue.get(i) {
                let id = q.id;
                sim.command(Command::MoveQueued { id, up: true });
            }
        }
        UiAction::PlanCancel(i) => {
            if let Some(q) = sim.world.settlement.queue.get(i) {
                let id = q.id;
                sim.command(Command::CancelQueued(id));
            }
        }
        UiAction::PlanCancelActive => {
            if let Some(p) = sim.world.active_project() {
                let id = p.id;
                sim.command(Command::CancelProject(id));
            }
        }
        UiAction::CycleBench => {
            let ids: Vec<usize> = sim.world.stations().map(|b| b.id).collect();
            let now = sim.world.training_bench().map(|b| b.id);
            let at = now.and_then(|b| ids.iter().position(|i| *i == b));
            let next = at.map_or(0, |i| (i + 1) % ids.len().max(1));
            if let Some(&bench) = ids.get(next) {
                sim.command(Command::SetTrainingBench(Some(bench)));
            }
        }
        UiAction::PlaceStore => {
            let kind = sim.world.defs.building_kind_id("outpost_storage");
            sim.placing = if sim.placing.is_some() { None } else { kind };
            sim.message = if sim.placing.is_some() {
                "Click a free tile at the outpost to place it (C shows the whole map)".into()
            } else {
                String::new()
            };
        }
        UiAction::ToggleOutpost => {
            if let Some(site) = sim.world.outpost_sites().first().copied() {
                let on = !sim.world.outpost_report(site).staffed;
                sim.command(Command::SetOutpostStaffed { site, on });
            }
        }
        UiAction::LocalStock(d) => {
            if let Some(site) = sim.world.outpost_sites().first().copied() {
                let r = sim.world.outpost_report(site);
                if let (Some(building), Some(resource)) = (r.store, r.resource) {
                    let amount = (r.keep as i32 + d).max(0) as u32;
                    sim.command(Command::SetLocalStock {
                        building,
                        resource,
                        amount,
                    });
                }
            }
        }
        UiAction::TrainHere => {
            let site = sim.world.outpost_sites().first().copied();
            match site.and_then(|s| sim.world.outpost_report(s).workshop) {
                Some(b) => sim.command(Command::SetTrainingBench(Some(b))),
                None => sim.message = "There's no workshop at the outpost yet".into(),
            }
        }
        UiAction::ToggleAutoGather => {
            let on = !sim.world.player.auto_gather;
            sim.command(Command::SetAutoGather(on));
        }
        UiAction::CycleRecipe => {
            let usable: Vec<_> = sim
                .world
                .recipe_options()
                .into_iter()
                .filter(|o| o.unlocked)
                .map(|o| o.id)
                .collect();
            let at = sim
                .world
                .player
                .recipe
                .and_then(|r| usable.iter().position(|u| *u == r));
            let next = match at {
                None => usable.first().copied(),
                Some(i) => usable.get(i + 1).copied(),
            };
            sim.command(Command::SetPlayerRecipe(next));
        }
        UiAction::RecipeAuto => sim.command(Command::SetPlayerRecipe(None)),
        UiAction::ChooseRecipe(i) => {
            if let Some(o) = sim.world.recipe_options().get(i) {
                let id = o.id;
                sim.command(Command::SetPlayerRecipe(Some(id)));
            }
        }
        UiAction::SetMode(mode) => {
            sim.command(Command::SetTrainingMode(mode));
            sim.message = format!("{} training", mode.label());
        }
        UiAction::ToggleDetails => details.0 = !details.0,
        UiAction::ToggleScenarios => scenario_buttons.0 = !scenario_buttons.0,
        UiAction::WriteReport => sim.write_report(),
        UiAction::ExtraTarget(i, d) => {
            if let Some(o) = sim.world.settlement.extra_orders.get(i) {
                let output = o.output;
                let target = (o.target as i32 + d).max(0) as u32;
                sim.command(Command::SetOrderTarget { output, target });
            }
        }
        UiAction::ExtraPriority(i) => {
            if let Some(o) = sim.world.settlement.extra_orders.get(i) {
                let output = o.output;
                let priority = if o.priority > 0 { 0 } else { 1 };
                sim.command(Command::SetOrderPriority { output, priority });
            }
        }
        UiAction::PlaceWorkshop => {
            let kind = sim.world.defs.building_kind_id("workshop");
            sim.placing = if sim.placing.is_some() { None } else { kind };
            sim.message = if sim.placing.is_some() {
                "Click a free tile on the map to place the Workshop".into()
            } else {
                String::new()
            };
        }
        UiAction::BuildRack => {
            if let Some(kind) = sim.world.defs.building_kind_id("log_rack") {
                sim.command(Command::RequestBuilding(kind));
            }
        }
        UiAction::ToggleKeepPlace => {
            let on = !sim.world.settlement.training.keep_bench_place;
            sim.command(Command::SetKeepBenchPlace(on));
        }
        UiAction::SpiritPrev | UiAction::SpiritNext => {
            let count = sim.world.spirits.len().max(1);
            let step = if action == UiAction::SpiritNext {
                1
            } else {
                count - 1
            };
            sim.selected_spirit = (sim.selected_spirit + step) % count;
        }
        UiAction::CycleFocus => {
            change_policy(&mut sim, |_, p, _| p.focus = p.focus.next());
        }
        UiAction::CycleSite => change_policy(&mut sim, |w, p, _| {
            let sites = w.defs.sites.len() as u16;
            p.site = match p.site {
                None if sites > 0 => Some(SiteId(0)),
                Some(s) if s.0 + 1 < sites => Some(SiteId(s.0 + 1)),
                _ => None,
            };
        }),
        UiAction::CycleStation => change_policy(&mut sim, |w, p, _| {
            let ids: Vec<usize> = w.stations().map(|b| b.id).collect();
            let at = p.station.and_then(|b| ids.iter().position(|i| *i == b));
            p.station = match at {
                None => ids.first().copied(),
                Some(i) => ids.get(i + 1).copied(),
            };
        }),
        UiAction::ToggleAdvanced => {
            change_policy(&mut sim, |_, p, _| p.prefer_advanced = !p.prefer_advanced)
        }
        UiAction::OpenMenu => {
            sim.menu_error = None;
            sim.open_menu();
        }
        UiAction::MenuContinue => sim.continue_game(),
        UiAction::MenuBack => sim.menu = false,
        UiAction::MenuNew(profile) => {
            if sim.store.any_files() && sim.confirm_new != Some(profile) {
                sim.confirm_new = Some(profile);
            } else {
                sim.new_journey(profile);
            }
        }
        UiAction::SoulOption(i) => {
            if let Some(o) = sim.world.soul_options(skill).get(i) {
                sim.soul_amount = o.preview.transfer;
            }
        }
        UiAction::TrainingArea(i) => {
            let site = SiteId(i as u16);
            let chosen = sim.world.settlement.training.area == Some(site);
            sim.command(Command::SetTrainingArea((!chosen).then_some(site)));
        }
        UiAction::BuildCache => {
            if let Some(kind) = sim.world.training_cache_kind() {
                sim.command(Command::RequestBuilding(kind));
            }
        }
        UiAction::ToggleCollect => {
            let on = !sim.world.settlement.training.collect;
            sim.command(Command::SetTrainingCollect(on));
        }
        UiAction::ToggleSupply => {
            let on = !sim.world.settlement.training.supply;
            sim.command(Command::SetTrainingSupply(on));
        }
        UiAction::ToggleTrainingFirst => {
            let on = !sim.world.settlement.training.training_first;
            sim.command(Command::SetTrainingFirst(on));
        }
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
            // The sacrifice can't be undone, so it takes two presses.
            let xp = sim.soul_amount;
            let valid = sim.world.soul_preview(skill, xp).error.is_none();
            if sim.soul_armed || !valid {
                sim.soul_armed = false;
                sim.separate_soul(skill, xp);
            } else {
                sim.soul_armed = true;
            }
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
        UiAction::SoulSkill(s) => {
            sim.soul_skill = s;
            sim.soul_amount = min;
        }
        UiAction::PlankAdjust(d) => {
            let target = (sim.world.settlement.order.target as i32 + d).max(0) as u32;
            sim.command(Command::SetProductionTarget(target));
        }
        UiAction::ReserveAdjust(d) => {
            let reserve = (sim.world.settlement.order.input_reserve as i32 + d).max(0) as u32;
            sim.command(Command::SetInputReserve(reserve));
        }
        UiAction::ToggleProduction => {
            let enabled = !sim.world.settlement.order.enabled;
            sim.command(Command::SetProductionEnabled(enabled));
        }
        UiAction::ToggleWorker(n) => {
            let worker = sim.world.crafters().nth(n).map(|s| (s.id, s.assigned));
            if let Some((spirit, assigned)) = worker {
                sim.command(Command::SetSpiritAssigned {
                    spirit,
                    assigned: !assigned,
                });
            }
        }
        UiAction::OrderBuilding => {
            if let Some(kind) = orderable(&sim.world) {
                sim.command(Command::RequestBuilding(kind));
            }
        }
    }
}

/// Edits the selected spirit's instructions.
fn change_policy(
    sim: &mut Sim,
    change: impl FnOnce(&World, &mut spirit_sim::entities::SpiritPolicy, usize),
) {
    let spirit = sim.selected_spirit;
    let Some(mut policy) = sim.world.spirits.get(spirit).map(|s| s.policy) else {
        return;
    };
    change(&sim.world, &mut policy, spirit);
    sim.command(Command::SetSpiritPolicy { spirit, policy });
}

/// Skills the player can turn into a spirit.
fn soul_skills(w: &World) -> Vec<SkillId> {
    (0..w.defs.skills.len())
        .map(|i| SkillId(i as u16))
        .filter(|s| w.defs.soul_role_for(*s).is_some())
        .collect()
}

/// The improvement the player can order from the panel.
fn orderable(w: &World) -> Option<BuildingKindId> {
    let i = w
        .defs
        .buildings
        .iter()
        .position(|b| b.manual && b.capacity_bonus > 0)?;
    Some(BuildingKindId(i as u16))
}

fn update_buttons(
    sim: Res<Sim>,
    tab: Res<Tab>,
    all_events: Res<AllEvents>,
    details: Res<Details>,
    scenario_buttons: Res<ScenarioButtons>,
    mut buttons: Query<(&UiAction, &mut BackgroundColor, &mut Node), With<Button>>,
    mut labels: Query<(&UiAction, &mut Text)>,
) {
    let w = &sim.world;
    let options = w.soul_options(sim.soul_skill);
    let recipes = w.recipe_options();
    let has_save = sim.store.any_files();
    let selected = w.spirits.get(sim.selected_spirit);
    let selected_role = selected.map(|s| w.defs.role(s.role));
    let selected_gathers = selected_role.is_some_and(|r| r.has_job(JobKind::Gather));
    let selected_hauls = selected_role.is_some_and(|r| r.has_job(JobKind::Haul));
    let outpost = w.outpost_sites().first().map(|s| w.outpost_report(*s));
    let plannable = w.plannable();
    let plans = w.plan_views();
    let selected_crafts = selected_role.is_some_and(|r| r.has_job(JobKind::Craft));
    let cache_built = w
        .training_cache_kind()
        .is_some_and(|k| w.buildings.iter().any(|b| b.kind == k));
    for (action, mut bg, mut node) in &mut buttons {
        let show = match action {
            UiAction::ToggleWorker(n) => w.crafters().nth(*n).is_some(),
            UiAction::SoulOption(i) => *i < options.len(),
            // A playthrough isn't a sandbox: nothing here throws it away.
            UiAction::Load(_) => !sim.is_journey() && scenario_buttons.0,
            UiAction::Restart | UiAction::ToggleScenarios => !sim.is_journey(),
            UiAction::StopActivity => {
                w.player.session.is_some() || !w.player.actor.queue.is_empty()
            }
            UiAction::CycleRecipe => bench_session(w) || w.player.recipe.is_some(),
            UiAction::ToggleAutoGather => !bench_session(w),
            UiAction::MenuContinue => has_save,
            UiAction::MenuBack => sim.started,
            UiAction::BuildCache => !cache_built,
            UiAction::CycleSite => selected_gathers || selected_hauls,
            UiAction::ToggleAdvanced => selected_gathers,
            UiAction::CycleBench => w.stations().count() > 1,
            UiAction::Plan(id) => w
                .defs
                .building_kind_id(id)
                .is_some_and(|k| plannable.contains(&k)),
            UiAction::PlanUp(i) => *i > 0 && *i < w.settlement.queue.len(),
            UiAction::PlanCancel(i) => *i < w.settlement.queue.len(),
            UiAction::PlanCancelActive => plans.iter().any(|v| v.project.is_some() && v.can_cancel),
            UiAction::PlaceStore => outpost.as_ref().is_some_and(|o| o.store.is_none()),
            UiAction::ToggleOutpost => outpost.is_some(),
            UiAction::TrainHere => outpost.as_ref().is_some_and(|o| o.workshop.is_some()),
            UiAction::LocalStock(_) => outpost.as_ref().is_some_and(|o| o.store.is_some()),
            UiAction::CycleStation => selected_crafts,
            UiAction::CycleFocus => !w.spirits.is_empty(),
            _ => true,
        };
        let display = if show { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
        let active = match action {
            UiAction::Speed(s) => *s == sim.speed && !sim.paused,
            UiAction::TogglePause => sim.paused,
            UiAction::ToggleConstruction => sim.world.settlement.allow_autonomous_construction,
            UiAction::ShowTab(t) => *t == *tab,
            UiAction::Load(s) => *s == sim.scenario,
            UiAction::ToggleSite(i) => sim.world.settlement.site_open[*i],
            UiAction::ToggleAllEvents => all_events.0,
            UiAction::SoulSkill(s) => *s == sim.soul_skill,
            UiAction::ToggleProduction => w.settlement.order.enabled,
            UiAction::ToggleWorker(n) => w.crafters().nth(*n).is_some_and(|s| s.assigned),
            UiAction::OrderBuilding => w.active_project().is_some(),
            UiAction::SoulOption(i) => options
                .get(*i)
                .is_some_and(|o| o.preview.transfer == sim.soul_amount),
            UiAction::SoulSeparate => sim.soul_armed,
            UiAction::TrainingArea(i) => w.settlement.training.area == Some(SiteId(*i as u16)),
            UiAction::ToggleCollect => w.settlement.training.collect,
            UiAction::ToggleSupply => w.settlement.training.supply,
            UiAction::ToggleTrainingFirst => w.settlement.training.training_first,
            UiAction::ToggleKeepPlace => w.settlement.training.keep_bench_place,
            UiAction::PlaceWorkshop => sim.placing.is_some(),
            UiAction::ExtraPriority(i) => w
                .settlement
                .extra_orders
                .get(*i)
                .is_some_and(|o| o.priority > 0),
            UiAction::ToggleAdvanced => selected.is_some_and(|s| s.policy.prefer_advanced),
            UiAction::ToggleAutoGather => w.player.auto_gather,
            UiAction::RecipeAuto => w.player.recipe.is_none(),
            UiAction::ChooseRecipe(i) => recipes.get(*i).is_some_and(|o| o.selected),
            UiAction::SetMode(m) => w.training_mode() == *m,
            UiAction::ToggleOutpost => outpost.as_ref().is_some_and(|o| o.staffed),
            UiAction::TrainHere => {
                let bench = w.training_bench().map(|b| b.id);
                outpost.as_ref().is_some_and(|o| o.workshop == bench)
            }
            UiAction::PlaceStore => sim.placing.is_some(),
            UiAction::ToggleDetails => details.0,
            UiAction::ToggleScenarios => scenario_buttons.0,
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
            UiAction::ToggleProduction => {
                let on = w.settlement.order.enabled;
                format!("Production: {}", if on { "ON" } else { "OFF" })
            }
            UiAction::ToggleWorker(n) => match w.crafters().nth(*n) {
                Some(s) if s.assigned => format!("{}: on planks", s.name),
                Some(s) => format!("{}: unassigned", s.name),
                None => continue,
            },
            UiAction::OrderBuilding => match orderable(w) {
                Some(kind) => format!("Order {}", w.defs.building(kind).name),
                None => continue,
            },
            UiAction::SoulOption(i) => match options.get(*i) {
                Some(o) => format!("{}: {}", o.label, o.preview.transfer),
                None => continue,
            },
            UiAction::SoulSeparate => {
                if sim.soul_armed {
                    format!("Confirm: give up {} XP forever", sim.soul_amount)
                } else {
                    "Separate Soul".into()
                }
            }
            UiAction::MenuNew(profile) => {
                if sim.confirm_new == Some(*profile) {
                    "Yes, start over".into()
                } else if *profile == "normal" {
                    "New Game".into()
                } else {
                    "Fast Game".into()
                }
            }
            UiAction::TrainingArea(i) => {
                let name = &w.defs.sites[*i].name;
                if w.settlement.training.area == Some(SiteId(*i as u16)) {
                    format!("Training at {name}")
                } else {
                    format!("Train at {name}")
                }
            }
            UiAction::BuildCache => match w.training_cache_kind() {
                Some(kind) => {
                    let def = w.defs.building(kind);
                    let cost = def.cost.iter().copied().collect();
                    format!("Build {}: {}", def.name, w.defs.describe_amounts(&cost))
                }
                None => continue,
            },
            UiAction::ExtraPriority(i) => match w.settlement.extra_orders.get(*i) {
                Some(o) => {
                    let name = ["Beams", "Panels"][*i];
                    let when = if o.priority > 0 {
                        "first"
                    } else {
                        "normal priority"
                    };
                    format!("{name}: {when}")
                }
                None => continue,
            },
            UiAction::PlaceWorkshop => {
                if sim.placing.is_some() {
                    "Cancel placing".into()
                } else {
                    match w.defs.building_kind_id("workshop") {
                        Some(kind) => {
                            let cost = w.defs.building(kind).cost.iter().copied().collect();
                            format!("Place Workshop: {}", w.defs.describe_amounts(&cost))
                        }
                        None => continue,
                    }
                }
            }
            UiAction::BuildRack => match w.defs.building_kind_id("log_rack") {
                Some(kind) => {
                    let def = w.defs.building(kind);
                    let cost = def.cost.iter().copied().collect();
                    format!("Build {}: {}", def.name, w.defs.describe_amounts(&cost))
                }
                None => continue,
            },
            UiAction::ToggleKeepPlace => {
                let on = w.settlement.training.keep_bench_place;
                format!("Keep a bench place: {}", if on { "ON" } else { "OFF" })
            }
            UiAction::CycleFocus => match selected {
                Some(s) => format!("Focus: {}", s.policy.focus.label()),
                None => continue,
            },
            UiAction::CycleSite => {
                let verb = if selected_hauls {
                    "Carry for"
                } else {
                    "Gather at"
                };
                match selected.and_then(|s| s.policy.site) {
                    Some(site) => format!("{verb}: {}", w.defs.site(site).name),
                    None => format!("{verb}: anywhere"),
                }
            }
            UiAction::Plan(id) => match w.defs.building_kind_id(id) {
                Some(kind) => {
                    let def = w.defs.building(kind);
                    let cost = def.cost.iter().copied().collect();
                    format!("Plan {}: {}", def.name, w.defs.describe_amounts(&cost))
                }
                None => continue,
            },
            UiAction::PlanUp(i) => match w.settlement.queue.get(*i) {
                Some(q) => format!("Move up: {}", w.defs.building(q.kind).name),
                None => continue,
            },
            UiAction::PlanCancel(i) => match w.settlement.queue.get(*i) {
                Some(q) => format!("Cancel: {}", w.defs.building(q.kind).name),
                None => continue,
            },
            UiAction::PlanCancelActive => "Call off the current project".into(),
            UiAction::CycleBench => match w.training_bench() {
                Some(b) => {
                    let place = w.defs.site_at(b.pos.x, b.pos.y);
                    let at = place.map_or("home".to_string(), |s| w.defs.site(s).name.clone());
                    format!("Your bench: {at}")
                }
                None => continue,
            },
            UiAction::PlaceStore => {
                if sim.placing.is_some() {
                    "Cancel placing".into()
                } else {
                    match w.defs.building_kind_id("outpost_storage") {
                        Some(kind) => {
                            let cost = w.defs.building(kind).cost.iter().copied().collect();
                            format!("Place Outpost Storage: {}", w.defs.describe_amounts(&cost))
                        }
                        None => continue,
                    }
                }
            }
            UiAction::ToggleOutpost => {
                if outpost.as_ref().is_some_and(|o| o.staffed) {
                    "Release outpost staff".into()
                } else {
                    "Staff outpost".into()
                }
            }
            UiAction::CycleStation => match selected.and_then(|s| s.policy.station) {
                Some(b) => format!("Work at: {} #{b}", w.buildings[b].name),
                None => "Work at: any bench".into(),
            },
            UiAction::ToggleAdvanced => {
                let on = selected.is_some_and(|s| s.policy.prefer_advanced);
                format!("Top tier first: {}", if on { "ON" } else { "OFF" })
            }
            UiAction::ToggleCollect => {
                let on = w.settlement.training.collect;
                format!("Collect my logs: {}", if on { "ON" } else { "OFF" })
            }
            UiAction::ToggleSupply => {
                let on = w.settlement.training.supply;
                format!("Stock the bench: {}", if on { "ON" } else { "OFF" })
            }
            UiAction::ToggleTrainingFirst => {
                if w.settlement.training.training_first {
                    "Haulers: my training first".into()
                } else {
                    "Haulers: production first".into()
                }
            }
            UiAction::ToggleAutoGather => {
                let on = w.player.auto_gather;
                format!("Next target: {}", if on { "AUTO" } else { "I'LL CLICK" })
            }
            UiAction::CycleRecipe => match w.player.recipe {
                Some(r) => format!("Making: {}", w.defs.recipe(r).name),
                None => "Making: best available".into(),
            },
            UiAction::ChooseRecipe(i) => match recipes.get(*i) {
                Some(o) if o.unlocked => o.name.clone(),
                Some(o) => format!("{} (Lv {})", o.name, o.level_required),
                None => continue,
            },
            UiAction::ToggleDetails => {
                format!("Details: {}", if details.0 { "ON" } else { "OFF" })
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
            Card::Production => production_lines(&sim.world),
            Card::ProductionLimit => production_limit_lines(&sim.world),
            Card::ChainTime => chain_time_lines(&sim.world),
            Card::Goal => goal_lines(&sim.world),
            Card::Ambitions => ambition_lines(&sim.world),
            Card::Menu => menu_lines(&sim),
            Card::Skills => skills_lines(&sim.world),
            Card::Workforce => workforce_lines(&sim.world),
            Card::Improvements => improvement_lines(&sim.world),
            Card::Recent => recent_lines(&sim.world),
            Card::TrainingStatus => training_lines(&sim.world),
            Card::PlayerTime => player_time_lines(&sim.world),
            Card::Orders => order_lines(&sim.world),
            Card::Stations => station_lines(&sim.world),
            Card::SpiritOrders => spirit_order_lines(&sim),
            Card::Activity => activity_lines(&sim),
            Card::Glance => glance_lines(&sim.world),
            Card::Recipes => recipe_lines(&sim.world),
            Card::TrainingPlan => training_plan_lines(&sim.world),
            Card::PlayReport => report_lines(&sim.world),
            Card::Outpost => outpost_lines(&sim.world),
            Card::ProjectNeeds => need_lines(&sim.world),
            Card::Projects => plan_lines(&sim.world),
        };
        if rendered.changed(&lines) {
            cards::rebuild(&mut commands, entity, &lines);
        }
    }
}

fn header_lines(sim: &Sim) -> Vec<Line> {
    let w = &sim.world;
    let secs = w.tick / w.defs.ticks_per_second as u64;
    let journey = format!("{} game", w.defs.profile.title);
    let scenario = match sim.scenario {
        ScenarioKind::NewGame if sim.is_journey() => journey.as_str(),
        ScenarioKind::NewGame => "Blank",
        ScenarioKind::Demo => "Demo",
        ScenarioKind::Regrind => "Regrind",
        ScenarioKind::GroveSustainable => "Grove: sustainable",
        ScenarioKind::GroveOversubscribed => "Grove: oversubscribed",
        ScenarioKind::WorkshopManual => "Workshop: manual",
        ScenarioKind::WorkshopAutomated => "Workshop: automated",
        ScenarioKind::WorkshopConstruction => "Workshop: construction",
        ScenarioKind::AdvancedSpecialization => "Specialization",
        ScenarioKind::ProductionBottleneck => "Bottleneck",
        ScenarioKind::ExpandedProduction => "Expansion",
        ScenarioKind::SupportedRetraining => "Retraining",
        ScenarioKind::SkillingSession => "Skilling session",
        ScenarioKind::WorkshopRecipes => "Workshop recipes",
        ScenarioKind::FullStorage => "Full storage",
        ScenarioKind::CentralProduction => "Central production",
        ScenarioKind::RemoteProduction => "Remote production",
        ScenarioKind::OutpostEstablishment => "Outpost",
        ScenarioKind::ExpandedCentral => "Expanded, central",
        ScenarioKind::ExpandedRemote => "Expanded, remote",
        ScenarioKind::ForestAccess => "Forest access",
        ScenarioKind::QuarryStart => "Quarry",
        ScenarioKind::WalledIn => "Walled in",
        ScenarioKind::PanelWorkshop => "Panel workshop",
        ScenarioKind::PlankSurplus => "Plank surplus",
        ScenarioKind::ProjectQueue => "Project plan",
        ScenarioKind::SharedHauler => "Shared hauler",
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
            "WASD/click move  E use  X stop  C map  H hide  V details",
        )],
    ]
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

/// What a skill unlocks, as chips: green when the player has it, grey with
/// its level otherwise.
fn unlock_chips(w: &World, skill: SkillId, level: u32) -> Vec<Item> {
    let tiers = w.defs.tree_tiers(skill).into_iter().map(|k| {
        let d = w.defs.tree(k);
        (d.name.clone(), d.level_required)
    });
    let recipes = w.defs.recipe_tiers(skill).into_iter().map(|r| {
        let d = w.defs.recipe(r);
        (d.name.clone(), d.level_required)
    });
    tiers
        .chain(recipes)
        .map(|(name, required)| {
            if level >= required {
                chip(name, GOOD)
            } else {
                chip(format!("{name} - Lv {required}"), MUTED)
            }
        })
        .collect()
}

/// Name, level, XP bar and what's next in one skill.
fn skill_lines(w: &World, skill: SkillId) -> Vec<Line> {
    let xp = w.player.xp(skill);
    let level = w.player.level(skill, &w.defs.xp_table);
    let (progress, next) = level_progress(w, xp);
    let h = w.player.history(skill);
    let mut lines = vec![vec![
        text(w.defs.skill(skill).name.clone()),
        big(format!("Lv {level}"), GOLD),
        bar(progress, XP, 130.0),
        Item::Spacer,
        muted(match next {
            Some(n) => format!("{xp} / {n}"),
            None => format!("{xp} (max)"),
        }),
    ]];
    let mut note = Vec::new();
    if let Some((name, required)) = next_unlock(w, skill, level) {
        note.push(format!("Next: {name} - Lv {required}"));
    }
    if h.spirits_created > 0 {
        note.push(format!(
            "Best ever Lv {}, {} XP given",
            h.highest_level, h.xp_sacrificed
        ));
    }
    if !note.is_empty() {
        lines.push(vec![muted(note.join("  |  "))]);
    }
    lines
}

fn player_lines(sim: &Sim) -> Vec<Line> {
    let w = &sim.world;
    let mut lines = Vec::new();
    for skill in soul_skills(w) {
        lines.extend(skill_lines(w, skill));
    }
    lines
}

/// The player is working at a bench.
fn bench_session(w: &World) -> bool {
    use spirit_sim::session::SessionKind;
    let session = w.player.session.as_ref();
    session.is_some_and(|s| matches!(s.kind, SessionKind::Crafting { .. }))
}

fn clock(seconds: u64) -> String {
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

fn activity_lines(sim: &Sim) -> Vec<Line> {
    let w = &sim.world;
    let r = w.activity_report();
    let (word, color) = match r.state {
        ActivityState::Idle => ("idle", MUTED),
        ActivityState::Working => ("working", GOOD),
        ActivityState::Waiting => ("waiting", WARN),
        ActivityState::Blocked => ("stopped", BAD),
    };
    let repeats_title = sim.message == r.title;
    let verb = if bench_session(w) { "made" } else { "cut" };
    let mut lines = vec![vec![big(r.title, TEXT), Item::Spacer, chip(word, color)]];
    if let Some(p) = r.progress {
        lines.push(vec![text(format!(
            "{} {verb}  |  {} XP  |  {}  |  {} XP/min",
            p.units,
            p.xp,
            clock(p.seconds),
            p.xp_per_min
        ))]);
    }
    if let Some(d) = r.detail {
        let c = if r.state == ActivityState::Blocked {
            WARN
        } else {
            MUTED
        };
        lines.push(vec![colored(d, c)]);
    }
    let inv = &w.player.actor.inventory;
    lines.push(vec![
        muted("Bag"),
        bar(inv.total() as f32 / inv.capacity.max(1) as f32, GOLD, 70.0),
        muted(if inv.total() == 0 {
            "empty".to_string()
        } else {
            w.defs.describe_amounts(inv.items())
        }),
    ]);
    if !sim.message.is_empty() && !repeats_title {
        lines.push(vec![colored(sim.message.clone(), INFO)]);
    }
    lines
}

fn glance_lines(w: &World) -> Vec<Line> {
    let g = w.settlement_glance();
    let main = &w.main().storage;
    let mut lines = Vec::new();
    if w.spirits.is_empty() {
        lines.push(vec![muted("No spirits yet.")]);
    } else {
        lines.push(vec![
            Item::Dot(Status::Chopping.color()),
            muted(format!("{} working", g.working)),
            Item::Dot(Status::Waiting.color()),
            muted(format!("{} waiting", g.waiting)),
            Item::Dot(BAD),
            muted(format!("{} blocked", g.blocked)),
            Item::Dot(Status::Idle.color()),
            muted(format!("{} done", g.idle - g.unassigned)),
            Item::Dot(MUTED),
            muted(format!("{} free", g.unassigned)),
        ]);
    }
    if let Some(v) = w
        .plan_views()
        .first()
        .filter(|v| v.project.is_some() || v.queued.is_some())
    {
        lines.push(vec![
            text("Building"),
            colored(v.name.clone(), INFO),
            Item::Spacer,
            muted(v.stage.label()),
        ]);
    }
    lines.push(vec![
        text("Production"),
        colored(g.production, if g.production_ok { GOOD } else { WARN }),
        Item::Spacer,
        muted(format!("Storage {} / {}", main.total(), main.capacity)),
    ]);
    if let Some(a) = g.attention {
        lines.push(vec![colored(a, WARN)]);
    }
    lines
}

fn recipe_lines(w: &World) -> Vec<Line> {
    let options = w.recipe_options();
    if options.is_empty() {
        return vec![vec![muted("No workshop yet.")]];
    }
    let mut lines = Vec::new();
    for o in options {
        let color = if o.selected { GOLD } else { TEXT };
        let state = if !o.unlocked {
            chip(format!("Lv {}", o.level_required), MUTED)
        } else if let Some(m) = &o.missing {
            chip(format!("have {m}"), WARN)
        } else {
            chip("ready", GOOD)
        };
        lines.push(vec![colored(o.name, color), Item::Spacer, state]);
        lines.push(vec![muted(format!(
            "     {} -> {}, {} XP  ({} XP/min, {} made/min)",
            o.inputs, o.output, o.xp, o.xp_per_min, o.output_per_min
        ))]);
    }
    if let Some(b) = w.station() {
        let slots = w.slots_of(b);
        lines.push(vec![muted(format!(
            "{}: {} of {slots} places in use",
            b.name,
            w.slots_used(b.id)
        ))]);
    }
    lines
}

fn training_plan_lines(w: &World) -> Vec<Line> {
    let p = w.training_plan();
    let mut lines = vec![
        vec![text(p.mode.trade())],
        vec![colored(p.limit, INFO)],
        vec![muted(format!(
            "You use {} {}/min  |  gatherers bring {}/min  |  {} spare",
            p.use_per_min, p.material, p.gathered_per_min, p.spare
        ))],
    ];
    if p.mode == TrainingMode::Optimized {
        for (name, done) in p.checks {
            let mark = if done {
                chip("done", GOOD)
            } else {
                chip("missing", WARN)
            };
            lines.push(vec![muted(name), Item::Spacer, mark]);
        }
    }
    lines
}

fn outpost_lines(w: &World) -> Vec<Line> {
    let Some(site) = w.outpost_sites().first().copied() else {
        return vec![vec![muted("No site can hold an outpost.")]];
    };
    let r = w.outpost_report(site);
    let built = |ok: bool, name: &str| {
        if ok {
            chip(name.to_string(), GOOD)
        } else {
            chip(format!("no {name}"), MUTED)
        }
    };
    let mut lines = vec![
        vec![
            big(r.name.clone(), TEXT),
            Item::Spacer,
            built(r.store.is_some(), "store"),
            built(r.workshop.is_some(), "bench"),
        ],
        vec![colored(r.status.clone(), INFO)],
    ];
    if r.store.is_some() {
        let name = r
            .resource
            .map_or(String::new(), |x| w.defs.resource(x).name.clone());
        lines.push(vec![
            text(name),
            bar(r.stock as f32 / r.keep.max(1) as f32, GOLD, 110.0),
            text(format!("{} here, keeping {}", r.stock, r.keep)),
        ]);
    }
    let names = |list: &[String]| {
        if list.is_empty() {
            "nobody".to_string()
        } else {
            list.join(", ")
        }
    };
    lines.push(vec![muted(format!("Gathering: {}", names(&r.gatherers)))]);
    lines.push(vec![muted(format!("At the bench: {}", names(&r.workers)))]);
    lines.push(vec![muted(format!(
        "Carrying only for here: {}",
        names(&r.haulers)
    ))]);
    if r.workshop.is_some() {
        lines.push(vec![muted(format!("Made here: {}", r.made))]);
    }
    lines
}

fn plan_lines(w: &World) -> Vec<Line> {
    use spirit_sim::projects::PlanStage;
    let views = w.plan_views();
    if views.is_empty() {
        return vec![vec![muted(
            "Nothing planned. Add something below and carry on with your own training.",
        )]];
    }
    let mut lines = Vec::new();
    for v in views {
        let color = match v.stage {
            PlanStage::Blocked => BAD,
            PlanStage::Queued => MUTED,
            PlanStage::Finished => GOOD,
            _ => INFO,
        };
        lines.push(vec![
            big(v.name, TEXT),
            Item::Spacer,
            chip(v.stage.label(), color),
        ]);
        if v.stage == PlanStage::Finished {
            lines.push(vec![muted(v.benefit)]);
            continue;
        }
        lines.push(vec![muted(format!("Needs {}", v.cost))]);
        if !v.benefit.is_empty() {
            lines.push(vec![muted(v.benefit)]);
        }
        for n in v.needs.iter().filter(|n| n.have < n.need) {
            lines.push(vec![colored(
                format!("Short of {} {}: {}", n.need - n.have, n.name, n.status),
                WARN,
            )]);
        }
        if !v.workers.is_empty() {
            lines.push(vec![muted(format!("Builder: {}", v.workers.join(", ")))]);
        }
        if v.stage == PlanStage::Blocked
            && let Some(r) = v.reason
        {
            lines.push(vec![colored(r, BAD)]);
        }
    }
    lines
}

fn need_lines(w: &World) -> Vec<Line> {
    let needs = w.project_needs();
    if needs.is_empty() {
        let text = match w.active_project() {
            Some(_) => "Everything is reserved. The builder is on it.",
            None => "No project under way.",
        };
        return vec![vec![muted(text)]];
    }
    let mut lines = Vec::new();
    for n in needs {
        let done = n.have >= n.need;
        lines.push(vec![
            text(n.name),
            bar(
                n.have as f32 / n.need.max(1) as f32,
                if done { GOOD } else { WARN },
                100.0,
            ),
            text(format!("{} / {}", n.have, n.need)),
        ]);
        if !done {
            lines.push(vec![muted(format!("     {}", n.status))]);
        }
    }
    lines
}

fn report_lines(w: &World) -> Vec<Line> {
    w.play_report()
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| vec![muted(l.to_string())])
        .collect()
}

fn soul_lines(sim: &Sim) -> Vec<Line> {
    let w = &sim.world;
    let p = w.soul_preview(sim.soul_skill, sim.soul_amount);
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
    let tree = |k: &spirit_sim::defs::TreeKindId| w.defs.tree(*k).name.clone();
    let recipe = |r: &spirit_sim::defs::RecipeId| w.defs.recipe(*r).name.clone();
    let lost: Vec<String> = p
        .lost
        .iter()
        .map(tree)
        .chain(p.lost_recipes.iter().map(recipe))
        .collect();
    let gained: Vec<String> = p
        .spirit_can_cut
        .iter()
        .map(tree)
        .chain(p.spirit_recipes.iter().map(recipe))
        .collect();
    let you_color = if lost.is_empty() { TEXT } else { BAD };
    let compared = w.soul_options(sim.soul_skill);
    if compared.len() > 1 {
        lines.push(vec![muted("Compare (XP, your level after, spirit level):")]);
        for o in &compared {
            let picked = o.preview.transfer == p.transfer;
            let color = if picked { GOLD } else { TEXT };
            let losing = o.preview.lost.len() + o.preview.lost_recipes.len();
            let mut line = vec![
                colored(o.label.clone(), color),
                Item::Spacer,
                muted(format!(
                    "{} XP  you Lv {}  spirit Lv {}",
                    o.preview.transfer, o.preview.after_level, o.preview.spirit_level
                )),
            ];
            if losing > 0 {
                line.push(colored("lose", BAD));
            }
            lines.push(line);
        }
    }
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
    let mut spirit = vec![muted("Spirit can do")];
    if gained.is_empty() {
        spirit.push(colored("nothing", BAD));
    } else {
        spirit.extend(gained.into_iter().map(|n| chip(n, GOOD)));
    }
    lines.push(spirit);
    if lost.is_empty() {
        lines.push(vec![muted("You keep everything you can do now.")]);
    } else {
        let mut line = vec![colored("You lose", BAD)];
        line.extend(lost.into_iter().map(|n| chip(n, BAD)));
        line.push(muted("until you retrain"));
        lines.push(line);
    }
    if !p.benefit.is_empty() {
        lines.push(vec![muted("For the settlement:"), text(p.benefit.clone())]);
    }
    lines.push(vec![colored(
        format!(
            "This can't be undone. Earning it back takes {} XP of your own work.",
            p.transfer
        ),
        WARN,
    )]);
    lines
}

fn order_lines(w: &World) -> Vec<Line> {
    let mut lines = Vec::new();
    for o in w.order_reports() {
        let met = o.in_main >= o.goal;
        lines.push(vec![
            text(o.name.clone()),
            bar(
                o.in_main as f32 / o.goal.max(1) as f32,
                if met { GOOD } else { WARN },
                110.0,
            ),
            text(format!("{} / {}", o.in_main, o.goal)),
            Item::Spacer,
            muted(format!("{:.0}/min", o.per_min)),
        ]);
        let color = if o.stalled { WARN } else { MUTED };
        lines.push(vec![colored(format!("     {}", o.status), color)]);
    }
    lines
}

fn station_lines(w: &World) -> Vec<Line> {
    let mut lines = Vec::new();
    for s in w.station_reports() {
        let full = s.occupied >= s.slots;
        lines.push(vec![
            text(format!("{} #{}", s.name, s.id)),
            bar(
                s.occupied as f32 / s.slots.max(1) as f32,
                if full { WARN } else { GOOD },
                90.0,
            ),
            text(format!("{}/{} places", s.occupied, s.slots)),
            Item::Spacer,
            muted(format!("{} made", s.crafted)),
        ]);
        if !s.workers.is_empty() {
            lines.push(vec![muted(format!("     {}", s.workers.join(", ")))]);
        }
    }
    let hauling = w.production_report().hauling;
    if hauling.total() > 0 {
        lines.push(vec![muted(format!(
            "Haulers spend {}% of their working time walking.",
            hauling.pct_of_working(hauling.traveling)
        ))]);
    }
    lines
}

fn spirit_order_lines(sim: &Sim) -> Vec<Line> {
    let w = &sim.world;
    let Some(s) = w.spirits.get(sim.selected_spirit) else {
        return vec![vec![muted("No spirits yet.")]];
    };
    let (status, word) = spirit_status(s);
    let mut lines = vec![
        vec![
            big(s.name.clone(), TEXT),
            muted(format!("{} Lv {}", w.defs.role(s.role).name, s.level())),
            Item::Spacer,
            colored(word, status.color()),
        ],
        vec![
            chip(w.worker_state(s).label(), status.color()),
            muted(s.activity.text().to_string()),
        ],
    ];
    let t = s.time.recent();
    let total = t.total().max(1);
    lines.push(vec![muted(format!(
        "Last 2 minutes: walking {}%, waiting {}%",
        t.traveling * 100 / total,
        (t.waiting_input + t.waiting_recovery + t.waiting_station) * 100 / total
    ))]);
    lines
}

fn goal_lines(w: &World) -> Vec<Line> {
    let Some(g) = w.guidance() else {
        return Vec::new();
    };
    let mut lines = vec![vec![big(g.title, GOLD)], vec![text(g.hint)]];
    if let Some((have, want, label)) = g.progress {
        lines.push(vec![
            bar(have as f32 / want.max(1) as f32, XP, 180.0),
            muted(format!("{} / {want} {label}", have.min(want))),
        ]);
    }
    lines
}

/// The next few long-term goals, and how many are behind you.
fn ambition_lines(w: &World) -> Vec<Line> {
    let all = w.ambitions();
    let done = all.iter().filter(|a| a.done).count();
    let mut lines = vec![vec![muted(format!("{done} of {} done", all.len()))]];
    for a in all.into_iter().filter(|a| !a.done).take(3) {
        lines.push(vec![text(a.title)]);
        lines.push(vec![muted(a.hint)]);
        if let Some((have, want, label)) = a.progress {
            lines.push(vec![
                bar(have as f32 / want.max(1) as f32, XP, 180.0),
                muted(format!("{} / {want} {label}", have.min(want))),
            ]);
        }
    }
    lines
}

fn menu_lines(sim: &Sim) -> Vec<Line> {
    let mut lines = vec![
        vec![big("SpiritScape", HEADING)],
        vec![text(
            "Train a skill by hand. Give that skill up to a spirit that works for you forever. Train again, with your settlement behind you.",
        )],
        vec![muted(match &sim.save_summary {
            Some(s) => s.clone(),
            None if sim.store.any_files() => "A save exists but can't be read.".to_string(),
            None => "No saved game yet.".to_string(),
        })],
    ];
    if let Some(e) = &sim.menu_error {
        lines.push(vec![colored(e.clone(), BAD)]);
    }
    if sim.confirm_new.is_some() {
        lines.push(vec![colored(
            "A new game replaces your saved game the first time it saves. Press again to confirm.",
            WARN,
        )]);
    }
    lines.push(vec![muted(
        "New Game is the intended pace. Fast Game uses rapid testing rates.",
    )]);
    lines
}

/// The next thing a skill unlocks above `level`.
fn next_unlock(w: &World, skill: SkillId, level: u32) -> Option<(String, u32)> {
    let trees = w.defs.tree_tiers(skill).into_iter().map(|k| {
        let d = w.defs.tree(k);
        (d.name.clone(), d.level_required)
    });
    let recipes = w.defs.recipe_tiers(skill).into_iter().map(|r| {
        let d = w.defs.recipe(r);
        (d.name.clone(), d.level_required)
    });
    trees
        .chain(recipes)
        .filter(|(_, required)| *required > level)
        .min_by_key(|(_, required)| *required)
}

fn skills_lines(w: &World) -> Vec<Line> {
    let table = &w.defs.xp_table;
    let mut lines = Vec::new();
    for skill in soul_skills(w) {
        let level = w.player.level(skill, table);
        let h = w.player.history(skill);
        let mut line = vec![
            text(w.defs.skill(skill).name.clone()),
            big(format!("Lv {level}"), GOLD),
            muted(format!("best ever Lv {}", h.highest_level)),
        ];
        if h.xp_sacrificed > 0 {
            line.push(Item::Spacer);
            line.push(colored(format!("{} XP given", h.xp_sacrificed), XP));
        }
        lines.push(line);
        lines.push(unlock_chips(w, skill, level));
        if let Some((name, required)) = next_unlock(w, skill, level) {
            let left = table
                .xp_for_level(required)
                .saturating_sub(w.player.xp(skill));
            lines.push(vec![muted(format!(
                "     Next: {name} at Lv {required} ({left} XP to go)"
            ))]);
        }
    }
    lines.push(vec![
        text("Invested in spirits"),
        Item::Spacer,
        colored(format!("{} XP", w.player.xp_invested()), XP),
    ]);
    lines
}

fn workforce_lines(w: &World) -> Vec<Line> {
    if w.spirits.is_empty() {
        return vec![vec![muted("No spirits yet.")]];
    }
    let mut lines = Vec::new();
    for s in &w.spirits {
        let role = &w.defs.role(s.role).name;
        let mut line = vec![
            text(s.name.clone()),
            muted(format!("{role} Lv {}", s.level())),
            Item::Spacer,
        ];
        let trees = w.defs.tree_tiers(s.skill()).into_iter();
        line.extend(
            trees
                .map(|k| w.defs.tree(k))
                .filter(|d| s.level() >= d.level_required)
                .map(|d| chip(d.name.clone(), GOOD)),
        );
        let recipes = w.defs.recipe_tiers(s.skill()).into_iter();
        line.extend(
            recipes
                .map(|r| w.defs.recipe(r))
                .filter(|d| s.level() >= d.level_required)
                .map(|d| chip(d.name.clone(), GOOD)),
        );
        lines.push(line);
    }
    lines
}

fn improvement_lines(w: &World) -> Vec<Line> {
    let mut lines = vec![vec![
        text("Main Storage"),
        Item::Spacer,
        text(format!(
            "{} / {} used",
            w.main().storage.total(),
            w.main().storage.capacity
        )),
    ]];
    let main = &w.main().storage;
    for (i, def) in w.defs.buildings.iter().enumerate() {
        if !def.manual {
            continue;
        }
        let kind = BuildingKindId(i as u16);
        let count = w.buildings.iter().filter(|b| b.kind == kind).count() as u32;
        let cost: std::collections::BTreeMap<_, _> = def.cost.iter().copied().collect();
        let short: Vec<String> = def
            .cost
            .iter()
            .filter(|(r, n)| main.available(*r) < *n)
            .map(|(r, n)| format!("{}/{n} {}", main.available(*r), w.defs.resource(*r).name))
            .collect();
        let state = if def.max_count.is_some_and(|m| count >= m) {
            chip("all built", GOOD)
        } else if short.is_empty() {
            chip("materials ready", GOOD)
        } else {
            chip(format!("short: {}", short.join(", ")), WARN)
        };
        lines.push(vec![
            text(format!("{} ({count} built)", def.name)),
            Item::Spacer,
            state,
        ]);
        let effect = if def.capacity_bonus > 0 {
            format!("+{} storage", def.capacity_bonus)
        } else {
            "a drop-off point where you train".to_string()
        };
        lines.push(vec![muted(format!(
            "     {}: {effect}",
            w.defs.describe_amounts(&cost)
        ))]);
    }
    lines
}

fn recent_lines(w: &World) -> Vec<Line> {
    let tps = w.defs.ticks_per_second as u64;
    let lines: Vec<Line> = w
        .notices
        .recent(RECENT_NOTICES)
        .map(|n| {
            let secs = n.tick / tps;
            vec![
                muted(format!("{:02}:{:02}", secs / 60, secs % 60)),
                text(n.text.clone()),
            ]
        })
        .collect();
    if lines.is_empty() {
        return vec![vec![muted("Nothing yet.")]];
    }
    lines
}

fn training_lines(w: &World) -> Vec<Line> {
    let r = w.training_report();
    let area = match &r.area {
        Some(name) => chip(name.clone(), INFO),
        None => chip("none chosen", MUTED),
    };
    let mut lines = vec![vec![
        muted("Spirits carry; only you earn XP."),
        Item::Spacer,
        text("Area"),
        area,
    ]];
    match r.cache {
        Some((stock, capacity)) => lines.push(vec![
            text("Cache"),
            bar(stock as f32 / capacity.max(1) as f32, GOLD, 150.0),
            text(format!("{stock} / {capacity}")),
        ]),
        None => lines.push(vec![muted(
            "Choose an area, then build a cache to drop logs nearby.",
        )]),
    }
    lines.push(vec![muted(r.collect_status)]);
    lines.push(vec![
        text("Bench"),
        bar(r.bench.0 as f32 / r.bench.1.max(1) as f32, GOLD, 150.0),
        text(format!("{} / {} set aside for you", r.bench.0, r.bench.1)),
    ]);
    lines.push(vec![muted(r.supply_status)]);
    lines
}

fn player_time_lines(w: &World) -> Vec<Line> {
    let t = &w.player.time;
    let pct = |n: u64| n * 100 / t.total().max(1);
    vec![
        vec![
            Item::Dot(Status::Chopping.color()),
            muted(format!("chop {}%", pct(t.gathering))),
            Item::Dot(Status::Crafting.color()),
            muted(format!("saw {}%", pct(t.crafting))),
            Item::Dot(Status::Walking.color()),
            muted(format!("walk {}%", pct(t.traveling))),
            Item::Dot(Status::Idle.color()),
            muted(format!("idle {}%", pct(t.idle))),
        ],
        vec![text("You"), Item::Spacer, chain_stack(t)],
    ]
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
    for b in w.buildings.iter().filter(|b| {
        b.id != w.main_storage
            && b.is_complete()
            && w.defs.building(b.kind).accepts_gatherer_deposits
    }) {
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
    for p in w.projects.iter().filter(|p| p.is_active()) {
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
            ProjectState::Complete | ProjectState::Cancelled => (1.0, "complete".into()),
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
    let access = if let Some(name) = &site.locked_by {
        chip(format!("needs a {name}"), WARN)
    } else if site.spirits_allowed {
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

fn production_lines(w: &World) -> Vec<Line> {
    let r = w.production_report();
    let order = &w.settlement.order;
    let met = r.output_in_main >= r.goal;
    let status_color = match r.status {
        WorkshopStatus::Working => GOOD,
        WorkshopStatus::TargetMet => INFO,
        WorkshopStatus::Disabled | WorkshopStatus::NoWorker | WorkshopStatus::NoStation => MUTED,
        _ => WARN,
    };
    let mut lines = vec![
        vec![colored(r.status.label(), status_color)],
        vec![
            text("Planks"),
            bar(
                r.output_in_main as f32 / r.goal.max(1) as f32,
                if met { GOOD } else { WARN },
                150.0,
            ),
            text(format!("{} / {}", r.output_in_main, r.goal)),
            muted(format!("(target {})", order.target)),
        ],
        vec![
            text("Logs  "),
            bar(
                r.input_in_main as f32 / w.main().storage.capacity.max(1) as f32,
                INFO,
                150.0,
            ),
            text(format!("{}", r.input_in_main)),
            muted(format!("(keep {} in reserve)", order.input_reserve)),
        ],
    ];
    if let Some(b) = w.station() {
        let def = w.defs.building(b.kind);
        lines.push(vec![
            text("Bench "),
            bar(
                r.input_at_station as f32 / def.input_buffer.max(1) as f32,
                GOLD,
                150.0,
            ),
            text(format!("{} logs", r.input_at_station)),
            muted(format!("{} planks finished", r.output_at_station)),
        ]);
    }
    let mut output = vec![
        text("Output"),
        colored(format!("{:.0} planks/min", r.output_per_min), INFO),
        muted(format!("{} worker(s)", r.workers)),
    ];
    if let Some(recipe) = w.production_recipe() {
        output.push(Item::Spacer);
        output.push(chip(&w.defs.recipe(recipe).name, GOOD));
    }
    lines.push(output);
    for p in w.projects.iter().filter(|p| p.is_active()) {
        let name = &w.defs.building(p.kind).name;
        let note = p.blocked.as_deref().unwrap_or("materials reserved");
        lines.push(vec![muted(format!("{name}: {note}"))]);
    }
    if let Some(kind) = orderable(w)
        && w.active_project().is_none()
    {
        let def = w.defs.building(kind);
        let cost = def.cost.iter().copied().collect();
        lines.push(vec![muted(format!(
            "{}: {}, +{} storage",
            def.name,
            w.defs.describe_amounts(&cost),
            def.capacity_bonus
        ))]);
    }
    lines
}

fn production_limit_lines(w: &World) -> Vec<Line> {
    let r = w.production_report();
    let color = match r.bottleneck {
        ProductionBottleneck::None => GOOD,
        ProductionBottleneck::ProcessingSpeed => INFO,
        ProductionBottleneck::Disabled | ProductionBottleneck::NoWorker => MUTED,
        _ => WARN,
    };
    vec![
        vec![big(r.bottleneck.label(), color)],
        vec![text(r.summary)],
    ]
}

fn chain_stack(t: &TimeUse) -> Item {
    let total = t.total().max(1) as f32;
    let f = |n: u64| ((n as f32 / total) * 50.0).round() / 50.0;
    Item::Stack {
        parts: vec![
            (f(t.gathering), Status::Chopping.color()),
            (f(t.crafting), Status::Crafting.color()),
            (f(t.traveling), Status::Walking.color()),
            (f(t.handling + t.building), Status::Carrying.color()),
            (
                f(t.waiting_recovery + t.waiting_input),
                Status::Waiting.color(),
            ),
            (f(t.blocked_storage + t.blocked_other), BAD),
            (f(t.idle), Status::Idle.color()),
        ],
        width: 260.0,
    }
}

fn chain_time_lines(w: &World) -> Vec<Line> {
    let r = w.production_report();
    vec![
        vec![
            Item::Dot(Status::Chopping.color()),
            muted("chop"),
            Item::Dot(Status::Crafting.color()),
            muted("saw"),
            Item::Dot(Status::Walking.color()),
            muted("walk"),
            Item::Dot(Status::Carrying.color()),
            muted("load"),
            Item::Dot(Status::Waiting.color()),
            muted("wait"),
            Item::Dot(Status::Idle.color()),
            muted("idle"),
        ],
        vec![text("Gatherers"), Item::Spacer, chain_stack(&r.gathering)],
        vec![text("Haulers"), Item::Spacer, chain_stack(&r.hauling)],
        vec![
            text("Woodworkers"),
            Item::Spacer,
            chain_stack(&r.processing),
        ],
    ]
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
