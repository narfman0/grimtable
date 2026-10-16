//! Default mission built from 40kdc deployment-pattern data plus a small,
//! hand-authored terrain layout. Numbers only.

use anyhow::{Context, Result};
use engine::content::{Award, AwardRule, AwardTiming, Mission};
use engine::geom::{Polygon, Vec2};
use serde_json::Value;
use std::path::Path;

/// Build a mission from a named deployment pattern in
/// `data/40kdc-data/data/core/deployment-patterns.json`.
pub fn default_mission(root: &Path, pattern_id: &str) -> Result<Mission> {
    let path = root.join("data/40kdc-data/data/core/deployment-patterns.json");
    let text = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    mission_from_patterns(&text, pattern_id)
}

/// Same, from the JSON text of `deployment-patterns.json`.
pub fn mission_from_patterns(text: &str, pattern_id: &str) -> Result<Mission> {
    let patterns: Vec<Value> = serde_json::from_str(text)?;
    let pattern = patterns.iter().find(|p| p["id"] == pattern_id).with_context(|| format!("no deployment pattern {pattern_id}"))?;

    let mut zones: Vec<Polygon> = Vec::new();
    for z in pattern["zones"].as_array().context("zones")? {
        let ox = z["position"]["x"].as_f64().unwrap_or(0.0) as f32;
        let oy = z["position"]["y"].as_f64().unwrap_or(0.0) as f32;
        let shape = &z["shape"];
        let poly = match shape["type"].as_str() {
            Some("rectangle") => Polygon::rect(ox, oy, shape["width"].as_f64().unwrap_or(0.0) as f32, shape["height"].as_f64().unwrap_or(0.0) as f32),
            _ => Polygon {
                points: shape["points"]
                    .as_array()
                    .context("points")?
                    .iter()
                    .map(|p| Vec2::new(ox + p["x"].as_f64().unwrap_or(0.0) as f32, oy + p["y"].as_f64().unwrap_or(0.0) as f32))
                    .collect(),
            },
        };
        zones.push(poly);
    }
    anyhow::ensure!(zones.len() == 2, "expected two deployment zones");
    let objectives: Vec<Vec2> = pattern["objectives"]
        .as_array()
        .context("objectives")?
        .iter()
        .map(|o| Vec2::new(o["x"].as_f64().unwrap_or(0.0) as f32, o["y"].as_f64().unwrap_or(0.0) as f32))
        .collect();
    // Home objective: the one deepest inside each zone.
    let home = |zone: &Polygon| objectives.iter().position(|&o| zone.contains(o));
    let home_objectives = [home(&zones[0]), home(&zones[1])];

    Ok(Mission {
        name: pattern["name"].as_str().unwrap_or(pattern_id).to_string(),
        deployment_zones: [zones.remove(0), zones.remove(0)],
        objectives,
        home_objectives,
        terrain: default_terrain(),
        awards: default_awards(),
        vp_per_round_cap: 15,
        vp_per_game_cap: 45,
    })
}

/// A symmetric ruins layout: numbers chosen to give every objective some
/// cover and leave firing lanes. Not an official layout.
fn default_terrain() -> Vec<Polygon> {
    let mut t = Vec::new();
    for &(x, y, w, h) in &[
        (26.0, 18.0, 8.0, 8.0),
        (6.0, 18.0, 6.0, 8.0),
        (48.0, 18.0, 6.0, 8.0),
        (26.0, 3.0, 8.0, 5.0),
        (26.0, 36.0, 8.0, 5.0),
        (14.0, 8.0, 5.0, 5.0),
        (41.0, 31.0, 5.0, 5.0),
        (41.0, 8.0, 5.0, 5.0),
        (14.0, 31.0, 5.0, 5.0),
    ] {
        t.push(Polygon::rect(x, y, w, h));
    }
    t
}

/// Primary scoring modelled on the launch Take-and-Hold mirror card:
/// early rounds reward the objective majority at end of turn, later rounds
/// pay per objective at the end of the Command phase, with a bonus on
/// non-home objectives while the home objective is held.
fn default_awards() -> Vec<Award> {
    vec![
        Award { timing: AwardTiming::EndOfTurn, round_min: 1, round_max: 2, rule: AwardRule::ObjectiveMajority { vp: 2 } },
        Award { timing: AwardTiming::EndOfCommandPhase, round_min: 2, round_max: 5, rule: AwardRule::PerObjective { vp: 3 } },
        Award { timing: AwardTiming::EndOfCommandPhase, round_min: 2, round_max: 5, rule: AwardRule::PerNonHomeWhileHoldingHome { vp: 2 } },
    ]
}
