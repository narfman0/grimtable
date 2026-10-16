//! A faction-neutral synthetic battle, used by the integration tests and by
//! the client while real content is loading.

use crate::attack::{Weapon, WeaponAbility};
use crate::content::*;
use crate::dice::DiceExpr;
use crate::geom::{Polygon, Vec2};
use crate::state::GameState;

fn rifle() -> WeaponSpec {
    WeaponSpec {
        kind: WeaponKind::Ranged,
        range: Some(24),
        profile: Weapon { name: "rifle".into(), attacks: DiceExpr::flat(2), skill: Some(3), strength: 4, ap: 1, damage: DiceExpr::flat(1), abilities: vec![WeaponAbility::RapidFire(1)] },
        profile_group: None,
    }
}

fn knife() -> WeaponSpec {
    WeaponSpec {
        kind: WeaponKind::Melee,
        range: None,
        profile: Weapon { name: "knife".into(), attacks: DiceExpr::flat(2), skill: Some(3), strength: 4, ap: 0, damage: DiceExpr::flat(1), abilities: vec![] },
        profile_group: None,
    }
}

pub fn squad(name: &str, count: u32, rules: Vec<CoreRule>) -> UnitSpec {
    UnitSpec {
        name: name.into(),
        points: 100,
        keywords: vec!["Infantry".into(), "Battleline".into()],
        stats: Stats { movement: 6, toughness: 4, save: 3, wounds: 2, leadership: 6, objective_control: 2, invuln: None, base_mm: 32 },
        models: vec![ModelSpec { name: "Trooper".into(), count, stats: None, weapons: vec![0, 1] }],
        weapons: vec![rifle(), knife()],
        core_rules: rules,
        abilities: vec![],
        can_lead: vec![],
        transport_capacity: None,
    }
}

pub fn captain() -> UnitSpec {
    let mut c = squad("Captain", 1, vec![CoreRule::Leader]);
    c.keywords.push("Character".into());
    c.stats.wounds = 5;
    c.can_lead = vec!["Troopers".into()];
    c.abilities.push(AbilitySpec {
        name: "Inspiring".into(),
        effect: Some(serde_json::json!({"type":"conditional","condition":{"type":"is-attached"},"effect":{"type":"roll-modifier","target":"unit","modifier":{"roll":"hit","operation":"add","value":1}}})),
    });
    c
}

pub fn mission() -> Mission {
    Mission {
        name: "demo".into(),
        deployment_zones: [Polygon::rect(0.0, 0.0, 60.0, 12.0), Polygon::rect(0.0, 32.0, 60.0, 12.0)],
        objectives: vec![Vec2::new(30.0, 22.0), Vec2::new(10.0, 22.0), Vec2::new(50.0, 22.0), Vec2::new(30.0, 6.0), Vec2::new(30.0, 38.0)],
        home_objectives: [Some(3), Some(4)],
        terrain: vec![Polygon::rect(26.0, 18.0, 8.0, 8.0), Polygon::rect(8.0, 10.0, 6.0, 5.0), Polygon::rect(46.0, 29.0, 6.0, 5.0)],
        awards: vec![
            Award { timing: AwardTiming::EndOfTurn, round_min: 1, round_max: 2, rule: AwardRule::ObjectiveMajority { vp: 2 } },
            Award { timing: AwardTiming::EndOfCommandPhase, round_min: 2, round_max: 5, rule: AwardRule::PerObjective { vp: 3 } },
            Award { timing: AwardTiming::EndOfCommandPhase, round_min: 2, round_max: 5, rule: AwardRule::PerNonHomeWhileHoldingHome { vp: 2 } },
        ],
        vp_per_round_cap: 15,
        vp_per_game_cap: 45,
    }
}

pub fn demo_game(seed: u64) -> GameState {
    let a = ArmyPack { name: "Red".into(), faction: "Red".into(), detachment: None, points: 400, units: vec![squad("Troopers", 5, vec![]), captain(), squad("Scouts", 3, vec![CoreRule::Scouts(6)]), squad("Reserves", 5, vec![CoreRule::DeepStrike])], army_rules: vec![] };
    let b = ArmyPack { name: "Blue".into(), faction: "Blue".into(), detachment: None, points: 300, units: vec![squad("Troopers", 5, vec![]), squad("Troopers", 5, vec![]), squad("Stalkers", 3, vec![CoreRule::Infiltrators, CoreRule::Stealth])], army_rules: vec![] };
    GameState::new([a, b], mission(), EditionParams::default(), seed)
}
