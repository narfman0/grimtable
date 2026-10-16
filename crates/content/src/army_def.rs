//! Editor-built armies: a small definition the client edits, validated
//! against a faction catalogue and turned into an engine pack.

use crate::catalogue::{Catalogue, Datasheet, ModelRow};
use engine::attack::{Weapon, WeaponAbility};
use engine::content::{AbilitySpec, ArmyPack, CoreRule, ModelSpec, Stats, UnitSpec, WeaponKind, WeaponSpec};
use engine::dice::DiceExpr;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ArmyDef {
    pub name: String,
    pub faction_id: String,
    /// Chosen detachments; their detachment points must fit the battle size.
    pub detachment_ids: Vec<String>,
    pub units: Vec<UnitDef>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnitDef {
    pub datasheet_id: String,
    /// One entry per model row of the datasheet, in catalogue order.
    pub models: Vec<ModelRowDef>,
    #[serde(default)]
    pub enhancement_id: Option<String>,
    #[serde(default)]
    pub warlord: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelRowDef {
    pub row: String,
    pub count: u32,
    /// Weapon ids each model in this row carries. Empty means the row default.
    #[serde(default)]
    pub weapon_ids: Vec<String>,
    /// Loadout variants used, by name, with counts (for rows with variants).
    #[serde(default)]
    pub variants: Vec<(String, u32)>,
}

/// Points for a unit: the tier matching the model count, else the smallest
/// tier at or above it, else the largest.
pub fn unit_points(sheet: &Datasheet, model_count: u32) -> u32 {
    if sheet.points.is_empty() {
        return 0;
    }
    sheet
        .points
        .iter()
        .find(|p| p.models == model_count)
        .or_else(|| sheet.points.iter().find(|p| p.models >= model_count))
        .or_else(|| sheet.points.last())
        .map(|p| p.cost)
        .unwrap_or(0)
}

fn model_count(u: &UnitDef) -> u32 {
    u.models.iter().map(|m| m.count).sum()
}

pub fn total_points(def: &ArmyDef, cat: &Catalogue) -> u32 {
    def.units
        .iter()
        .map(|u| {
            let sheet = cat.units.iter().find(|s| s.id == u.datasheet_id);
            let base = sheet.map(|s| unit_points(s, model_count(u))).unwrap_or(0);
            let enh = u.enhancement_id.as_ref().and_then(|e| cat.enhancements.iter().find(|x| &x.id == e)).map(|e| e.cost).unwrap_or(0);
            base + enh
        })
        .sum()
}

pub fn detachment_points(def: &ArmyDef, cat: &Catalogue) -> u32 {
    def.detachment_ids.iter().filter_map(|d| cat.detachments.iter().find(|x| &x.id == d)).map(|d| d.detachment_points).sum()
}

/// Army-construction problems. Empty means legal as far as the data can tell.
pub fn validate(def: &ArmyDef, cat: &Catalogue, points_limit: u32, dp_budget: u32) -> Vec<String> {
    let mut problems = Vec::new();
    let points = total_points(def, cat);
    if points > points_limit {
        problems.push(format!("{points} pts is over the {points_limit} pt limit by {}", points - points_limit));
    }
    if def.detachment_ids.is_empty() {
        problems.push("no detachment selected".into());
    }
    let dp = detachment_points(def, cat);
    if dp > dp_budget {
        problems.push(format!("{dp} detachment points over the budget of {dp_budget}"));
    }
    for d in &def.detachment_ids {
        if !cat.detachments.iter().any(|x| &x.id == d) {
            problems.push(format!("unknown detachment {d}"));
        }
    }
    if def.units.is_empty() {
        problems.push("no units".into());
    }
    let warlords = def.units.iter().filter(|u| u.warlord).count();
    if warlords != 1 {
        problems.push(format!("exactly one Warlord is needed, found {warlords}"));
    }
    let mut counts: HashMap<&str, u32> = HashMap::new();
    let mut enhancements_used: Vec<&str> = Vec::new();
    for u in &def.units {
        let Some(sheet) = cat.units.iter().find(|s| s.id == u.datasheet_id) else {
            problems.push(format!("unknown datasheet {}", u.datasheet_id));
            continue;
        };
        *counts.entry(sheet.id.as_str()).or_default() += 1;
        if u.warlord && !sheet.keywords.iter().any(|k| k.eq_ignore_ascii_case("Character")) {
            problems.push(format!("{} cannot be the Warlord; it is not a Character", sheet.name));
        }
        // Model rows.
        for row in &sheet.models {
            let count = u.models.iter().find(|m| m.row == row.name).map(|m| m.count).unwrap_or(0);
            if count < row.min || count > row.max {
                problems.push(format!("{}: {} needs {}–{} models, has {count}", sheet.name, row.name, row.min, row.max));
            }
        }
        for m in &u.models {
            let Some(row) = sheet.models.iter().find(|r| r.name == m.row) else {
                problems.push(format!("{}: no model row named {}", sheet.name, m.row));
                continue;
            };
            let allowed = allowed_weapons(sheet, row);
            for w in &m.weapon_ids {
                if !allowed.contains(w) {
                    problems.push(format!("{}: {} cannot carry {w}", sheet.name, row.name));
                }
            }
            for (v, n) in &m.variants {
                match row.loadout_variants.iter().find(|x| &x.name == v) {
                    Some(var) => {
                        if let Some(max) = var.max_count {
                            if *n > max {
                                problems.push(format!("{}: at most {max} × {v}", sheet.name));
                            }
                        }
                    }
                    None => problems.push(format!("{}: no loadout variant {v}", sheet.name)),
                }
            }
        }
        // Enhancement.
        if let Some(e) = &u.enhancement_id {
            match cat.enhancements.iter().find(|x| &x.id == e) {
                None => problems.push(format!("unknown enhancement {e}")),
                Some(enh) => {
                    if !def.detachment_ids.contains(&enh.detachment_id) {
                        problems.push(format!("{} belongs to a detachment not in this army", enh.name));
                    }
                    if sheet.is_epic_hero {
                        problems.push(format!("{} is an Epic Hero and cannot take {}", sheet.name, enh.name));
                    }
                    if !sheet.keywords.iter().any(|k| k.eq_ignore_ascii_case("Character")) {
                        problems.push(format!("{} is not a Character and cannot take {}", sheet.name, enh.name));
                    }
                    let unit_keywords: Vec<String> = sheet.keywords.iter().chain(cat.faction_keywords.iter()).map(|k| k.to_ascii_lowercase()).collect();
                    for req in &enh.keyword_restrictions {
                        let ok = req.split_whitespace().all(|w| unit_keywords.iter().any(|k| k == &w.to_ascii_lowercase())) || unit_keywords.contains(&req.to_ascii_lowercase());
                        if !ok {
                            problems.push(format!("{} requires the {req} keyword; {} lacks it", enh.name, sheet.name));
                        }
                    }
                    if enhancements_used.contains(&enh.id.as_str()) {
                        problems.push(format!("{} is taken more than once", enh.name));
                    }
                    enhancements_used.push(&enh.id);
                }
            }
        }
    }
    for (id, n) in counts {
        let sheet = cat.units.iter().find(|s| s.id == id).unwrap();
        let limit = if sheet.is_epic_hero { 1 } else if sheet.is_battleline || sheet.is_dedicated_transport { 6 } else { 3 };
        if n > limit {
            problems.push(format!("{} appears {n} times; the limit is {limit}", sheet.name));
        }
    }
    problems
}

/// Weapons a model row may carry: defaults, loadout variants, and wargear
/// option replacements.
pub fn allowed_weapons(sheet: &Datasheet, row: &ModelRow) -> Vec<String> {
    let mut out: Vec<String> = row.default_weapon_ids.clone();
    for v in &row.loadout_variants {
        out.extend(v.weapon_ids.iter().cloned());
    }
    for o in &sheet.wargear_options {
        let applies = o.get("model_constraint").and_then(|c| c.get("model_name")).and_then(Value::as_str).map(|n| n == row.name).unwrap_or(true);
        if !applies {
            continue;
        }
        for k in ["replacement"] {
            if let Some(a) = o.get(k).and_then(Value::as_array) {
                out.extend(a.iter().filter_map(|x| x.as_str().map(String::from)));
            }
        }
        if let Some(choices) = o.get("replacement_choice").and_then(Value::as_array) {
            for c in choices {
                out.extend(c.as_array().into_iter().flatten().filter_map(|x| x.as_str().map(String::from)));
            }
        }
    }
    out.extend(sheet.weapon_ids.iter().cloned());
    out.sort();
    out.dedup();
    out
}

fn weapon_spec(w: &crate::catalogue::Weapon, p: &crate::catalogue::WeaponProfile, grouped: bool) -> WeaponSpec {
    WeaponSpec {
        kind: if w.kind == "melee" { WeaponKind::Melee } else { WeaponKind::Ranged },
        range: p.range,
        profile: Weapon {
            name: p.name.clone(),
            attacks: DiceExpr::parse(&p.attacks).unwrap_or(DiceExpr::flat(1)),
            skill: p.skill,
            strength: p.strength,
            ap: p.ap,
            damage: DiceExpr::parse(&p.damage).unwrap_or(DiceExpr::flat(1)),
            abilities: p.keywords.iter().filter_map(|k| WeaponAbility::parse(k)).collect(),
        },
        profile_group: if grouped { Some(w.name.clone()) } else { None },
    }
}

fn stats(s: &crate::catalogue::Stats) -> Stats {
    Stats { movement: s.movement, toughness: s.toughness, save: s.save, wounds: s.wounds, leadership: s.leadership, objective_control: s.objective_control, invuln: s.invuln, base_mm: s.base_mm }
}

/// Build the engine pack. `abilities` is the enrichment index (lower-cased
/// name → entry) used for effect trees and detachment rules.
pub fn to_pack(def: &ArmyDef, cat: &Catalogue, abilities: &HashMap<String, Value>) -> ArmyPack {
    let by_ability_id: HashMap<&str, &Value> = abilities.values().filter_map(|a| a.get("ability_id").and_then(Value::as_str).map(|id| (id, a))).collect();
    let mut units = Vec::new();
    for u in &def.units {
        let Some(sheet) = cat.units.iter().find(|s| s.id == u.datasheet_id) else { continue };
        // Weapons: every id any model carries, expanded to profiles.
        let mut weapon_ids: Vec<String> = Vec::new();
        for m in &u.models {
            let row = sheet.models.iter().find(|r| r.name == m.row);
            let ids = if m.weapon_ids.is_empty() { row.map(|r| r.default_weapon_ids.clone()).unwrap_or_default() } else { m.weapon_ids.clone() };
            weapon_ids.extend(ids);
        }
        weapon_ids.sort();
        weapon_ids.dedup();
        let mut weapons: Vec<WeaponSpec> = Vec::new();
        let mut index: HashMap<String, Vec<usize>> = HashMap::new();
        for id in &weapon_ids {
            let Some(w) = cat.weapons.iter().find(|w| &w.id == id) else { continue };
            let grouped = w.profiles.len() > 1;
            for p in &w.profiles {
                index.entry(id.clone()).or_default().push(weapons.len());
                weapons.push(weapon_spec(w, p, grouped));
            }
        }
        let models: Vec<ModelSpec> = u
            .models
            .iter()
            .filter(|m| m.count > 0)
            .map(|m| {
                let row = sheet.models.iter().find(|r| r.name == m.row);
                let ids = if m.weapon_ids.is_empty() { row.map(|r| r.default_weapon_ids.clone()).unwrap_or_default() } else { m.weapon_ids.clone() };
                let mut idx: Vec<usize> = ids.iter().flat_map(|id| index.get(id).cloned().unwrap_or_default()).collect();
                idx.sort_unstable();
                idx.dedup();
                ModelSpec { name: m.row.clone(), count: m.count, stats: row.and_then(|r| r.stats.as_ref()).map(stats), weapons: idx }
            })
            .collect();
        let mut core_rules = Vec::new();
        let mut ability_specs = Vec::new();
        for id in &sheet.ability_ids {
            let name = cat.ability_names.get(id).cloned().unwrap_or_else(|| id.clone());
            let rule = roster::CoreRule::parse(&name);
            match rule {
                roster::CoreRule::Other(_) => {
                    let effect = by_ability_id.get(id.as_str()).and_then(|a| a.get("effect")).cloned().or_else(|| abilities.get(&name.to_ascii_lowercase()).and_then(|a| a.get("effect")).cloned());
                    ability_specs.push(AbilitySpec { name, effect });
                }
                r => core_rules.push(convert_rule(r)),
            }
        }
        if !sheet.can_lead_ids.is_empty() && !core_rules.contains(&CoreRule::Leader) && !core_rules.contains(&CoreRule::Support) {
            core_rules.push(CoreRule::Leader);
        }
        let mut points = unit_points(sheet, model_count(u));
        if let Some(e) = u.enhancement_id.as_ref().and_then(|e| cat.enhancements.iter().find(|x| &x.id == e)) {
            points += e.cost;
            let effect = e.ability_id.as_deref().and_then(|id| by_ability_id.get(id)).and_then(|a| a.get("effect")).cloned();
            ability_specs.push(AbilitySpec { name: e.name.clone(), effect });
        }
        let mut keywords = sheet.keywords.clone();
        for k in &cat.faction_keywords {
            if !keywords.iter().any(|x| x.eq_ignore_ascii_case(k)) {
                keywords.push(k.clone());
            }
        }
        keywords.push(format!("Faction: {}", cat.name));
        if u.warlord {
            keywords.push("Warlord".into());
        }
        units.push(UnitSpec {
            name: sheet.name.clone(),
            points,
            keywords,
            stats: stats(&sheet.stats),
            models,
            weapons,
            core_rules,
            abilities: ability_specs,
            can_lead: sheet.can_lead_ids.iter().filter_map(|id| cat.units.iter().find(|s| &s.id == id)).map(|s| s.name.clone()).collect(),
            transport_capacity: sheet.transport_capacity,
        });
    }
    let mut army_rules: Vec<AbilitySpec> = abilities
        .values()
        .filter(|a| a.get("ability_type").and_then(Value::as_str) == Some("faction"))
        .filter_map(|a| Some(AbilitySpec { name: a.get("name")?.as_str()?.to_string(), effect: a.get("effect").cloned() }))
        .collect();
    for d in def.detachment_ids.iter().filter_map(|d| cat.detachments.iter().find(|x| &x.id == d)) {
        if let Some(a) = d.rule_ability_id.as_deref().and_then(|id| by_ability_id.get(id)) {
            if let Some(name) = a.get("name").and_then(Value::as_str) {
                army_rules.push(AbilitySpec { name: name.to_string(), effect: a.get("effect").cloned() });
            }
        }
    }
    army_rules.sort_by(|a, b| a.name.cmp(&b.name));
    army_rules.dedup_by(|a, b| a.name == b.name);
    let detachment = def.detachment_ids.iter().filter_map(|d| cat.detachments.iter().find(|x| &x.id == d)).map(|d| d.name.clone()).collect::<Vec<_>>().join(" + ");
    ArmyPack {
        name: def.name.clone(),
        faction: cat.name.clone(),
        detachment: if detachment.is_empty() { None } else { Some(detachment) },
        points: total_points(def, cat),
        units,
        army_rules,
    }
}

fn convert_rule(r: roster::CoreRule) -> CoreRule {
    use roster::CoreRule as R;
    match r {
        R::DeadlyDemise(d) => CoreRule::DeadlyDemise(d),
        R::DeepStrike => CoreRule::DeepStrike,
        R::FeelNoPain(t) => CoreRule::FeelNoPain(t),
        R::FightsFirst => CoreRule::FightsFirst,
        R::FiringDeck(n) => CoreRule::FiringDeck(n),
        R::Infiltrators => CoreRule::Infiltrators,
        R::Leader => CoreRule::Leader,
        R::LoneOperative => CoreRule::LoneOperative,
        R::Scouts(d) => CoreRule::Scouts(d),
        R::Stealth => CoreRule::Stealth,
        R::Support => CoreRule::Support,
        R::Other(s) => CoreRule::Other(s),
    }
}

/// A default unit definition for a datasheet: minimum models, default weapons.
pub fn default_unit(sheet: &Datasheet) -> UnitDef {
    UnitDef {
        datasheet_id: sheet.id.clone(),
        models: sheet.models.iter().map(|r| ModelRowDef { row: r.name.clone(), count: r.min, weapon_ids: r.default_weapon_ids.clone(), variants: vec![] }).collect(),
        enhancement_id: None,
        warlord: false,
    }
}
