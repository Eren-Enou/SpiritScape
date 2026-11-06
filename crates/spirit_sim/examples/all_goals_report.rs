//! One long game that tries to meet every long-term goal. Deterministic.
//!
//!     cargo run -p spirit_sim --release --example all_goals_report
//!
//! The player never hesitates, so a person will take far longer. This shows the
//! goals can be reached, not how long they should take.

use spirit_sim::autoplay::play_all_goals;
use spirit_sim::save::{decode, encode};
use spirit_sim::world::{Command, World};

fn clock(secs: u64) -> String {
    format!("{:02}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
}

/// Ends the same long game the other way, to see that it holds together with
/// everything built. The boat is paid for here: this isn't a route, just a test.
fn call_them_home(w: &mut World) {
    let main = w.main_storage;
    w.buildings[main].storage.capacity += 200;
    for (id, n) in [("stillwood", 40), ("pitch", 20), ("beams", 20)] {
        let r = w.defs.resource_id(id).expect("resource");
        let put = w.buildings[main].storage.deposit(r, n);
        *w.ledger.seeded.entry(r).or_default() += put as u64;
    }
    let kind = w.defs.building_kind_id("still_boat").expect("boat");
    w.apply(Command::QueueBuilding { kind, site: None })
        .expect("a boat can be planned");
    let start = w.tick;
    while w.boat().is_none() && w.tick < start + 72_000 {
        w.step();
    }
    println!(
        "
## Calling them home
"
    );
    println!("Boat built in {} seconds.", (w.tick - start) / 10);
    let spirits = w.spirits.len();
    let called = w.tick;
    w.apply(Command::CallHome).expect("they can be called");
    while !w.homecoming_finished() && w.tick < called + 72_000 {
        w.step();
    }
    w.check_conservation().expect("conservation");
    println!(
        "{spirits} spirits home in {} seconds.",
        (w.tick - called) / 10
    );
    println!(
        "{}",
        w.ending_story().expect("a story").join(
            "
"
        )
    );
    let (back, _) = decode(&encode(w, "report")).expect("the save loads");
    println!(
        "
Saved and loaded with {} spirits.",
        back.spirits.len()
    );
}

fn main() {
    println!("# All goals report\n");
    match play_all_goals(60) {
        Ok((mut w, r)) => {
            w.check_conservation().expect("conservation");
            println!("| Goal | Met |\n|---|---|");
            for (title, at) in &r.met {
                println!("| {title} | {} |", clock(*at));
            }
            println!("\nMissed: {:?}", r.missed);
            println!("Ended at {}.\n", clock(r.seconds));
            let main = &w.main().storage;
            println!("Main Storage: {} of {}.", main.total(), main.capacity);
            let left: u64 = w.ledger.left_behind.values().sum();
            println!("Left behind: {left} units.");
            let came: Vec<&str> = w
                .settlement
                .visitors_seen
                .iter()
                .map(|i| w.defs.visitors[*i as usize].name.as_str())
                .collect();
            println!("Visitors, in the order they came: {came:?}");
            if std::env::args().any(|a| a == "--home") {
                call_them_home(&mut w);
            }
            if std::env::args().any(|a| a == "--state") {
                println!("\n```text\n{}\n```", r.final_state);
            }
        }
        Err(e) => println!("FAILED: {e}"),
    }
}
