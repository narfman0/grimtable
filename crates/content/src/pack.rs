//! Roster → engine content pack, with 40kdc effect trees attached by ability
//! name. Pure: no file access, so the browser can run it.

use engine::attack::Weapon;
use engine::content::{AbilitySpec, ArmyPack, CoreRule, ModelSpec, Stats, UnitSpec, WeaponKind, WeaponSpec};
use engine::dice::DiceExpr;
use roster::{Army, Base, UnitEntry};
use serde_json::Value;
use std::collections::HashMap;

/// Build a pack. `abilities` maps lower-cased ability name → 40kdc entry
/// (an object with at least `name`, `effect` and `ability_type`).
pub fn build_pack(army: &Army, abilities: &HashMap<String, Value>) -> ArmyPack {
    build_pack_with_detachments(army, abilities, None)
}

/// Same, attaching the chosen detachment's rule when `detachments_json` (the
/// faction's 40kdc `detachments.json` text) names it.
pub fn build_pack_with_detachments(army: &Army, abilities: &HashMap<String, Value>, detachments_json: Option<&str>) -> ArmyPack {
    let mut pack = build_pack_inner(army, abilities);
    if let (Some(text), Some(chosen)) = (detachments_json, &army.detachment) {
        if let Ok(list) = serde_json::from_str::<Vec<Value>>(text) {
            let rule_id = list
                .iter()
                .find(|d| d.get("name").and_then(Value::as_str).map(|n| n.eq_ignore_ascii_case(chosen)).unwrap_or(false))
                .and_then(|d| d.get("detachment_rule_id").and_then(Value::as_str));
            if let Some(id) = rule_id {
                if let Some(a) = abilities.values().find(|a| a.get("ability_id").and_then(Value::as_str) == Some(id)) {
                    if let Some(name) = a.get("name").and_then(Value::as_str) {
                        pack.army_rules.push(AbilitySpec { name: name.to_string(), effect: a.get("effect").cloned() });
                    }
                }
            }
        }
    }
    pack
}

fn build_pack_inner(army: &Army, abilities: &HashMap<String, Value>) -> ArmyPack {
    // Faction-wide rules apply to every unit. Detachment rules are matched by
    // the detachment's name appearing in the entry's unit list or name.
    let mut army_rules: Vec<AbilitySpec> = abilities
        .values()
        .filter(|a| a.get("ability_type").and_then(Value::as_str) == Some("faction"))
        .filter_map(|a| Some(AbilitySpec { name: a.get("name")?.as_str()?.to_string(), effect: a.get("effect").cloned() }))
        .collect();
    army_rules.sort_by(|a, b| a.name.cmp(&b.name));
    ArmyPack {
        name: army.name.clone(),
        faction: army.faction.clone(),
        detachment: army.detachment.clone(),
        points: army.points,
        units: army.units.iter().map(|u| unit_spec(u, abilities)).collect(),
        army_rules,
    }
}

fn base_mm(base: &Base) -> u32 {
    match base {
        Base::Round { mm } => *mm,
        Base::Oval { mm_long, mm_short } => (mm_long + mm_short) / 2,
        // Hull-based vehicles have no base; a large round stand-in keeps the
        // geometry honest enough for engagement and coherency.
        Base::Hull => 90,
        Base::Unknown(_) => 32,
    }
}

fn stats(s: &roster::Stats) -> Stats {
    Stats {
        movement: s.movement,
        toughness: s.toughness,
        save: s.save,
        wounds: s.wounds,
        leadership: s.leadership,
        objective_control: s.objective_control,
        invuln: s.invuln,
        base_mm: base_mm(&s.base),
    }
}

fn core_rule(r: &roster::CoreRule) -> CoreRule {
    use roster::CoreRule as R;
    match r {
        R::DeadlyDemise(d) => CoreRule::DeadlyDemise(d.clone()),
        R::DeepStrike => CoreRule::DeepStrike,
        R::FeelNoPain(t) => CoreRule::FeelNoPain(*t),
        R::FightsFirst => CoreRule::FightsFirst,
        R::FiringDeck(n) => CoreRule::FiringDeck(*n),
        R::Infiltrators => CoreRule::Infiltrators,
        R::Leader => CoreRule::Leader,
        R::LoneOperative => CoreRule::LoneOperative,
        R::Scouts(d) => CoreRule::Scouts(*d),
        R::Stealth => CoreRule::Stealth,
        R::Support => CoreRule::Support,
        R::Other(s) => CoreRule::Other(s.clone()),
    }
}

/// "➤ Eradication beamer - focused" → group "Eradication beamer".
fn profile_group(name: &str) -> Option<String> {
    let n = name.trim_start_matches('➤').trim();
    if n.len() == name.len() {
        return None;
    }
    Some(n.rsplit_once(" - ").map(|(g, _)| g).unwrap_or(n).trim().to_string())
}

fn weapon_spec(w: &roster::WeaponProfile) -> WeaponSpec {
    let abilities = w.keywords.iter().filter_map(|k| roster::parse_weapon_keyword(k)).collect();
    WeaponSpec {
        kind: match w.kind {
            roster::WeaponKind::Ranged => WeaponKind::Ranged,
            roster::WeaponKind::Melee => WeaponKind::Melee,
        },
        range: w.range,
        profile: Weapon {
            name: w.name.trim_start_matches('➤').trim().to_string(),
            attacks: DiceExpr::parse(&w.attacks).unwrap_or(DiceExpr::flat(1)),
            skill: w.skill,
            strength: w.strength,
            ap: w.ap,
            damage: DiceExpr::parse(&w.damage).unwrap_or(DiceExpr::flat(1)),
            abilities,
        },
        profile_group: profile_group(&w.name),
    }
}

fn unit_spec(u: &UnitEntry, abilities: &HashMap<String, Value>) -> UnitSpec {
    let weapons: Vec<WeaponSpec> = u.weapons.iter().map(weapon_spec).collect();
    let index_of = |name: &str| -> Vec<usize> {
        u.weapons.iter().enumerate().filter(|(_, w)| w.name == name).map(|(i, _)| i).collect()
    };
    let models = u
        .models
        .iter()
        .map(|g| {
            let mut idx: Vec<usize> = g.weapons.iter().flat_map(|n| index_of(n)).collect();
            idx.sort_unstable();
            idx.dedup();
            ModelSpec { name: g.name.clone(), count: g.count, stats: g.stats.as_ref().map(stats), weapons: idx }
        })
        .collect();
    let mut keywords = u.keywords.clone();
    keywords.retain(|k| k != "Configuration");
    UnitSpec {
        name: u.name.clone(),
        points: u.points,
        keywords,
        stats: stats(&u.stats),
        models,
        weapons,
        core_rules: u.core_rules.iter().map(core_rule).collect(),
        abilities: u
            .abilities
            .iter()
            .map(|name| AbilitySpec { name: name.clone(), effect: abilities.get(&name.to_ascii_lowercase()).and_then(|a| a.get("effect")).cloned() })
            .collect(),
        can_lead: u.can_lead.clone(),
        transport_capacity: u.transport_capacity,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_alternative_profiles() {
        assert_eq!(profile_group("➤ Eradication beamer - focused").as_deref(), Some("Eradication beamer"));
        assert_eq!(profile_group("Galvanic rifle"), None);
    }
}

/// Army-construction checks a pack must pass for a standard battle. Returns
/// human-readable problems; empty means legal as far as the data can tell.
pub fn validate(pack: &ArmyPack, points_limit: u32) -> Vec<String> {
    let mut problems = Vec::new();
    if pack.points > points_limit {
        problems.push(format!("{} pts is over the {points_limit} pt limit by {}", pack.points, pack.points - points_limit));
    }
    if pack.detachment.is_none() {
        problems.push("no detachment selected".into());
    }
    if pack.units.is_empty() {
        problems.push("no units".into());
    }
    let characters = pack.units.iter().filter(|u| u.keywords.iter().any(|k| k.eq_ignore_ascii_case("Character"))).count();
    if characters == 0 {
        problems.push("no Character to be the Warlord".into());
    }
    let mut epic: Vec<&str> = Vec::new();
    for u in &pack.units {
        if u.keywords.iter().any(|k| k.eq_ignore_ascii_case("Epic Hero")) {
            if epic.contains(&u.name.as_str()) {
                problems.push(format!("{} is an Epic Hero and appears more than once", u.name));
            }
            epic.push(&u.name);
        }
    }
    let mut counts: HashMap<&str, u32> = HashMap::new();
    for u in &pack.units {
        *counts.entry(u.name.as_str()).or_default() += 1;
    }
    for (name, n) in counts {
        let battleline = pack.units.iter().find(|u| u.name == name).map(|u| u.keywords.iter().any(|k| k.eq_ignore_ascii_case("Battleline"))).unwrap_or(false);
        let transport = pack.units.iter().find(|u| u.name == name).map(|u| u.keywords.iter().any(|k| k.eq_ignore_ascii_case("Dedicated Transport"))).unwrap_or(false);
        let limit = if battleline || transport { 6 } else { 3 };
        if n > limit {
            problems.push(format!("{name} appears {n} times; the limit is {limit}"));
        }
    }
    problems
}
