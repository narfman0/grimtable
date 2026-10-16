//! Interpreter for the community ability DSL (40kdc): a tree of
//! `conditional` / `sequence` nodes over leaf effects such as `feel-no-pain`.
//!
//! The tree is kept as loosely typed JSON so unknown node types degrade to a
//! named "apply by hand" note instead of a parse failure. Everything the
//! interpreter understands is listed in `evaluate`; everything else lands in
//! `Buffs::unsupported` and is shown to the players.

use crate::attack::Reroll;
use crate::state::Phase;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub type Effect = Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttackType {
    Ranged,
    Melee,
}

/// The situation an ability is evaluated in.
#[derive(Clone, Default)]
pub struct Ctx<'a> {
    pub phase: Option<Phase>,
    pub my_turn: bool,
    pub attached: bool,
    pub unit_keywords: &'a [String],
    pub target_keywords: Option<&'a [String]>,
    pub attack_type: Option<AttackType>,
    pub battle_shocked: bool,
    pub below_half_strength: bool,
    pub charged_this_turn: bool,
    pub within_range_of_objective: bool,
    pub in_engagement_range: bool,
    /// A timing marker for `timing-is` conditions, e.g. "end-of-command-phase".
    pub timing: Option<&'a str>,
    /// Chosen option per stance or choice name.
    pub stances: Option<&'a BTreeMap<String, usize>>,
    /// Is a friendly unit with every word of the keyword within `range`?
    pub near_friendly: Option<&'a dyn Fn(&str, f32) -> bool>,
}

impl std::fmt::Debug for Ctx<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ctx").field("phase", &self.phase).field("attached", &self.attached).finish()
    }
}

/// What a set of abilities adds up to for one unit in one situation.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Buffs {
    pub feel_no_pain: Option<u8>,
    pub invuln: Option<u8>,
    pub hit_modifier: i32,
    pub wound_modifier: i32,
    pub reroll_hits: Reroll,
    pub reroll_wounds: Reroll,
    /// (stat, delta) for A, S, AP, D, M, T, Sv, OC, Ld, BS, WS.
    pub stat_add: Vec<(String, i32)>,
    pub keywords: Vec<String>,
    pub oc_add: i32,
    pub oc_sticky: bool,
    pub move_add: i32,
    pub damage_reduction: u32,
    pub fights_first: bool,
    pub deep_strike: bool,
    pub stealth: bool,
    pub lone_operative: bool,
    /// Weapon abilities granted to the unit's weapons: (printed keyword, kind filter).
    pub weapon_abilities: Vec<(String, Option<AttackType>)>,
    /// Modifier applied to enemy hit rolls against this unit in melee.
    pub enemy_melee_hit_modifier: i32,
    /// Critical hit threshold override (e.g. 5 for "crits on 5+").
    pub critical_hit_on: Option<u8>,
    /// Wounds recovered at the end of the Command phase, as a dice expression.
    pub recovery: Option<String>,
    /// Stances and choices that still need the player to pick an option.
    pub choices_pending: Vec<(String, Vec<String>)>,
    /// Ability names (or node types) the engine does not apply. Players do.
    pub unsupported: Vec<String>,
}

impl Buffs {
    pub fn merge(&mut self, other: Buffs) {
        if other.feel_no_pain.is_some() {
            self.feel_no_pain = match (self.feel_no_pain, other.feel_no_pain) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
        }
        if other.invuln.is_some() {
            self.invuln = match (self.invuln, other.invuln) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
        }
        self.hit_modifier += other.hit_modifier;
        self.wound_modifier += other.wound_modifier;
        self.reroll_hits = better_reroll(self.reroll_hits, other.reroll_hits);
        self.reroll_wounds = better_reroll(self.reroll_wounds, other.reroll_wounds);
        self.stat_add.extend(other.stat_add);
        self.keywords.extend(other.keywords);
        self.oc_add += other.oc_add;
        self.oc_sticky |= other.oc_sticky;
        self.move_add += other.move_add;
        self.damage_reduction = self.damage_reduction.max(other.damage_reduction);
        self.fights_first |= other.fights_first;
        self.deep_strike |= other.deep_strike;
        self.stealth |= other.stealth;
        self.lone_operative |= other.lone_operative;
        self.weapon_abilities.extend(other.weapon_abilities);
        self.enemy_melee_hit_modifier += other.enemy_melee_hit_modifier;
        self.critical_hit_on = match (self.critical_hit_on, other.critical_hit_on) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        if other.recovery.is_some() {
            self.recovery = other.recovery;
        }
        self.choices_pending.extend(other.choices_pending);
        self.unsupported.extend(other.unsupported);
    }

    pub fn stat(&self, stat: &str) -> i32 {
        self.stat_add.iter().filter(|(s, _)| s.eq_ignore_ascii_case(stat)).map(|(_, v)| v).sum()
    }
}

fn better_reroll(a: Reroll, b: Reroll) -> Reroll {
    let rank = |r: Reroll| match r {
        Reroll::None => 0,
        Reroll::Ones => 1,
        Reroll::Failed => 2,
        Reroll::Any => 3,
    };
    if rank(b) > rank(a) {
        b
    } else {
        a
    }
}

/// Evaluate one ability's effect tree in context. `name` labels notes.
pub fn evaluate(name: &str, effect: &Effect, ctx: &Ctx) -> Buffs {
    let mut buffs = Buffs::default();
    apply(name, effect, ctx, &mut buffs);
    buffs
}

fn apply(name: &str, e: &Effect, ctx: &Ctx, out: &mut Buffs) {
    let ty = e.get("type").and_then(Value::as_str).unwrap_or("");
    let target = e.get("target").and_then(Value::as_str).unwrap_or("self");
    let m = e.get("modifier").cloned().unwrap_or(Value::Null);
    let applies_to_bearer = matches!(target, "self" | "unit" | "bearer" | "model" | "attached-unit" | "all-friendly");
    // Effects scoped to one attack type only apply in that kind of attack.
    if let Some(at) = m.get("attack_type").or(m.get("context")).and_then(Value::as_str) {
        let wanted = match at {
            "melee" => Some(AttackType::Melee),
            "ranged" | "shooting" => Some(AttackType::Ranged),
            _ => None,
        };
        if wanted.is_some() && wanted != ctx.attack_type && ty != "roll-modifier" {
            return;
        }
    }
    match ty {
        "conditional" => {
            let ok = e.get("condition").map(|c| condition(c, ctx, name, out)).unwrap_or(true);
            if ok {
                if let Some(inner) = e.get("effect") {
                    apply(name, inner, ctx, out);
                }
            }
        }
        "sequence" => {
            for step in e.get("steps").and_then(Value::as_array).into_iter().flatten() {
                apply(name, step, ctx, out);
            }
        }
        "stance-select" | "choice" => {
            let label = e.get("choice_label").and_then(Value::as_str).unwrap_or(name).to_string();
            let options: Vec<&Value> = e.get("options").and_then(Value::as_array).into_iter().flatten().collect();
            let names: Vec<String> = options
                .iter()
                .enumerate()
                .map(|(i, o)| o.get("name").and_then(Value::as_str).map(String::from).unwrap_or_else(|| format!("option {}", i + 1)))
                .collect();
            match ctx.stances.and_then(|st| st.get(&label)).and_then(|&i| options.get(i)) {
                Some(opt) => {
                    let inner = opt.get("effect").unwrap_or(opt);
                    apply(name, inner, ctx, out);
                }
                None => out.choices_pending.push((label, names)),
            }
        }
        "bs-modifier" | "ws-modifier" => {
            let v = m.get("value").and_then(Value::as_i64).unwrap_or(1) as i32;
            let v = match m.get("operation").and_then(Value::as_str) {
                Some("worsen") | Some("subtract") => -v,
                _ => v,
            };
            out.stat_add.push((if ty == "bs-modifier" { "BS" } else { "WS" }.to_string(), v));
        }
        "roll-modifier" if target == "attacker" => {
            let roll = m.get("roll").and_then(Value::as_str).unwrap_or("");
            let ctxt = m.get("context").and_then(Value::as_str).unwrap_or("");
            if roll == "hit" && ctxt == "melee" {
                out.enemy_melee_hit_modifier += signed(&m);
            } else {
                out.unsupported.push(format!("{name} (enemy {roll} modifier)"));
            }
        }
        "recovery-pool" => {
            out.recovery = Some(m.get("dice").and_then(Value::as_str).unwrap_or("D3").to_string());
        }
        _ if !applies_to_bearer => out.unsupported.push(format!("{name} ({ty} on {target})")),
        "feel-no-pain" => {
            if m.get("scope").is_some() {
                out.unsupported.push(format!("{name} (scoped feel-no-pain)"));
            } else if let Some(t) = m.get("threshold").and_then(Value::as_u64) {
                out.feel_no_pain = Some(out.feel_no_pain.map_or(t as u8, |x| x.min(t as u8)));
            }
        }
        "invulnerable-save" => {
            if let Some(t) = m.get("invuln_sv").or(m.get("threshold")).and_then(Value::as_u64) {
                out.invuln = Some(out.invuln.map_or(t as u8, |x| x.min(t as u8)));
            }
        }
        "roll-modifier" => {
            let roll = m.get("roll").and_then(Value::as_str).unwrap_or("");
            let v = signed(&m);
            match roll {
                "hit" => out.hit_modifier += v,
                "wound" => out.wound_modifier += v,
                _ => out.unsupported.push(format!("{name} ({roll} roll modifier)")),
            }
        }
        "re-roll" => {
            let roll = m.get("roll").and_then(Value::as_str).unwrap_or("");
            let subset = m.get("subset").and_then(Value::as_str).unwrap_or("all-failures");
            let r = match subset {
                "ones" => Reroll::Ones,
                "all" | "any" => Reroll::Any,
                _ => Reroll::Failed,
            };
            match roll {
                "hit" => out.reroll_hits = better_reroll(out.reroll_hits, r),
                "wound" => out.reroll_wounds = better_reroll(out.reroll_wounds, r),
                _ => out.unsupported.push(format!("{name} ({roll} re-roll)")),
            }
        }
        "stat-modifier" => {
            let stat = m.get("stat").and_then(Value::as_str).unwrap_or("");
            let op = m.get("operation").and_then(Value::as_str).unwrap_or("add");
            if stat.is_empty() || op == "set" {
                out.unsupported.push(format!("{name} (stat {stat} {op})"));
            } else {
                let v = m.get("value").and_then(Value::as_i64).unwrap_or(0) as i32;
                let v = match op {
                    "subtract" | "worsen" => -v,
                    _ => v,
                };
                out.stat_add.push((stat.to_string(), v));
            }
        }
        "keyword-grant" => {
            let kind = match m.get("weapon_type").and_then(Value::as_str) {
                Some("ranged") => Some(AttackType::Ranged),
                Some("melee") => Some(AttackType::Melee),
                _ => None,
            };
            let mut list: Vec<String> = m.get("keywords").and_then(Value::as_array).into_iter().flatten().filter_map(|k| k.as_str().map(String::from)).collect();
            if let Some(k) = m.get("keyword").and_then(Value::as_str) {
                list.push(k.to_string());
            }
            for k in list {
                if crate::attack::WeaponAbility::parse(&k).is_some() {
                    out.weapon_abilities.push((k, kind));
                } else {
                    out.keywords.push(k);
                }
            }
        }
        "objective-control-modifier" => {
            if m.get("sticky").and_then(Value::as_bool) == Some(true) {
                out.oc_sticky = true;
            } else if m.get("value").is_some() {
                out.oc_add += signed(&m);
            } else {
                out.unsupported.push(format!("{name} (objective control)"));
            }
        }
        "movement-modifier" => {
            if m.get("value").is_some() {
                out.move_add += signed(&m);
            } else {
                out.unsupported.push(format!("{name} (movement)"));
            }
        }
        "damage-reduction" => {
            out.damage_reduction = out.damage_reduction.max(m.get("reduction").and_then(Value::as_u64).unwrap_or(0) as u32);
        }
        "deep-strike" => out.deep_strike = true,
        "ability-grant" => match m.get("ability_id").or(m.get("grant_type")).and_then(Value::as_str).unwrap_or("") {
            "melee-critical-hit-on-5-plus" if ctx.attack_type == Some(AttackType::Melee) => out.critical_hit_on = Some(5),
            "melee-critical-hit-on-5-plus" => {}
            "deep-strike" => out.deep_strike = true,
            "fights-first" => out.fights_first = true,
            "stealth" => out.stealth = true,
            "lone-operative" => out.lone_operative = true,
            "leader" | "support" => {}
            other => out.unsupported.push(format!("{name} (grants {other})")),
        },
        other => out.unsupported.push(format!("{name} ({other})")),
    }
}

fn signed(m: &Value) -> i32 {
    let v = m.get("value").and_then(Value::as_i64).unwrap_or(0) as i32;
    match m.get("operation").and_then(Value::as_str) {
        Some("subtract") | Some("worsen") => -v,
        _ => v,
    }
}

fn condition(c: &Value, ctx: &Ctx, name: &str, out: &mut Buffs) -> bool {
    if let Some(op) = c.get("operator").and_then(Value::as_str) {
        let ops: Vec<bool> = c
            .get("operands")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|o| condition(o, ctx, name, out))
            .collect();
        return match op {
            "and" => ops.iter().all(|&b| b),
            "or" => ops.iter().any(|&b| b),
            "not" => !ops.iter().any(|&b| b),
            _ => false,
        };
    }
    let p = c.get("parameters").cloned().unwrap_or(Value::Null);
    let ty = c.get("type").and_then(Value::as_str).unwrap_or("");
    match ty {
        "is-attached" => ctx.attached,
        "unit-has-keyword" => has_kw(ctx.unit_keywords, p.get("keyword")),
        "target-has-keyword" => ctx.target_keywords.map(|k| has_kw(k, p.get("keyword"))).unwrap_or(false),
        "phase-is" => match (p.get("phase").and_then(Value::as_str), ctx.phase) {
            (Some("command"), Some(Phase::Command)) => true,
            (Some("movement"), Some(Phase::Movement)) => true,
            (Some("shooting"), Some(Phase::Shooting)) => true,
            (Some("charge"), Some(Phase::Charge)) => true,
            (Some("fight"), Some(Phase::Fight)) => true,
            _ => false,
        },
        "player-turn-is" => match p.get("turn").and_then(Value::as_str) {
            Some("opponent") | Some("enemy") => !ctx.my_turn,
            _ => ctx.my_turn,
        },
        "attack-is-type" => match (p.get("attack_type").and_then(Value::as_str), ctx.attack_type) {
            (Some("ranged"), Some(AttackType::Ranged)) => true,
            (Some("melee"), Some(AttackType::Melee)) => true,
            _ => false,
        },
        "is-battle-shocked" => ctx.battle_shocked,
        "unit-below-half-strength" => ctx.below_half_strength,
        "charged-this-turn" => ctx.charged_this_turn,
        "within-range-of-objective" => ctx.within_range_of_objective,
        "engagement-state" => match p.get("state").and_then(Value::as_str) {
            Some("in-engagement-range") => ctx.in_engagement_range,
            _ => !ctx.in_engagement_range,
        },
        "timing-is" => p.get("timing").and_then(Value::as_str) == ctx.timing,
        "unit-within-range-of" => {
            let range = p.get("range").and_then(Value::as_f64).unwrap_or(6.0) as f32;
            let kw = p.get("keyword").and_then(Value::as_str).unwrap_or("");
            match (p.get("target_type").and_then(Value::as_str), ctx.near_friendly) {
                (Some("friendly-keyword"), Some(f)) => f(kw, range),
                _ => {
                    out.unsupported.push(format!("{name} (condition unit-within-range-of)"));
                    false
                }
            }
        }
        other => {
            out.unsupported.push(format!("{name} (condition {other})"));
            false
        }
    }
}

fn has_kw(keywords: &[String], wanted: Option<&Value>) -> bool {
    let Some(w) = wanted.and_then(Value::as_str) else { return false };
    // A multi-word keyword requirement means every word must be present.
    w.split_whitespace().all(|word| {
        keywords.iter().any(|k| k.eq_ignore_ascii_case(word) || k.eq_ignore_ascii_case(w))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn leader_grants_fnp_only_while_attached() {
        let e = json!({"type":"sequence","steps":[
            {"type":"conditional","condition":{"operator":"and","operands":[{"type":"is-attached"},{"operator":"not","operands":[{"type":"unit-has-keyword","parameters":{"keyword":"ELECTRO-PRIESTS"}}]}]},
             "effect":{"type":"feel-no-pain","target":"unit","modifier":{"threshold":5}}},
            {"type":"conditional","condition":{"operator":"and","operands":[{"type":"is-attached"},{"type":"unit-has-keyword","parameters":{"keyword":"ELECTRO-PRIESTS"}}]},
             "effect":{"type":"feel-no-pain","target":"unit","modifier":{"threshold":4}}}]});
        let kws = vec!["Infantry".to_string(), "Skitarii".to_string()];
        let detached = Ctx { unit_keywords: &kws, ..Default::default() };
        assert_eq!(evaluate("x", &e, &detached).feel_no_pain, None);
        let attached = Ctx { unit_keywords: &kws, attached: true, ..Default::default() };
        assert_eq!(evaluate("x", &e, &attached).feel_no_pain, Some(5));
        let priests = vec!["Electro-Priests".to_string()];
        let attached = Ctx { unit_keywords: &priests, attached: true, ..Default::default() };
        assert_eq!(evaluate("x", &e, &attached).feel_no_pain, Some(4));
    }

    #[test]
    fn sticky_objectives_and_unknown_nodes() {
        let e = json!({"type":"objective-control-modifier","target":"unit","modifier":{"sticky":true}});
        assert!(evaluate("x", &e, &Ctx::default()).oc_sticky);
        let e = json!({"type":"teleport-homer","target":"unit","modifier":{}});
        assert_eq!(evaluate("Odd", &e, &Ctx::default()).unsupported, vec!["Odd (teleport-homer)".to_string()]);
    }

    #[test]
    fn roll_modifiers_and_rerolls() {
        let e = json!({"type":"sequence","steps":[
            {"type":"roll-modifier","target":"unit","modifier":{"roll":"hit","operation":"add","value":1}},
            {"type":"re-roll","target":"unit","modifier":{"roll":"wound","subset":"ones"}},
            {"type":"conditional","condition":{"type":"phase-is","parameters":{"phase":"fight"}},
             "effect":{"type":"stat-modifier","target":"unit","modifier":{"stat":"A","operation":"add","value":2}}}]});
        let b = evaluate("x", &e, &Ctx { phase: Some(Phase::Shooting), ..Default::default() });
        assert_eq!((b.hit_modifier, b.reroll_wounds, b.stat("A")), (1, Reroll::Ones, 0));
        let b = evaluate("x", &e, &Ctx { phase: Some(Phase::Fight), ..Default::default() });
        assert_eq!(b.stat("A"), 2);
    }
}
