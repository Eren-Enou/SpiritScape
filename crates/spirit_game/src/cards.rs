//! Small retained-mode helper for the side panel: a card body is described
//! each frame as lines of text, bars and chips, and its UI nodes are rebuilt
//! only when that description changes. Buttons live outside card bodies, so
//! a rebuild never swallows a click.

use bevy::picking::Pickable;
use bevy::prelude::*;

pub const MUTED: Color = Color::srgb(0.62, 0.66, 0.72);
pub const TEXT: Color = Color::srgb(0.90, 0.92, 0.94);
pub const GOOD: Color = Color::srgb(0.45, 0.85, 0.45);
pub const WARN: Color = Color::srgb(1.0, 0.65, 0.25);
pub const BAD: Color = Color::srgb(1.0, 0.4, 0.35);
pub const INFO: Color = Color::srgb(0.55, 0.78, 1.0);
pub const GOLD: Color = Color::srgb(0.98, 0.80, 0.35);
const TRACK: Color = Color::srgb(0.20, 0.22, 0.26);

#[derive(Clone, PartialEq, Debug)]
pub enum Item {
    Text {
        text: String,
        size: f32,
        color: Color,
    },
    /// A horizontal bar `width` px wide, filled to `frac` (0..=1).
    Bar { frac: f32, color: Color, width: f32 },
    /// A bar split into coloured segments (fractions should sum to <= 1).
    Stack {
        parts: Vec<(f32, Color)>,
        width: f32,
    },
    /// A small coloured tag.
    Chip { text: String, color: Color },
    /// A coloured square status dot.
    Dot(Color),
    /// Pushes the following items to the right edge.
    Spacer,
}

pub type Line = Vec<Item>;

pub fn text(s: impl Into<String>) -> Item {
    Item::Text {
        text: s.into(),
        size: 13.0,
        color: TEXT,
    }
}

pub fn colored(s: impl Into<String>, color: Color) -> Item {
    Item::Text {
        text: s.into(),
        size: 13.0,
        color,
    }
}

pub fn muted(s: impl Into<String>) -> Item {
    Item::Text {
        text: s.into(),
        size: 12.0,
        color: MUTED,
    }
}

pub fn big(s: impl Into<String>, color: Color) -> Item {
    Item::Text {
        text: s.into(),
        size: 17.0,
        color,
    }
}

pub fn bar(frac: f32, color: Color, width: f32) -> Item {
    // Rounded so tiny changes don't trigger a rebuild every frame.
    Item::Bar {
        frac: (frac.clamp(0.0, 1.0) * 100.0).round() / 100.0,
        color,
        width,
    }
}

pub fn chip(s: impl Into<String>, color: Color) -> Item {
    Item::Chip {
        text: s.into(),
        color,
    }
}

/// What a card body currently shows.
#[derive(Component, Default)]
pub struct Rendered(Option<Vec<Line>>);

impl Rendered {
    /// Returns true (and remembers `lines`) when they differ from what is shown.
    pub fn changed(&mut self, lines: &[Line]) -> bool {
        if self.0.as_deref() == Some(lines) {
            return false;
        }
        self.0 = Some(lines.to_vec());
        true
    }
}

/// Replaces the children of `body` with nodes for `lines`.
pub fn rebuild(commands: &mut Commands, body: Entity, lines: &[Line]) {
    commands.entity(body).despawn_children();
    commands.entity(body).with_children(|p| {
        for line in lines {
            p.spawn((
                Node {
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(6.0),
                    row_gap: Val::Px(2.0),
                    ..default()
                },
                Pickable::IGNORE,
            ))
            .with_children(|row| {
                for item in line {
                    spawn_item(row, item);
                }
            });
        }
    });
}

fn spawn_item(row: &mut ChildSpawnerCommands, item: &Item) {
    match item {
        Item::Text { text, size, color } => {
            row.spawn((
                Text::new(text.clone()),
                TextFont::from_font_size(*size),
                TextColor(*color),
                Pickable::IGNORE,
            ));
        }
        Item::Bar { frac, color, width } => {
            track(row, *width).with_children(|t| {
                fill(t, *frac, *color);
            });
        }
        Item::Stack { parts, width } => {
            track(row, *width).with_children(|t| {
                for (frac, color) in parts {
                    fill(t, *frac, *color);
                }
            });
        }
        Item::Chip { text, color } => {
            row.spawn((
                Node {
                    padding: UiRect::axes(Val::Px(6.0), Val::Px(1.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(4.0)),
                    ..default()
                },
                BorderColor::all(*color),
                BackgroundColor(color.with_alpha(0.15)),
                Pickable::IGNORE,
            ))
            .with_children(|c| {
                c.spawn((
                    Text::new(text.clone()),
                    TextFont::from_font_size(12.0),
                    TextColor(*color),
                    Pickable::IGNORE,
                ));
            });
        }
        Item::Dot(color) => {
            row.spawn((
                Node {
                    width: Val::Px(10.0),
                    height: Val::Px(10.0),
                    border_radius: BorderRadius::all(Val::Px(5.0)),
                    ..default()
                },
                BackgroundColor(*color),
                Pickable::IGNORE,
            ));
        }
        Item::Spacer => {
            row.spawn((
                Node {
                    flex_grow: 1.0,
                    ..default()
                },
                Pickable::IGNORE,
            ));
        }
    }
}

fn track<'a>(row: &'a mut ChildSpawnerCommands, width: f32) -> EntityCommands<'a> {
    row.spawn((
        Node {
            width: Val::Px(width),
            height: Val::Px(8.0),
            flex_direction: FlexDirection::Row,
            border_radius: BorderRadius::all(Val::Px(3.0)),
            overflow: Overflow::clip(),
            ..default()
        },
        BackgroundColor(TRACK),
        Pickable::IGNORE,
    ))
}

fn fill(t: &mut ChildSpawnerCommands, frac: f32, color: Color) {
    t.spawn((
        Node {
            width: Val::Percent(frac.clamp(0.0, 1.0) * 100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        BackgroundColor(color),
        Pickable::IGNORE,
    ));
}
