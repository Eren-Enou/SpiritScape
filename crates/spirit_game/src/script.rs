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
//!     9.0 exit
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
        "exit" => Action::Exit,
        _ => return None,
    };
    Some((at, action))
}

fn key(name: &str) -> Option<(KeyCode, Key)> {
    let code = match name {
        "Space" => return Some((KeyCode::Space, Key::Space)),
        "F5" => return Some((KeyCode::F5, Key::F5)),
        "G" => KeyCode::KeyG,
        "H" => KeyCode::KeyH,
        "C" => KeyCode::KeyC,
        "E" => KeyCode::KeyE,
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
    match action {
        Action::Move(p) => move_to(&mut s, p),
        Action::Click(p) => {
            move_to(&mut s, p);
            let e = mouse(ButtonState::Pressed);
            buttons.write(e);
            window_events.write(WindowEvent::MouseButtonInput(e));
            s.release_mouse = true;
        }
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
            exit.write(AppExit::Success);
        }
    }
}
