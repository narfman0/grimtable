//! The attack sequence: attacks → hit → wound → allocate → save → damage.
//!
//! This module is edition logic, not faction content. Weapon abilities are an
//! enum because the edition defines a closed list of them; everything a
//! datasheet adds on top arrives later as effects on hooks.
//!
//! Every roll is explained in the outcome's log in our own words, which is what
//! makes the engine a learning tool and not just a referee.

use crate::dice::{Dice, DiceExpr};
use serde::{Deserialize, Serialize};

/// Universal weapon abilities. Parameters are the edition's.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum WeaponAbility {
    Assault,
    Blast,
    DevastatingWounds,
    ExtraAttacks,
    Hazardous,
    Heavy,
    IgnoresCover,
    IndirectFire,
    Lance,
    LethalHits,
    Melta(u32),
    Pistol,
    Precision,
    Psychic,
    RapidFire(u32),
    SustainedHits(DiceExpr),
    Torrent,
    TwinLinked,
    /// `Anti { keyword, threshold }`: unmodified wound rolls of `threshold`+ against
    /// a target with `keyword` are critical wounds.
    Anti { keyword: String, threshold: u8 },
}

impl WeaponAbility {
    /// Parse a printed weapon keyword such as "Rapid Fire 2", "Sustained Hits D3",
    /// "Anti-Vehicle 4+" or "Twin-linked". Case-insensitive.
    pub fn parse(text: &str) -> Option<WeaponAbility> {
        let t = text.replace('\u{a0}', " ");
        let t = t.trim();
        let lower = t.to_ascii_lowercase();
        let number = |prefix: &str| -> Option<String> {
            lower.strip_prefix(prefix).map(|r| r.trim().to_string())
        };
        Some(match lower.as_str() {
            "assault" => WeaponAbility::Assault,
            "blast" => WeaponAbility::Blast,
            "devastating wounds" => WeaponAbility::DevastatingWounds,
            "extra attacks" => WeaponAbility::ExtraAttacks,
            "hazardous" => WeaponAbility::Hazardous,
            "heavy" => WeaponAbility::Heavy,
            "ignores cover" => WeaponAbility::IgnoresCover,
            "indirect fire" => WeaponAbility::IndirectFire,
            "lance" => WeaponAbility::Lance,
            "lethal hits" => WeaponAbility::LethalHits,
            "pistol" => WeaponAbility::Pistol,
            "precision" => WeaponAbility::Precision,
            "psychic" => WeaponAbility::Psychic,
            "torrent" => WeaponAbility::Torrent,
            "twin-linked" | "twin linked" => WeaponAbility::TwinLinked,
            _ => {
                if let Some(n) = number("melta") {
                    WeaponAbility::Melta(n.parse().ok()?)
                } else if let Some(n) = number("rapid fire") {
                    WeaponAbility::RapidFire(n.parse().ok()?)
                } else if let Some(n) = number("sustained hits") {
                    WeaponAbility::SustainedHits(DiceExpr::parse(&n).ok()?)
                } else if let Some(rest) = number("anti-").or_else(|| number("anti ")) {
                    let (kw, thr) = rest.rsplit_once(' ')?;
                    let original = &t[t.len() - rest.len()..];
                    let keyword = original[..kw.len()].trim().to_string();
                    WeaponAbility::Anti { keyword, threshold: thr.trim_end_matches('+').parse().ok()? }
                } else {
                    return None;
                }
            }
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Weapon {
    pub name: String,
    pub attacks: DiceExpr,
    /// Ballistic or weapon skill as the number needed, e.g. 3 for 3+. `None`
    /// means the weapon does not roll to hit (Torrent).
    pub skill: Option<u8>,
    pub strength: u32,
    /// Armour penetration as a positive number: AP-2 is `2`.
    pub ap: u32,
    pub damage: DiceExpr,
    pub abilities: Vec<WeaponAbility>,
}

impl Weapon {
    pub fn has(&self, pred: impl Fn(&WeaponAbility) -> bool) -> bool {
        self.abilities.iter().any(pred)
    }
}

/// The target unit, as far as the attack sequence needs to know it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Target {
    pub toughness: u32,
    /// Armour save as the number needed, e.g. 3 for 3+. 7 means no save.
    pub save: u8,
    pub invuln: Option<u8>,
    pub feel_no_pain: Option<u8>,
    pub wounds_per_model: u32,
    pub models: u32,
    /// Wounds left on the model that currently takes damage, if one is damaged.
    pub damaged_model_wounds: Option<u32>,
    pub keywords: Vec<String>,
    pub in_cover: bool,
}

impl Target {
    fn alive(&self) -> bool {
        self.models > 0
    }

    fn current_model_wounds(&self) -> u32 {
        self.damaged_model_wounds.unwrap_or(self.wounds_per_model)
    }

    /// Apply `amount` to the current model. Returns `(absorbed, destroyed)`.
    /// With `spill`, excess carries on to the next model (mortal wounds);
    /// without, excess is lost when the model dies (normal damage).
    fn take(&mut self, mut amount: u32, spill: bool) -> (u32, u32) {
        let mut destroyed = 0;
        let mut absorbed = 0;
        while amount > 0 && self.alive() {
            let left = self.current_model_wounds();
            if amount >= left {
                amount -= left;
                absorbed += left;
                self.models -= 1;
                self.damaged_model_wounds = None;
                destroyed += 1;
                if !spill {
                    break;
                }
            } else {
                self.damaged_model_wounds = Some(left - amount);
                absorbed += amount;
                amount = 0;
            }
        }
        (absorbed, destroyed)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Reroll {
    #[default]
    None,
    Ones,
    Failed,
    /// Re-roll any roll that is not a critical. Used when fishing for 6s.
    Any,
}

/// Everything about the moment of the attack that is not on the weapon or the
/// target: distance, movement, and modifiers granted by effects on hooks.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Situation {
    pub half_range: bool,
    /// Heavy's condition is decided by the movement phase, not here.
    pub heavy_eligible: bool,
    pub hit_modifier: i32,
    pub wound_modifier: i32,
    pub reroll_hits: Reroll,
    pub reroll_wounds: Reroll,
    pub critical_hit_on: u8,
    pub critical_wound_on: u8,
    /// The edition's net cap on hit and wound modifiers.
    pub modifier_cap: i32,
}

impl Default for Situation {
    fn default() -> Self {
        Situation {
            half_range: false,
            heavy_eligible: false,
            hit_modifier: 0,
            wound_modifier: 0,
            reroll_hits: Reroll::None,
            reroll_wounds: Reroll::None,
            critical_hit_on: 6,
            critical_wound_on: 6,
            modifier_cap: 1,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Outcome {
    pub attacks: u32,
    pub hits: u32,
    pub critical_hits: u32,
    pub wounds: u32,
    pub critical_wounds: u32,
    pub saves_failed: u32,
    pub mortal_wounds: u32,
    pub damage_inflicted: u32,
    pub models_destroyed: u32,
    pub log: Vec<String>,
}

/// The number needed on the wound roll from strength against toughness.
pub fn wound_threshold(strength: u32, toughness: u32) -> u8 {
    if strength >= 2 * toughness {
        2
    } else if strength > toughness {
        3
    } else if strength == toughness {
        4
    } else if 2 * strength <= toughness {
        6
    } else {
        5
    }
}

fn cap(modifier: i32, cap: i32) -> i32 {
    modifier.clamp(-cap, cap)
}

/// One roll with re-roll and modifier handling. Returns `(success, critical)`.
/// An unmodified 1 always fails; an unmodified roll at or above `crit` always
/// succeeds and is critical. Modifiers never turn a roll into a critical.
fn roll_against(
    dice: &mut dyn Dice,
    needed: u8,
    modifier: i32,
    crit: u8,
    reroll: Reroll,
    log: &mut Vec<String>,
    what: &str,
) -> (bool, bool) {
    let evaluate = |raw: u8| -> (bool, bool) {
        if raw == 1 {
            return (false, false);
        }
        if raw >= crit {
            return (true, true);
        }
        ((raw as i32 + modifier) >= needed as i32, false)
    };
    let raw = dice.d6();
    let (ok, critical) = evaluate(raw);
    let wants_reroll = match reroll {
        Reroll::None => false,
        Reroll::Ones => raw == 1,
        Reroll::Failed => !ok,
        Reroll::Any => !critical,
    };
    if !wants_reroll {
        log.push(format!("{what}: rolled {raw}{} → {}", mod_str(modifier), verdict(ok, critical)));
        return (ok, critical);
    }
    let again = dice.d6();
    let (ok2, crit2) = evaluate(again);
    log.push(format!(
        "{what}: rolled {raw}, re-rolled to {again}{} → {}",
        mod_str(modifier),
        verdict(ok2, crit2)
    ));
    (ok2, crit2)
}

fn mod_str(m: i32) -> String {
    match m.signum() {
        0 => String::new(),
        _ => format!(" ({m:+})"),
    }
}

fn verdict(ok: bool, critical: bool) -> &'static str {
    match (ok, critical) {
        (_, true) => "critical",
        (true, _) => "success",
        _ => "fail",
    }
}

/// Resolve every attack of one weapon profile against the target, mutating the
/// target's model count and wounds.
pub fn resolve(weapon: &Weapon, target: &mut Target, sit: &Situation, dice: &mut dyn Dice) -> Outcome {
    let mut out = Outcome::default();
    let mut log: Vec<String> = Vec::new();
    let log = &mut log;

    // Attacks.
    let mut attacks = dice.roll(&weapon.attacks);
    if weapon.has(|a| matches!(a, WeaponAbility::Blast)) {
        let bonus = target.models / 5;
        if bonus > 0 {
            attacks += bonus;
            log.push(format!("Blast: +{bonus} attacks against a unit of {} models", target.models));
        }
    }
    if sit.half_range {
        for a in &weapon.abilities {
            if let WeaponAbility::RapidFire(n) = a {
                attacks += n;
                log.push(format!("Rapid Fire {n}: +{n} attacks at half range"));
            }
        }
    }
    out.attacks = attacks;
    log.push(format!("{}: {attacks} attacks", weapon.name));

    // Hit rolls.
    let torrent = weapon.has(|a| matches!(a, WeaponAbility::Torrent));
    let lethal = weapon.has(|a| matches!(a, WeaponAbility::LethalHits));
    let sustained = weapon.abilities.iter().find_map(|a| match a {
        WeaponAbility::SustainedHits(x) => Some(*x),
        _ => None,
    });
    let mut hit_mod = sit.hit_modifier;
    if sit.heavy_eligible && weapon.has(|a| matches!(a, WeaponAbility::Heavy)) {
        hit_mod += 1;
        log.push("Heavy: +1 to hit".into());
    }
    if target.in_cover && !weapon.has(|a| matches!(a, WeaponAbility::IgnoresCover)) {
        hit_mod -= 1;
        log.push("Target in cover: -1 to hit".into());
    }
    let hit_mod = cap(hit_mod, sit.modifier_cap);

    // Each entry is one hit still to be rolled to wound, or an automatic wound.
    let mut to_wound: u32 = 0;
    let mut auto_wounds: u32 = 0;
    if torrent {
        out.hits = attacks;
        to_wound = attacks;
        log.push("Torrent: every attack hits automatically".into());
    } else if let Some(skill) = weapon.skill {
        for _ in 0..attacks {
            let (hit, critical) =
                roll_against(dice, skill, hit_mod, sit.critical_hit_on, sit.reroll_hits, log, "hit");
            if !hit {
                continue;
            }
            out.hits += 1;
            if critical {
                out.critical_hits += 1;
                if let Some(x) = sustained {
                    let extra = dice.roll(&x);
                    out.hits += extra;
                    to_wound += extra;
                    log.push(format!("Sustained Hits {x}: critical hit scores {extra} extra hits"));
                }
                if lethal {
                    auto_wounds += 1;
                    log.push("Lethal Hits: critical hit wounds automatically".into());
                    continue;
                }
            }
            to_wound += 1;
        }
    }

    // Wound rolls.
    let needed = wound_threshold(weapon.strength, target.toughness);
    let wound_mod = cap(sit.wound_modifier, sit.modifier_cap);
    let mut crit_wound_on = sit.critical_wound_on;
    for a in &weapon.abilities {
        if let WeaponAbility::Anti { keyword, threshold } = a {
            if target.keywords.iter().any(|k| k.eq_ignore_ascii_case(keyword)) && *threshold < crit_wound_on {
                crit_wound_on = *threshold;
                log.push(format!("Anti-{keyword} {threshold}+: wound rolls of {threshold}+ are critical"));
            }
        }
    }
    let reroll_wounds = if weapon.has(|a| matches!(a, WeaponAbility::TwinLinked)) {
        log.push("Twin-linked: re-roll failed wound rolls".into());
        Reroll::Failed
    } else {
        sit.reroll_wounds
    };
    log.push(format!("S{} against T{}: wounding on {needed}+", weapon.strength, target.toughness));
    let devastating = weapon.has(|a| matches!(a, WeaponAbility::DevastatingWounds));
    let melta = if sit.half_range {
        weapon.abilities.iter().find_map(|a| match a {
            WeaponAbility::Melta(n) => Some(*n),
            _ => None,
        })
    } else {
        None
    };
    let roll_damage = |dice: &mut dyn Dice, log: &mut Vec<String>| -> u32 {
        let mut d = dice.roll(&weapon.damage);
        if let Some(n) = melta {
            d += n;
            log.push(format!("Melta {n}: +{n} damage at half range"));
        }
        d
    };

    let mut saves_to_make: u32 = auto_wounds;
    out.wounds += auto_wounds;
    for _ in 0..to_wound {
        let (wounded, critical) = roll_against(dice, needed, wound_mod, crit_wound_on, reroll_wounds, log, "wound");
        if !wounded {
            continue;
        }
        out.wounds += 1;
        if critical {
            out.critical_wounds += 1;
            if devastating {
                let dmg = roll_damage(dice, log);
                log.push(format!("Devastating Wounds: critical wound inflicts {dmg} mortal wounds, no save"));
                let dealt = inflict(target, dmg, true, &mut out, log, dice);
                out.mortal_wounds += dealt;
                continue;
            }
        }
        saves_to_make += 1;
    }

    // Saves and damage, one wound at a time, allocated to the current model.
    let armour = target.save as u32 + weapon.ap;
    let best = match target.invuln {
        Some(inv) if (inv as u32) < armour => {
            log.push(format!("Invulnerable save {inv}+ is better than armour {}+ after AP-{}", armour.min(7), weapon.ap));
            inv as u32
        }
        _ => armour,
    };
    for _ in 0..saves_to_make {
        if !target.alive() {
            log.push("No models left to allocate to".into());
            break;
        }
        let raw = dice.d6();
        let saved = raw != 1 && raw as u32 >= best;
        if saved {
            log.push(format!("save: rolled {raw}, needed {best}+ → saved"));
            continue;
        }
        out.saves_failed += 1;
        let dmg = roll_damage(dice, log);
        log.push(format!("save: rolled {raw}, needed {best}+ → failed, {dmg} damage"));
        inflict(target, dmg, false, &mut out, log, dice);
    }

    out.log = std::mem::take(log);
    out
}

/// Apply damage point by point through Feel No Pain, then to the model.
/// Returns the damage that got through FNP.
fn inflict(
    target: &mut Target,
    amount: u32,
    spill: bool,
    out: &mut Outcome,
    log: &mut Vec<String>,
    dice: &mut dyn Dice,
) -> u32 {
    let mut through = amount;
    if let Some(fnp) = target.feel_no_pain {
        let mut ignored = 0;
        for _ in 0..amount {
            if dice.d6() >= fnp {
                ignored += 1;
            }
        }
        if ignored > 0 {
            log.push(format!("Feel No Pain {fnp}+: ignored {ignored} of {amount} damage"));
        }
        through -= ignored;
    }
    let (absorbed, destroyed) = target.take(through, spill);
    out.damage_inflicted += absorbed;
    out.models_destroyed += destroyed;
    if destroyed > 0 {
        log.push(format!("{destroyed} model(s) destroyed, {} remain", target.models));
    }
    absorbed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dice::Scripted;

    fn gun(attacks: i32, skill: u8, s: u32, ap: u32, d: i32, abilities: Vec<WeaponAbility>) -> Weapon {
        Weapon {
            name: "test gun".into(),
            attacks: DiceExpr::flat(attacks),
            skill: Some(skill),
            strength: s,
            ap,
            damage: DiceExpr::flat(d),
            abilities,
        }
    }

    fn squad(t: u32, save: u8, w: u32, models: u32) -> Target {
        Target {
            toughness: t,
            save,
            invuln: None,
            feel_no_pain: None,
            wounds_per_model: w,
            models,
            damaged_model_wounds: None,
            keywords: vec![],
            in_cover: false,
        }
    }

    #[test]
    fn wound_table() {
        assert_eq!(wound_threshold(8, 4), 2);
        assert_eq!(wound_threshold(5, 4), 3);
        assert_eq!(wound_threshold(4, 4), 4);
        assert_eq!(wound_threshold(3, 4), 5);
        assert_eq!(wound_threshold(2, 4), 6);
        assert_eq!(wound_threshold(4, 9), 6);
    }

    #[test]
    fn plain_sequence_hit_wound_fail_save_damage() {
        let w = gun(1, 3, 4, 1, 2, vec![]);
        let mut t = squad(4, 3, 2, 5);
        // hit 4, wound 4 (4+ needed), save 3 (needs 4+ after AP-1) fails.
        let mut d = Scripted::new([4, 4, 3]);
        let o = resolve(&w, &mut t, &Situation::default(), &mut d);
        assert_eq!((o.hits, o.wounds, o.saves_failed, o.damage_inflicted, o.models_destroyed), (1, 1, 1, 2, 1));
        assert_eq!(t.models, 4);
    }

    #[test]
    fn unmodified_one_always_fails_and_six_always_hits() {
        let w = gun(2, 2, 4, 0, 1, vec![]);
        let mut t = squad(4, 7, 1, 10);
        let sit = Situation { hit_modifier: 5, ..Default::default() };
        // A 1 with +5 would be 6, but still fails. A 6 is a critical. Then one wound roll of 1 fails.
        let mut d = Scripted::new([1, 6, 1]);
        let o = resolve(&w, &mut t, &sit, &mut d);
        assert_eq!((o.hits, o.critical_hits, o.wounds), (1, 1, 0));
    }

    #[test]
    fn modifiers_are_capped_to_plus_minus_one() {
        let w = gun(1, 5, 4, 0, 1, vec![]);
        let mut t = squad(4, 7, 1, 1);
        let sit = Situation { hit_modifier: 3, ..Default::default() };
        // 3 + capped 1 = 4, needs 5: miss.
        let mut d = Scripted::new([3]);
        assert_eq!(resolve(&w, &mut t, &sit, &mut d).hits, 0);
        let mut d = Scripted::new([4, 1]);
        assert_eq!(resolve(&w, &mut t, &sit, &mut d).hits, 1);
    }

    #[test]
    fn heavy_and_cover_cancel_out() {
        let w = gun(1, 4, 4, 0, 1, vec![WeaponAbility::Heavy]);
        let mut t = squad(4, 7, 1, 1);
        t.in_cover = true;
        let sit = Situation { heavy_eligible: true, ..Default::default() };
        let mut d = Scripted::new([4, 1]);
        let o = resolve(&w, &mut t, &sit, &mut d);
        assert_eq!(o.hits, 1);
        assert!(o.log.iter().any(|l| l.contains("Heavy")));
        assert!(o.log.iter().any(|l| l.contains("cover")));
    }

    #[test]
    fn ignores_cover_removes_the_penalty() {
        let w = gun(1, 4, 4, 0, 1, vec![WeaponAbility::IgnoresCover]);
        let mut t = squad(4, 7, 1, 1);
        t.in_cover = true;
        let mut d = Scripted::new([4, 1]);
        assert_eq!(resolve(&w, &mut t, &Situation::default(), &mut d).hits, 1);
    }

    #[test]
    fn torrent_skips_hit_rolls() {
        let w = Weapon { skill: None, ..gun(3, 0, 4, 0, 1, vec![WeaponAbility::Torrent]) };
        let mut t = squad(4, 7, 1, 1);
        // Only wound rolls are consumed: three of them.
        let mut d = Scripted::new([1, 1, 1]);
        let o = resolve(&w, &mut t, &Situation::default(), &mut d);
        assert_eq!((o.attacks, o.hits, o.wounds), (3, 3, 0));
    }

    #[test]
    fn lethal_hits_skip_the_wound_roll() {
        let w = gun(2, 3, 2, 0, 1, vec![WeaponAbility::LethalHits]);
        let mut t = squad(10, 7, 1, 5);
        // hit 6 (critical → auto wound), hit 3 → wound roll 1 fails. Then one save roll of 1 fails.
        let mut d = Scripted::new([6, 3, 1, 1]);
        let o = resolve(&w, &mut t, &Situation::default(), &mut d);
        assert_eq!((o.hits, o.wounds, o.saves_failed), (2, 1, 1));
    }

    #[test]
    fn sustained_hits_add_hits_on_criticals() {
        let w = gun(1, 3, 4, 0, 1, vec![WeaponAbility::SustainedHits(DiceExpr::flat(2))]);
        let mut t = squad(4, 7, 1, 10);
        // hit 6 → 3 hits total; three wound rolls 4,4,4; three saves all fail (7+ needed).
        let mut d = Scripted::new([6, 4, 4, 4, 2, 2, 2]);
        let o = resolve(&w, &mut t, &Situation::default(), &mut d);
        assert_eq!((o.hits, o.wounds, o.models_destroyed), (3, 3, 3));
    }

    #[test]
    fn devastating_wounds_become_mortals_that_spill_over() {
        let w = gun(1, 3, 4, 0, 3, vec![WeaponAbility::DevastatingWounds]);
        let mut t = squad(4, 2, 2, 3);
        // hit 4, wound 6 critical → 3 mortal wounds: kills one 2-wound model and leaves the next on 1.
        let mut d = Scripted::new([4, 6]);
        let o = resolve(&w, &mut t, &Situation::default(), &mut d);
        assert_eq!((o.mortal_wounds, o.models_destroyed, o.saves_failed), (3, 1, 0));
        assert_eq!((t.models, t.damaged_model_wounds), (2, Some(1)));
    }

    #[test]
    fn normal_damage_does_not_spill() {
        let w = gun(1, 3, 8, 0, 6, vec![]);
        let mut t = squad(4, 7, 2, 3);
        let mut d = Scripted::new([4, 4, 1]);
        let o = resolve(&w, &mut t, &Situation::default(), &mut d);
        assert_eq!((o.damage_inflicted, o.models_destroyed), (2, 1));
        assert_eq!(t.models, 2);
    }

    #[test]
    fn anti_keyword_lowers_the_critical_wound_threshold() {
        let w = gun(1, 3, 4, 0, 1, vec![
            WeaponAbility::Anti { keyword: "Vehicle".into(), threshold: 4 },
            WeaponAbility::DevastatingWounds,
        ]);
        let mut t = squad(10, 7, 1, 1);
        t.keywords = vec!["VEHICLE".into()];
        // hit 4; wound 4 would normally fail against T10 (6+), but Anti makes it a critical.
        let mut d = Scripted::new([4, 4]);
        let o = resolve(&w, &mut t, &Situation::default(), &mut d);
        assert_eq!((o.wounds, o.critical_wounds, o.mortal_wounds), (1, 1, 1));
    }

    #[test]
    fn twin_linked_rerolls_failed_wounds() {
        let w = gun(1, 3, 4, 0, 1, vec![WeaponAbility::TwinLinked]);
        let mut t = squad(4, 7, 1, 1);
        let mut d = Scripted::new([4, 2, 5, 1]);
        let o = resolve(&w, &mut t, &Situation::default(), &mut d);
        assert_eq!((o.wounds, o.saves_failed), (1, 1));
    }

    #[test]
    fn rapid_fire_and_melta_apply_at_half_range_only() {
        let w = gun(1, 3, 8, 0, 2, vec![WeaponAbility::RapidFire(1), WeaponAbility::Melta(2)]);
        let mut far = squad(4, 7, 10, 1);
        let mut d = Scripted::new([4, 4, 1]);
        let o = resolve(&w, &mut far, &Situation::default(), &mut d);
        assert_eq!((o.attacks, o.damage_inflicted), (1, 2));

        let mut near = squad(4, 7, 10, 1);
        let sit = Situation { half_range: true, ..Default::default() };
        let mut d = Scripted::new([4, 4, 4, 4, 1, 1]);
        let o = resolve(&w, &mut near, &sit, &mut d);
        assert_eq!((o.attacks, o.damage_inflicted), (2, 8));
    }

    #[test]
    fn blast_adds_one_attack_per_five_models() {
        let w = gun(1, 3, 4, 0, 1, vec![WeaponAbility::Blast]);
        let mut t = squad(4, 7, 1, 12);
        let mut d = Scripted::new([1, 1, 1]);
        assert_eq!(resolve(&w, &mut t, &Situation::default(), &mut d).attacks, 3);
    }

    #[test]
    fn invulnerable_save_is_used_when_better() {
        let w = gun(1, 3, 4, 3, 1, vec![]);
        let mut t = squad(4, 3, 1, 1);
        t.invuln = Some(4);
        // armour would be 6+, invuln 4+: a 4 saves.
        let mut d = Scripted::new([4, 4, 4]);
        let o = resolve(&w, &mut t, &Situation::default(), &mut d);
        assert_eq!(o.saves_failed, 0);
    }

    #[test]
    fn feel_no_pain_rolls_per_point_of_damage() {
        let w = gun(1, 3, 4, 0, 3, vec![]);
        let mut t = squad(4, 7, 5, 1);
        t.feel_no_pain = Some(5);
        // hit, wound, save fails, then three FNP rolls: 5, 2, 6 → two ignored.
        let mut d = Scripted::new([4, 4, 1, 5, 2, 6]);
        let o = resolve(&w, &mut t, &Situation::default(), &mut d);
        assert_eq!(o.damage_inflicted, 1);
        assert_eq!(t.damaged_model_wounds, Some(4));
    }

    #[test]
    fn stops_allocating_once_the_unit_is_dead() {
        let w = gun(3, 2, 8, 0, 1, vec![]);
        let mut t = squad(4, 7, 1, 1);
        // three hits, three wounds, one failed save kills the only model; no more dice.
        let mut d = Scripted::new([4, 4, 4, 4, 4, 4, 1]);
        let o = resolve(&w, &mut t, &Situation::default(), &mut d);
        assert_eq!((o.saves_failed, o.models_destroyed), (1, 1));
    }

    #[test]
    fn engine_never_names_a_faction() {
        // Cheap neutrality check: no faction, unit or named-ability strings in this crate's source.
        // Test modules are excluded, since this list has to live somewhere.
        let src: String = [include_str!("attack.rs"), include_str!("dice.rs"), include_str!("lib.rs")]
            .iter()
            .map(|f| f.split("#[cfg(test)]").next().unwrap_or(""))
            .collect();
        for banned in ["Mechanicus", "Custodes", "Necron", "Skitarii", "Astartes", "Tyranid", "Ork", "Aeldari"] {
            assert!(!src.contains(banned), "engine source names {banned}");
        }
    }
}
