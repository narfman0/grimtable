//! Subcommands:
//!   scrub <in.ros> <out.ros>   strip prose; the result parses to the same Army
//!   show <file.ros>            print the parsed army, one line per unit

use roster::{parse_ros, parse_ros_file, scrub::scrub};
use std::process::exit;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["scrub", input, output] => run_scrub(input, output),
        ["show", input] => run_show(input),
        _ => {
            eprintln!("usage: roster scrub <in.ros> <out.ros>\n       roster show <file.ros>");
            exit(2);
        }
    };
    if let Err(e) = result {
        eprintln!("error: {e}");
        exit(1);
    }
}

fn run_scrub(input: &str, output: &str) -> Result<(), Box<dyn std::error::Error>> {
    let xml = std::fs::read_to_string(input)?;
    let before = parse_ros(&xml)?;
    let scrubbed = scrub(&xml)?;
    let after = parse_ros(&scrubbed)?;
    if before != after {
        return Err("scrubbed file does not parse to the same army".into());
    }
    std::fs::write(output, scrubbed)?;
    println!(
        "{output}: {} units, {} pts, {} bytes removed",
        after.units.len(),
        after.points,
        xml.len()
            .saturating_sub(std::fs::metadata(output)?.len() as usize)
    );
    Ok(())
}

fn run_show(input: &str) -> Result<(), Box<dyn std::error::Error>> {
    let army = parse_ros_file(input)?;
    println!(
        "{} ({}) — {} — {} — {} pts, {} units",
        army.name,
        army.faction,
        army.detachment.as_deref().unwrap_or("no detachment"),
        army.battle_size.as_deref().unwrap_or("no battle size"),
        army.points,
        army.units.len()
    );
    for u in &army.units {
        let models: u32 = u.models.iter().map(|m| m.count).sum();
        println!(
            "  {:>4} pts  {:<36} {} model(s)  M{} T{} Sv{}+ W{} Ld{}+ OC{}{}  {} weapon(s), {} abilities, rules: {:?}",
            u.points,
            u.name,
            models,
            u.stats.movement,
            u.stats.toughness,
            u.stats.save,
            u.stats.wounds,
            u.stats.leadership,
            u.stats.objective_control,
            u.stats
                .invuln
                .map(|i| format!(" InSv{i}+"))
                .unwrap_or_default(),
            u.weapons.len(),
            u.abilities.len(),
            u.rules
        );
    }
    Ok(())
}
