//! Development aid: drive the game with scripted mouse and keyboard input
//! and save screenshots along the way, then exit.
//!
//!     spirit_game --scenario grove_oversubscribed --script ui_test.txt --capture target/shots
//!
//! Each script line is `<seconds> <action> [args]`, timed from start-up in
//! real (not simulation) seconds. `#` starts a comment.
//!
//!     1.0 move 640 300      # cursor to (640, 300), logical pixels
//!     1.5 click 1338 385    # move there, press and release the left button
//!     2.0 scroll 1380 600 -5  # wheel at a point; negative scrolls down
//!     2.5 key G             # press and release a key (letter, digit, Space, F5)
//!     3.0 shot gathering    # save <capture dir>/<name>.png
//!     4.0 press Separate Soul   # click the visible button with this label
//!     4.2 press Confirm*        # ...or whose label starts like this
//!     4.5 tile 5 14         # click a map tile
//!     5.0 expect Lv 4 -> Lv 3   # fail the run unless this text is on screen
//!     9.0 exit
//!
//! `press`, `tile` and `expect` don't depend on the panel layout. A failed
//! `expect` or a missing button is reported and makes the program exit with
//! an error, so a script works as an automated UI test.
//!
//! Input is written as the same messages the window backend sends, so
//! picking, buttons and `ButtonInput` see it as real input. The OS cursor is
//! never moved: the scripted position is published through [`Cursor`].

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::mouse::{MouseButtonInput, MouseScrollUnit, MouseWheel};
use bevy::input::touch::TouchPhase;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy::ui_widgets::Button;
use bevy::window::{CursorMoved, PrimaryWindow, WindowEvent};

/// Where the mouse cursor is, in logical window pixels. Systems read this
/// instead of `Window::cursor_position` so a script can stand in for the mouse.
#[derive(Resource, Default)]
pub struct Cursor(pub Option<Vec2>);

#[derive(Clone, Debug)]
enum Action {
    Move(Vec2),
    Click(Vec2),
    Scroll(Vec2, f32),
    Key(KeyCode, Key),
    Shot(String),
    /// Click the visible button with this label.
    Press(String),
    /// Click the middle of a map tile.
    Tile(i32, i32),
    /// Some text on screen must contain this.
    Expect(String),
    Exit,
}

#[derive(Resource)]
pub struct Script {
    steps: Vec<(f32, Action)>,
    next: usize,
    dir: String,
    elapsed: f32,
    cursor: Option<Vec2>,
    /// Releases to send on the following frame.
    release_mouse: bool,
    release_key: Option<(KeyCode, Key)>,
    failures: u32,
}

impl Script {
    pub fn from_args(args: &[String]) -> Option<Self> {
        let value = |flag: &str| {
            args.iter()
                .position(|a| a == flag)
                .and_then(|i| args.get(i + 1))
                .cloned()
        };
        let path = value("--script")?;
        let dir = value("--capture").unwrap_or_else(|| "target/script".into());
        let _ = std::fs::create_dir_all(&dir);
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("can't read script {path}: {e}"));
        let mut steps = Vec::new();
        for (n, line) in source.lines().enumerate() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let step =
                parse(line).unwrap_or_else(|| panic!("{path}:{}: can't parse {line:?}", n + 1));
            steps.push(step);
        }
        steps.sort_by(|a, b| a.0.total_cmp(&b.0));
        Some(Self {
            steps,
            next: 0,
            dir,
            elapsed: 0.0,
            cursor: None,
            release_mouse: false,
            release_key: None,
            failures: 0,
        })
    }
}

fn parse(line: &str) -> Option<(f32, Action)> {
    let mut words = line.split_whitespace();
    let at: f32 = words.next()?.parse().ok()?;
    let verb = words.next()?;
    let mut num = || -> Option<f32> { words.next()?.parse().ok() };
    let action = match verb {
        "move" => Action::Move(Vec2::new(num()?, num()?)),
        "click" => Action::Click(Vec2::new(num()?, num()?)),
        "scroll" => Action::Scroll(Vec2::new(num()?, num()?), num()?),
        "key" => {
            let name = line.split_whitespace().nth(2)?;
            let (code, key) = key(name)?;
            Action::Key(code, key)
        }
        "shot" => Action::Shot(line.split_whitespace().nth(2)?.to_string()),
        "press" => Action::Press(rest_of_line(line)?),
        "tile" => Action::Tile(num()? as i32, num()? as i32),
        "expect" => Action::Expect(rest_of_line(line)?),
        "exit" => Action::Exit,
        _ => return None,
    };
    Some((at, action))
}

/// Everything after the time and the verb.
fn rest_of_line(line: &str) -> Option<String> {
    let rest = line.split_once(char::is_whitespace)?.1.trim_start();
    Some(rest.split_once(char::is_whitespace)?.1.trim().to_string())
}

fn key(name: &str) -> Option<(KeyCode, Key)> {
    let code = match name {
        "Space" => return Some((KeyCode::Space, Key::Space)),
        "F5" => return Some((KeyCode::F5, Key::F5)),
        "F9" => return Some((KeyCode::F9, Key::F9)),
        "F6" => return Some((KeyCode::F6, Key::F6)),
        "G" => KeyCode::KeyG,
        "H" => KeyCode::KeyH,
        "I" => KeyCode::KeyI,
        "N" => KeyCode::KeyN,
        "M" => KeyCode::KeyM,
        "Enter" => return Some((KeyCode::Enter, Key::Enter)),
        "C" => KeyCode::KeyC,
        "E" => KeyCode::KeyE,
        "P" => KeyCode::KeyP,
        "J" => KeyCode::KeyJ,
        "L" => KeyCode::KeyL,
        "T" => KeyCode::KeyT,
        "O" => KeyCode::KeyO,
        "R" => KeyCode::KeyR,
        "U" => KeyCode::KeyU,
        "B" => KeyCode::KeyB,
        "V" => KeyCode::KeyV,
        "X" => KeyCode::KeyX,
        "1" => KeyCode::Digit1,
        "2" => KeyCode::Digit2,
        "3" => KeyCode::Digit3,
        "4" => KeyCode::Digit4,
        _ => return None,
    };
    Some((code, Key::Character(name.to_lowercase().into())))
}

pub struct ScriptPlugin;

impl Plugin for ScriptPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Cursor>()
            .add_systems(First, (run_script, track_cursor).chain());
    }
}

/// Publishes the real cursor position, unless a script is driving input.
fn track_cursor(
    script: Option<Res<Script>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut cursor: ResMut<Cursor>,
) {
    cursor.0 = match script {
        Some(s) => s.cursor,
        None => windows.single().ok().and_then(|w| w.cursor_position()),
    };
}

#[allow(clippy::too_many_arguments)]
fn run_script(
    mut commands: Commands,
    time: Res<Time>,
    script: Option<ResMut<Script>>,
    window: Query<Entity, With<PrimaryWindow>>,
    mut window_events: MessageWriter<WindowEvent>,
    mut cursor_moved: MessageWriter<CursorMoved>,
    mut buttons: MessageWriter<MouseButtonInput>,
    mut wheel: MessageWriter<MouseWheel>,
    mut keys: MessageWriter<KeyboardInput>,
    mut exit: MessageWriter<AppExit>,
    texts: Query<(&Text, Option<&ChildOf>, &InheritedVisibility)>,
    button_nodes: Query<(&ComputedNode, &UiGlobalTransform), With<Button>>,
    camera: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    windows: Query<&Window>,
) {
    let (Some(mut s), Ok(window)) = (script, window.single()) else {
        return;
    };
    s.elapsed += time.delta_secs();

    let mouse = |state| MouseButtonInput {
        button: MouseButton::Left,
        state,
        window,
    };
    let keyboard = |code, key, state| KeyboardInput {
        key_code: code,
        logical_key: key,
        state,
        text: None,
        repeat: false,
        window,
    };
    if std::mem::take(&mut s.release_mouse) {
        let e = mouse(ButtonState::Released);
        buttons.write(e);
        window_events.write(WindowEvent::MouseButtonInput(e));
    }
    if let Some((code, key)) = s.release_key.take() {
        let e = keyboard(code, key, ButtonState::Released);
        keys.write(e.clone());
        window_events.write(WindowEvent::KeyboardInput(e));
    }

    // One action per frame, so presses and releases land on separate frames.
    let Some((at, action)) = s.steps.get(s.next).cloned() else {
        return;
    };
    if s.elapsed < at || s.release_mouse || s.release_key.is_some() {
        return;
    }
    s.next += 1;
    info!("script {at:>5.1}s: {action:?}");
    let mut move_to = |s: &mut Script, p: Vec2| {
        let e = CursorMoved {
            window,
            position: p,
            delta: s.cursor.map(|c| p - c),
        };
        s.cursor = Some(p);
        cursor_moved.write(e.clone());
        window_events.write(WindowEvent::CursorMoved(e));
    };
    // Resolve layout-independent targets to a screen point.
    let action = match action {
        Action::Press(label) => {
            let found = texts.iter().find_map(|(text, parent, visible)| {
                let matches = match label.strip_suffix('*') {
                    Some(prefix) => text.0.starts_with(prefix),
                    None => text.0 == label,
                };
                if !matches || !visible.get() {
                    return None;
                }
                let (node, transform) = button_nodes.get(parent?.parent()).ok()?;
                (node.size().x > 0.0).then(|| transform.translation * node.inverse_scale_factor())
            });
            let height = windows.single().map_or(f32::MAX, |w| w.height());
            match found {
                Some(p) if p.y < height => Action::Click(p),
                Some(_) => {
                    error!("script FAIL: button {label:?} is below the window: scroll first");
                    s.failures += 1;
                    return;
                }
                None => {
                    error!("script FAIL: no visible button {label:?}");
                    s.failures += 1;
                    return;
                }
            }
        }
        Action::Tile(x, y) => {
            let at = crate::world_view::tile_world(x as f32, y as f32);
            let point = camera
                .single()
                .ok()
                .and_then(|(c, t)| c.world_to_viewport(t, at).ok());
            match point {
                Some(p) => Action::Click(p),
                None => {
                    error!("script FAIL: tile ({x}, {y}) is off screen");
                    s.failures += 1;
                    return;
                }
            }
        }
        other => other,
    };
    match action {
        Action::Move(p) => move_to(&mut s, p),
        Action::Click(p) => {
            move_to(&mut s, p);
            let e = mouse(ButtonState::Pressed);
            buttons.write(e);
            window_events.write(WindowEvent::MouseButtonInput(e));
            s.release_mouse = true;
        }
        Action::Expect(wanted) => {
            let seen = texts
                .iter()
                .any(|(text, _, visible)| visible.get() && text.0.contains(&wanted));
            if !seen {
                error!("script FAIL: {wanted:?} is not on screen");
                s.failures += 1;
            }
        }
        Action::Press(_) | Action::Tile(..) => unreachable!("resolved above"),
        Action::Scroll(p, lines) => {
            move_to(&mut s, p);
            let e = MouseWheel {
                unit: MouseScrollUnit::Line,
                x: 0.0,
                y: lines,
                window,
                phase: TouchPhase::Moved,
            };
            wheel.write(e);
            window_events.write(WindowEvent::MouseWheel(e));
        }
        Action::Key(code, key) => {
            let e = keyboard(code, key.clone(), ButtonState::Pressed);
            keys.write(e.clone());
            window_events.write(WindowEvent::KeyboardInput(e));
            s.release_key = Some((code, key));
        }
        Action::Shot(name) => {
            let path = format!("{}/{name}.png", s.dir);
            commands
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk(path));
        }
        Action::Exit => {
            if s.failures == 0 {
                info!("script PASS");
                exit.write(AppExit::Success);
            } else {
                error!("script FAILED: {} problem(s)", s.failures);
                exit.write(AppExit::from_code(1));
            }
        }
    }
}
