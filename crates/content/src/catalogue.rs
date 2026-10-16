//! A faction's army-construction data for the editor and for building packs
//! from editor armies. Built from 40kdc core data with BSData points on top.
//! Numbers, names and keywords only.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Catalogue {
    pub faction_id: String,
    pub name: String,
    pub faction_keywords: Vec<String>,
    pub detachments: Vec<Detachment>,
    pub enhancements: Vec<Enhancement>,
    pub units: Vec<Datasheet>,
    pub weapons: Vec<Weapon>,
    /// Ability id → name, for the ids datasheets carry.
    pub ability_names: HashMap<String, String>,
    /// Points source and date, for the editor to display.
    pub points_source: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Detachment {
    pub id: String,
    pub name: String,
    pub detachment_points: u32,
    pub rule_ability_id: Option<String>,
    pub enhancement_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Enhancement {
    pub id: String,
    pub name: String,
    pub detachment_id: String,
    pub cost: u32,
    pub keyword_restrictions: Vec<String>,
    pub ability_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stats {
    pub movement: u32,
    pub toughness: u32,
    pub save: u8,
    pub wounds: u32,
    pub leadership: u8,
    pub objective_control: u32,
    pub invuln: Option<u8>,
    pub base_mm: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PointsTier {
    pub models: u32,
    pub cost: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LoadoutVariant {
    pub name: String,
    pub weapon_ids: Vec<String>,
    pub max_count: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelRow {
    pub name: String,
    pub min: u32,
    pub max: u32,
    pub default_weapon_ids: Vec<String>,
    pub loadout_variants: Vec<LoadoutVariant>,
    pub is_leader_model: bool,
    pub stats: Option<Stats>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Datasheet {
    pub id: String,
    pub name: String,
    pub role: String,
    pub keywords: Vec<String>,
    pub stats: Stats,
    pub points: Vec<PointsTier>,
    pub models: Vec<ModelRow>,
    pub weapon_ids: Vec<String>,
    /// Raw 40kdc wargear options (replaces / replacement / replacement_choice / model_constraint).
    pub wargear_options: Vec<Value>,
    pub ability_ids: Vec<String>,
    /// Datasheet ids this unit can lead (Leader or Support).
    pub can_lead_ids: Vec<String>,
    pub transport_capacity: Option<u32>,
    pub is_epic_hero: bool,
    pub is_battleline: bool,
    pub is_dedicated_transport: bool,
    pub bsdata_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WeaponProfile {
    pub name: String,
    pub range: Option<u32>,
    pub attacks: String,
    pub skill: Option<u8>,
    pub strength: u32,
    pub ap: u32,
    pub damage: String,
    /// Printed keywords like "Rapid Fire 2", "Anti-Vehicle 4+".
    pub keywords: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Weapon {
    pub id: String,
    pub name: String,
    /// "ranged" or "melee".
    pub kind: String,
    pub profiles: Vec<WeaponProfile>,
}

fn read(root: &Path, faction: &str, file: &str) -> Result<Vec<Value>> {
    let path = root.join("data/40kdc-data/data/core").join(faction).join(file);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
}

fn s(v: &Value, k: &str) -> String {
    v.get(k).and_then(Value::as_str).unwrap_or("").to_string()
}

fn strs(v: &Value, k: &str) -> Vec<String> {
    v.get(k).and_then(Value::as_array).into_iter().flatten().filter_map(|x| x.as_str().map(String::from)).collect()
}

fn num(v: &Value, k: &str) -> Option<u64> {
    v.get(k).and_then(|x| x.as_u64().or_else(|| x.as_str().and_then(|t| t.trim_end_matches('+').parse().ok())))
}

/// Stat value that may be a number, "D6", "D3+2" or "N/A".
fn stat_string(v: &Value) -> String {
    match v {
        Value::Number(n) => n.to_string(),
        Value::String(t) => t.clone(),
        _ => "1".into(),
    }
}

fn base_mm(v: &Value) -> u32 {
    match v.get("shape").and_then(Value::as_str) {
        Some("oval") => ((v.get("length").and_then(Value::as_f64).unwrap_or(0.0) + v.get("width").and_then(Value::as_f64).unwrap_or(0.0)) / 2.0) as u32,
        Some("hull") | Some("none") => 90,
        _ => v.get("diameter").and_then(Value::as_f64).unwrap_or(32.0) as u32,
    }
}

fn stats_from(profile: &Value, base: &Value) -> Stats {
    Stats {
        movement: num(profile, "M").unwrap_or(6) as u32,
        toughness: num(profile, "T").unwrap_or(4) as u32,
        save: num(profile, "Sv").unwrap_or(7) as u8,
        wounds: num(profile, "W").unwrap_or(1) as u32,
        leadership: num(profile, "Ld").unwrap_or(7) as u8,
        objective_control: num(profile, "OC").unwrap_or(1) as u32,
        invuln: num(profile, "invuln_sv").map(|x| x as u8),
        base_mm: base_mm(base),
    }
}

/// Printed keyword text from a 40kdc weapon keyword object.
pub fn keyword_text(k: &Value) -> String {
    let id = s(k, "keyword_id");
    let p = k.get("parameters").cloned().unwrap_or(Value::Null);
    let words: String = id.split('-').map(|w| {
        let mut c = w.chars();
        match c.next() {
            Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            None => String::new(),
        }
    }).collect::<Vec<_>>().join(" ");
    match id.as_str() {
        "anti" => format!("Anti-{} {}+", s(&p, "target_keyword"), num(&p, "threshold").unwrap_or(4)),
        "rapid-fire" | "melta" | "sustained-hits" => {
            let v = p.get("value").or(p.get("count")).or(p.get("extra_hits")).map(stat_string).unwrap_or_else(|| "1".into());
            format!("{words} {v}")
        }
        "twin-linked" => "Twin-linked".into(),
        _ => words,
    }
}

/// Build the catalogue for one 40kdc faction slug. `bsdata` overlays points
/// when given: unit bsdata id → base cost, detachment name → detachment points.
pub fn build(root: &Path, faction: &str, bsdata: Option<&BsPoints>) -> Result<Catalogue> {
    anyhow::ensure!(
        !faction.contains('/') && !faction.contains("..") && root.join("data/40kdc-data/data/core").join(faction).join("units.json").exists(),
        "no faction data for {faction}"
    );
    let factions = read(root, faction, "factions.json")?;
    let fac = factions.first().cloned().unwrap_or(Value::Null);
    let compositions = read(root, faction, "unit-compositions.json")?;
    let options = read(root, faction, "wargear-options.json")?;
    let attachments = read(root, faction, "leader-attachments.json")?;
    let mut ability_names = HashMap::new();
    for dir in ["_core", faction] {
        let path = root.join("data/40kdc-data/data/enrichment").join(dir).join("abilities.json");
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(list) = serde_json::from_str::<Vec<Value>>(&text) {
                for a in list {
                    if let (Some(id), Some(name)) = (a.get("ability_id").and_then(Value::as_str), a.get("name").and_then(Value::as_str)) {
                        ability_names.entry(id.to_string()).or_insert(name.to_string());
                    }
                }
            }
        }
    }

    let detachments = read(root, faction, "detachments.json")?
        .iter()
        .filter(|d| d.get("game_modes").and_then(Value::as_array).map(|m| m.iter().any(|x| x == "matched-play")).unwrap_or(true))
        .map(|d| {
            let name = s(d, "name");
            let dp = bsdata.and_then(|b| b.detachment_points.get(&name.to_ascii_lowercase()).copied()).unwrap_or(num(d, "detachment_points").unwrap_or(1) as u32);
            Detachment { id: s(d, "id"), name, detachment_points: dp, rule_ability_id: d.get("detachment_rule_id").and_then(Value::as_str).map(String::from), enhancement_ids: strs(d, "enhancement_ids") }
        })
        .collect();
    let enhancements = read(root, faction, "enhancements.json")?
        .iter()
        .map(|e| Enhancement { id: s(e, "id"), name: s(e, "name"), detachment_id: s(e, "detachment_id"), cost: num(e, "cost").unwrap_or(0) as u32, keyword_restrictions: strs(e, "keyword_restrictions"), ability_id: e.get("ability_id").and_then(Value::as_str).map(String::from) })
        .collect();
    let weapons = read(root, faction, "weapons.json")?
        .iter()
        .map(|w| Weapon {
            id: s(w, "id"),
            name: s(w, "name"),
            kind: s(w, "type"),
            profiles: w
                .get("profiles")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|p| {
                    let st = p.get("stats").cloned().unwrap_or(Value::Null);
                    let ap = st.get("AP").and_then(Value::as_i64).unwrap_or(0).unsigned_abs() as u32;
                    WeaponProfile {
                        name: s(p, "name"),
                        range: p.get("range").and_then(Value::as_u64).map(|r| r as u32),
                        attacks: st.get("A").map(stat_string).unwrap_or_else(|| "1".into()),
                        skill: st.get("BS").or(st.get("WS")).and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|t| t.trim_end_matches('+').parse().ok()))).map(|x| x as u8),
                        strength: st.get("S").and_then(Value::as_u64).unwrap_or(4) as u32,
                        ap,
                        damage: st.get("D").map(stat_string).unwrap_or_else(|| "1".into()),
                        keywords: p.get("keywords").and_then(Value::as_array).into_iter().flatten().map(keyword_text).collect(),
                    }
                })
                .collect(),
        })
        .collect();

    let mut units = Vec::new();
    for u in read(root, faction, "units.json")? {
        if u.get("game_modes").and_then(Value::as_array).map(|m| !m.iter().any(|x| x == "matched-play")).unwrap_or(false) {
            continue;
        }
        if u.get("is_legend").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let id = s(&u, "id");
        let profile = u.get("profiles").and_then(Value::as_array).and_then(|p| p.first()).cloned().unwrap_or(Value::Null);
        let base = u.get("base_size_mm").cloned().unwrap_or(Value::Null);
        let stats = stats_from(&profile, &base);
        let bsdata_id = u.get("external_refs").and_then(Value::as_array).and_then(|r| r.iter().find(|x| x["namespace"] == "bsdata")).and_then(|r| r.get("id")).and_then(Value::as_str).map(String::from);
        let mut points: Vec<PointsTier> = u
            .get("points")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|p| PointsTier { models: num(p, "models").unwrap_or(1) as u32, cost: num(p, "cost").unwrap_or(0) as u32 })
            .collect();
        points.sort_by_key(|p| (p.models, p.cost));
        points.dedup_by_key(|p| p.models);
        if let (Some(b), Some(bid)) = (bsdata, &bsdata_id) {
            if let Some(&base_cost) = b.unit_base_cost.get(bid) {
                if let Some(first) = points.first_mut() {
                    first.cost = base_cost;
                }
            }
        }
        let comp = compositions.iter().find(|c| c.get("unit_id").and_then(Value::as_str) == Some(id.as_str()));
        let profiles: Vec<Value> = u.get("profiles").and_then(Value::as_array).cloned().unwrap_or_default();
        let models: Vec<ModelRow> = match comp.and_then(|c| c.get("models")).and_then(Value::as_array) {
            Some(rows) => rows
                .iter()
                .map(|m| {
                    let pname = m.get("profile_name").and_then(Value::as_str).unwrap_or("");
                    let own = profiles.iter().find(|p| p.get("name").and_then(Value::as_str) == Some(pname)).filter(|_| profiles.len() > 1);
                    ModelRow {
                        name: s(m, "name"),
                        min: num(m, "min").unwrap_or(1) as u32,
                        max: num(m, "max").unwrap_or(1) as u32,
                        default_weapon_ids: strs(m, "default_weapon_ids"),
                        loadout_variants: m
                            .get("loadout_variants")
                            .and_then(Value::as_array)
                            .into_iter()
                            .flatten()
                            .map(|v| LoadoutVariant { name: s(v, "name"), weapon_ids: strs(v, "weapon_ids"), max_count: num(v, "max_count").map(|x| x as u32) })
                            .collect(),
                        is_leader_model: m.get("is_leader_model").and_then(Value::as_bool).unwrap_or(false),
                        stats: own.map(|p| stats_from(p, m.get("base_size_mm").unwrap_or(&base))),
                    }
                })
                .collect(),
            None => {
                let mc = u.get("model_count").cloned().unwrap_or(Value::Null);
                vec![ModelRow { name: s(&u, "name"), min: num(&mc, "min").unwrap_or(1) as u32, max: num(&mc, "max").unwrap_or(1) as u32, default_weapon_ids: strs(&u, "weapon_ids"), loadout_variants: vec![], is_leader_model: false, stats: None }]
            }
        };
        let keywords = strs(&u, "keywords");
        let role = s(&u, "role");
        units.push(Datasheet {
            can_lead_ids: attachments.iter().filter(|a| a.get("leader_id").and_then(Value::as_str) == Some(id.as_str())).flat_map(|a| strs(a, "eligible_bodyguard_ids")).collect(),
            wargear_options: options.iter().filter(|o| o.get("unit_id").and_then(Value::as_str) == Some(id.as_str())).cloned().collect(),
            transport_capacity: u.get("transport_capacity").and_then(|t| t.get("capacity")).and_then(Value::as_u64).map(|x| x as u32),
            is_epic_hero: role == "epic-hero" || keywords.iter().any(|k| k.eq_ignore_ascii_case("Epic Hero")),
            is_battleline: role == "battleline" || keywords.iter().any(|k| k.eq_ignore_ascii_case("Battleline")),
            is_dedicated_transport: role == "dedicated-transport" || keywords.iter().any(|k| k.eq_ignore_ascii_case("Dedicated Transport")),
            id,
            name: s(&u, "name"),
            role,
            keywords,
            stats,
            points,
            models,
            weapon_ids: strs(&u, "weapon_ids"),
            ability_ids: strs(&u, "ability_ids"),
            bsdata_id,
        });
    }
    units.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(Catalogue {
        faction_id: faction.to_string(),
        name: s(&fac, "name"),
        faction_keywords: strs(&fac, "keywords"),
        detachments,
        enhancements,
        units,
        weapons,
        ability_names,
        points_source: if bsdata.is_some() { "40kdc-data with BSData wh40k-11e points".into() } else { "40kdc-data".into() },
    })
}

/// Points pulled out of a BSData faction file.
#[derive(Clone, Debug, Default)]
pub struct BsPoints {
    /// Shared selection entry id → the entry's own base points cost.
    pub unit_base_cost: HashMap<String, u32>,
    /// Lower-cased detachment name → detachment points.
    pub detachment_points: HashMap<String, u32>,
}

/// Read a BSData faction file (`data/wh40k-11e/<file>`).
pub fn bsdata_points(root: &Path, file: &str) -> Result<BsPoints> {
    let path = root.join("data/wh40k-11e").join(file);
    let text = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let v: Value = serde_json::from_str(&text)?;
    let mut out = BsPoints::default();
    walk(&v, &mut out);
    Ok(out)
}

fn walk(v: &Value, out: &mut BsPoints) {
    match v {
        Value::Object(map) => {
            if let (Some(id), Some(name), Some(ty)) = (map.get("id").and_then(Value::as_str), map.get("name").and_then(Value::as_str), map.get("type").and_then(Value::as_str)) {
                let own = |cost_name: &str| -> Option<u32> {
                    map.get("costs").and_then(Value::as_array).into_iter().flatten().find(|c| c["name"] == cost_name).and_then(|c| c["value"].as_f64()).map(|x| x as u32).filter(|&x| x > 0)
                };
                if ty == "unit" || ty == "model" {
                    if let Some(c) = own("pts") {
                        out.unit_base_cost.entry(id.to_string()).or_insert(c);
                    }
                }
                if let Some(dp) = own("Detachment Points") {
                    out.detachment_points.entry(name.to_ascii_lowercase()).or_insert(dp);
                }
            }
            map.values().for_each(|c| walk(c, out));
        }
        Value::Array(a) => a.iter().for_each(|c| walk(c, out)),
        _ => {}
    }
}

/// The BSData file for a 40kdc faction slug, when known.
pub fn bsdata_file(faction: &str) -> Option<&'static str> {
    Some(match faction {
        "adeptus-mechanicus" => "Imperium - Adeptus Mechanicus.json",
        "adeptus-custodes" => "Imperium - Adeptus Custodes.json",
        "necrons" => "Necrons.json",
        "adepta-sororitas" => "Imperium - Adepta Sororitas.json",
        "astra-militarum" => "Imperium - Astra Militarum.json",
        "adeptus-astartes" => "Imperium - Space Marines.json",
        "blood-angels" => "Imperium - Blood Angels.json",
        "dark-angels" => "Imperium - Dark Angels.json",
        "space-wolves" => "Imperium - Space Wolves.json",
        "grey-knights" => "Imperium - Grey Knights.json",
        "imperial-knights" => "Imperium - Imperial Knights.json",
        "agents-of-the-imperium" => "Imperium - Agents of the Imperium.json",
        "chaos-knights" => "Chaos - Chaos Knights.json",
        "chaos-space-marines" => "Chaos - Chaos Space Marines.json",
        "chaos-daemons" => "Chaos - Chaos Daemons.json",
        "death-guard" => "Chaos - Death Guard.json",
        "thousand-sons" => "Chaos - Thousand Sons.json",
        "world-eaters" => "Chaos - World Eaters.json",
        "emperors-children" => "Chaos - Emperor's Children.json",
        "aeldari" => "Aeldari - Craftworlds.json",
        "drukhari" => "Aeldari - Drukhari.json",
        "orks" => "Orks.json",
        "tyranids" => "Tyranids.json",
        "genestealer-cults" => "Genestealer Cults.json",
        "tau-empire" => "T'au Empire.json",
        "leagues-of-votann" => "Leagues of Votann.json",
        _ => return None,
    })
}

/// Every faction slug with a core data directory.
pub fn factions(root: &Path) -> Result<Vec<(String, String)>> {
    let dir = root.join("data/40kdc-data/data/core");
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir).with_context(|| format!("listing {}", dir.display()))? {
        let entry = entry?;
        if !entry.path().join("units.json").exists() {
            continue;
        }
        let slug = entry.file_name().to_string_lossy().to_string();
        let name = read(root, &slug, "factions.json")?.first().map(|f| s(f, "name")).unwrap_or_else(|| slug.clone());
        out.push((slug, name));
    }
    out.sort();
    Ok(out)
}
