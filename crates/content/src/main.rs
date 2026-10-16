//! Content build. Reads `data/40kdc-data` (effect trees, missions) and
//! `data/wh40k-11e` (stats, points) and joins them on the BSData ids that 40kdc
//! units carry in `external_refs`.
//!
//! Subcommands:
//!   report   print every points and detachment-point disagreement between the
//!            two sources for the configured factions.

use anyhow::{bail, Context, Result};
use content::{enrich, mission, pack};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Faction slug in 40kdc → faction file in BSData.
const FACTIONS: &[(&str, &str)] = &[
    ("adeptus-mechanicus", "Imperium - Adeptus Mechanicus.json"),
    ("adeptus-custodes", "Imperium - Adeptus Custodes.json"),
    ("necrons", "Necrons.json"),
];

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("report") => report(&repo_root()?),
        Some("pack") => {
            let root = repo_root()?;
            let path = args.get(1).context("usage: content pack <file.ros>")?;
            let army = roster::parse_ros_file(path)?;
            let slug = enrich::faction_slug(&army.faction).unwrap_or("_core");
            let abilities = enrich::load_abilities(&root, slug)?;
            let detachments = std::fs::read_to_string(root.join("data/40kdc-data/data/core").join(slug).join("detachments.json")).ok();
            let pack = pack::build_pack_with_detachments(&army, &abilities, detachments.as_deref());
            let missing: Vec<&str> = pack.units.iter().flat_map(|u| u.abilities.iter().filter(|a| a.effect.is_none()).map(|a| a.name.as_str())).collect();
            eprintln!("{}: {} units, {} pts, {} abilities without effect data: {:?}", pack.name, pack.units.len(), pack.points, missing.len(), missing);
            println!("{}", serde_json::to_string_pretty(&pack)?);
            Ok(())
        }
        Some("mission") => {
            let m = mission::default_mission(&repo_root()?, args.get(1).map(String::as_str).unwrap_or("dawn-of-war"))?;
            println!("{}", serde_json::to_string_pretty(&m)?);
            Ok(())
        }
        _ => {
            eprintln!("usage: content report | pack <file.ros> | mission [pattern-id]");
            std::process::exit(2);
        }
    }
}

fn repo_root() -> Result<PathBuf> {
    let mut dir = std::env::current_dir()?;
    loop {
        if dir.join("data/40kdc-data").is_dir() && dir.join("data/wh40k-11e").is_dir() {
            return Ok(dir);
        }
        if !dir.pop() {
            bail!("run from inside the grimtable checkout; data/ submodules not found (git submodule update --init)");
        }
    }
}

fn read_json(path: &Path) -> Result<Value> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
}

/// A unit as 40kdc records it: name, points by model count, the BSData id to join on.
struct KdcUnit {
    name: String,
    points: Vec<(u32, u32)>,
    bsdata_id: Option<String>,
}

fn load_kdc_units(root: &Path, faction: &str) -> Result<Vec<KdcUnit>> {
    let v = read_json(&root.join("data/40kdc-data/data/core").join(faction).join("units.json"))?;
    let mut out = Vec::new();
    for u in v.as_array().context("units.json is not an array")? {
        let points = u["points"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|p| (p["models"].as_u64().unwrap_or(0) as u32, p["cost"].as_u64().unwrap_or(0) as u32))
                    .collect()
            })
            .unwrap_or_default();
        let bsdata_id = u["external_refs"]
            .as_array()
            .and_then(|refs| refs.iter().find(|r| r["namespace"] == "bsdata"))
            .and_then(|r| r["id"].as_str().map(String::from));
        out.push(KdcUnit { name: u["name"].as_str().unwrap_or("?").to_string(), points, bsdata_id });
    }
    Ok(out)
}

fn load_kdc_detachments(root: &Path, faction: &str) -> Result<BTreeMap<String, u32>> {
    let v = read_json(&root.join("data/40kdc-data/data/core").join(faction).join("detachments.json"))?;
    Ok(v.as_array()
        .context("detachments.json is not an array")?
        .iter()
        .filter_map(|d| Some((d["name"].as_str()?.to_string(), d["detachment_points"].as_u64()? as u32)))
        .collect())
}

/// Everything we pull out of one BSData faction file.
#[derive(Default)]
struct BsFaction {
    /// Shared selection entry id → (name, the entry's own non-zero points costs; base size only).
    entries: BTreeMap<String, (String, Vec<u32>)>,
    /// Detachment name → detachment points.
    detachments: BTreeMap<String, u32>,
}

fn load_bsdata(root: &Path, file: &str) -> Result<BsFaction> {
    let v = read_json(&root.join("data/wh40k-11e").join(file))?;
    let mut out = BsFaction::default();
    walk(&v, &mut out);
    Ok(out)
}

fn walk(v: &Value, out: &mut BsFaction) {
    match v {
        Value::Object(map) => {
            if let (Some(id), Some(name), Some(ty)) = (map.get("id"), map.get("name"), map.get("type")) {
                if ty == "unit" || ty == "model" {
                    // The entry's own cost is the base size. Larger sizes are
                    // encoded as conditional modifiers on the pts field, and
                    // wargear costs sit on child entries; neither is read here.
                    let mut pts = Vec::new();
                    own_costs(map, "pts", &mut pts);
                    out.entries
                        .entry(id.as_str().unwrap_or("").to_string())
                        .or_insert((name.as_str().unwrap_or("").to_string(), pts));
                }
                let mut dp = Vec::new();
                collect_costs(v, "Detachment Points", &mut dp);
                if let Some(&d) = dp.first() {
                    out.detachments.entry(name.as_str().unwrap_or("").to_string()).or_insert(d);
                }
            }
            for child in map.values() {
                walk(child, out);
            }
        }
        Value::Array(a) => a.iter().for_each(|c| walk(c, out)),
        _ => {}
    }
}

fn own_costs(map: &serde_json::Map<String, Value>, cost_name: &str, out: &mut Vec<u32>) {
    if let Some(costs) = map.get("costs").and_then(Value::as_array) {
        for c in costs {
            if c["name"] == cost_name {
                let val = c["value"].as_f64().unwrap_or(0.0) as u32;
                if val > 0 {
                    out.push(val);
                }
            }
        }
    }
}

fn collect_costs(v: &Value, cost_name: &str, out: &mut Vec<u32>) {
    match v {
        Value::Object(map) => {
            if let Some(costs) = map.get("costs").and_then(Value::as_array) {
                for c in costs {
                    if c["name"] == cost_name {
                        let val = c["value"].as_f64().unwrap_or(0.0) as u32;
                        if val > 0 {
                            out.push(val);
                        }
                    }
                }
            }
            for child in map.values() {
                collect_costs(child, cost_name, out);
            }
        }
        Value::Array(a) => a.iter().for_each(|c| collect_costs(c, cost_name, out)),
        _ => {}
    }
}

fn report(root: &Path) -> Result<()> {
    let mut total_units = 0;
    let mut unjoined = 0;
    let mut disagreements = 0;
    for (faction, file) in FACTIONS {
        println!("== {faction}");
        let kdc = load_kdc_units(root, faction)?;
        let kdc_det = load_kdc_detachments(root, faction)?;
        let bs = load_bsdata(root, file)?;

        for u in &kdc {
            total_units += 1;
            let Some(id) = &u.bsdata_id else {
                unjoined += 1;
                println!("  no bsdata ref      {}", u.name);
                continue;
            };
            let Some((_, bs_pts)) = bs.entries.get(id) else {
                unjoined += 1;
                println!("  bsdata id missing  {}  ({id})", u.name);
                continue;
            };
            let kdc_costs: Vec<u32> = u.points.iter().map(|&(_, c)| c).collect();
            // Base cost is what both sides agree on most reliably: the smallest non-zero value.
            let kdc_base = kdc_costs.iter().copied().filter(|&c| c > 0).min();
            let bs_base = bs_pts.first().copied();
            if kdc_base != bs_base {
                disagreements += 1;
                println!(
                    "  points             {:40} 40kdc {:?}  bsdata {:?}",
                    u.name, kdc_base, bs_base
                );
            }
        }
        for (name, kdc_dp) in &kdc_det {
            match bs.detachments.get(name) {
                Some(bs_dp) if bs_dp != kdc_dp => {
                    disagreements += 1;
                    println!("  detachment points  {name:40} 40kdc {kdc_dp}  bsdata {bs_dp}");
                }
                None => println!("  detachment unknown {name:40} (not found in bsdata)"),
                _ => {}
            }
        }
    }
    println!();
    println!("units: {total_units}  unjoined: {unjoined}  disagreements: {disagreements}");
    Ok(())
}
