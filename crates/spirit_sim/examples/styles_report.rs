//! The long game played four ways: how much do visitors and winter matter?
//!
//!     cargo run -p spirit_sim --release --example styles_report

use spirit_sim::autoplay::{Offers, Style, play_all_goals_styled};

fn clock(secs: u64) -> String {
    format!("{:02}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
}

fn main() {
    let styles = [
        ("Ignores visitors", Style::default()),
        (
            "Takes every offer",
            Style {
                offers: Offers::Take,
                ..Style::default()
            },
        ),
        (
            "Turns everyone down",
            Style {
                offers: Offers::Refuse,
                ..Style::default()
            },
        ),
        (
            "Plans for winter",
            Style {
                plans_for_winter: true,
                ..Style::default()
            },
        ),
        (
            "Stocks logs all year",
            Style {
                stocks_all_year: true,
                ..Style::default()
            },
        ),
        (
            "Sends caravans early",
            Style {
                eager_caravans: true,
                ..Style::default()
            },
        ),
        (
            "No winter at all",
            Style {
                no_winter: true,
                ..Style::default()
            },
        ),
    ];
    println!("# Styles report\n");
    println!("| Style | All goals | Hearth | Visitors | Offers taken | Missed |");
    println!("|---|---|---|---|---|---|");
    let only = std::env::args().nth(1);
    let chosen = styles
        .into_iter()
        .filter(|s| only.as_ref().is_none_or(|o| s.0.contains(o.as_str())));
    for (name, style) in chosen {
        match play_all_goals_styled(60, style) {
            Ok((w, r)) => {
                w.check_conservation().expect("conservation");
                let hearth = r.met.iter().find(|m| m.0.contains("Hearth"));
                println!(
                    "| {name} | {} | {} | {} | {} | {} |",
                    clock(r.seconds),
                    hearth.map_or("-".to_string(), |m| clock(m.1)),
                    w.settlement.visitors_seen.len(),
                    w.settlement.offers_taken,
                    r.missed.len()
                );
                let came: Vec<&str> = w
                    .settlement
                    .visitors_seen
                    .iter()
                    .map(|i| w.defs.visitors[*i as usize].id.as_str())
                    .collect();
                println!("<!-- {came:?} -->");
            }
            Err(e) => println!("| {name} | FAILED: {e} | | | | |"),
        }
    }
}
