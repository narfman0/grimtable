//! A whole small battle through the phase machine with synthetic armies.
//! Faction-neutral: two identical squads of "troopers" with a rifle and a knife.

use engine::actions::{apply, Action};
use engine::attack::{Weapon, WeaponAbility};
use engine::content::*;
use engine::dice::DiceExpr;
use engine::geom::{Polygon, Vec2};
use engine::state::{GameState, MoveKind, Phase};

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

fn squad(name: &str, count: u32, rules: Vec<CoreRule>) -> UnitSpec {
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

fn captain() -> UnitSpec {
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

fn mission() -> Mission {
    Mission {
        name: "test".into(),
        deployment_zones: [Polygon::rect(0.0, 0.0, 60.0, 12.0), Polygon::rect(0.0, 32.0, 60.0, 12.0)],
        objectives: vec![Vec2::new(24.0, 19.0), Vec2::new(10.0, 8.0), Vec2::new(50.0, 36.0)],
        home_objectives: [Some(1), Some(2)],
        terrain: vec![Polygon::rect(25.0, 18.0, 4.0, 8.0)],
        awards: vec![Award { timing: AwardTiming::EndOfCommandPhase, round_min: 2, round_max: 5, rule: AwardRule::PerObjective { vp: 3 } }],
        vp_per_round_cap: 15,
        vp_per_game_cap: 45,
    }
}

fn new_game(seed: u64) -> GameState {
    let a = ArmyPack { name: "Red".into(), faction: "A".into(), detachment: None, points: 300, units: vec![squad("Troopers", 5, vec![]), captain(), squad("Scouts", 3, vec![CoreRule::Scouts(6)])] };
    let b = ArmyPack { name: "Blue".into(), faction: "B".into(), detachment: None, points: 200, units: vec![squad("Troopers", 5, vec![]), squad("Troopers", 5, vec![])] };
    GameState::new([a, b], mission(), EditionParams::default(), seed)
}

fn line(x: f32, y: f32, n: usize) -> Vec<Vec2> {
    (0..n).map(|i| Vec2::new(x + i as f32 * 1.5, y)).collect()
}

fn ok(state: &mut GameState, player: usize, action: Action) {
    if let Err(e) = apply(state, player, action.clone()) {
        panic!("{action:?} failed: {e}\nlog:\n{}", state.log.iter().map(|e| e.text.clone()).collect::<Vec<_>>().join("\n"));
    }
}

#[test]
fn muster_attaches_leader_and_grants_its_ability() {
    let mut s = new_game(1);
    assert!(apply(&mut s, 0, Action::Attach { leader: 1, bodyguard: 2 }).is_err(), "captain cannot lead scouts");
    ok(&mut s, 0, Action::Attach { leader: 1, bodyguard: 0 });
    assert_eq!(s.units[0].models.len(), 6);
    assert!(s.units[0].models[5].is_leader);
    assert!(s.units[1].is_destroyed(), "merged leader is no longer its own unit");
    let buffs = engine::rules::unit_buffs(&s, &s.units[0], None, None);
    assert_eq!(buffs.hit_modifier, 1);
}

#[test]
fn deployment_enforces_zones_and_alternates() {
    let mut s = new_game(1);
    ok(&mut s, 0, Action::FinishMuster);
    ok(&mut s, 1, Action::FinishMuster);
    assert_eq!(s.phase, Phase::Deployment);
    let first = s.first_player;
    let other = 1 - first;
    let (zone_y, other_y) = if first == 0 { (5.0, 40.0) } else { (40.0, 5.0) };
    // Wrong zone is rejected, wrong player is rejected.
    assert!(apply(&mut s, first, Action::Deploy { unit: if first == 0 { 0 } else { 3 }, positions: line(5.0, 22.0, 5) }).is_err());
    assert!(apply(&mut s, other, Action::Deploy { unit: if other == 0 { 0 } else { 3 }, positions: line(5.0, other_y, 5) }).is_err());
    ok(&mut s, first, Action::Deploy { unit: if first == 0 { 0 } else { 3 }, positions: line(5.0, zone_y, 5) });
    assert_eq!(s.active, other);
}

/// Plays a full five-round battle with both sides advancing and shooting.
#[test]
fn full_battle_runs_to_a_result() {
    let mut s = new_game(7);
    ok(&mut s, 0, Action::Attach { leader: 1, bodyguard: 0 });
    ok(&mut s, 0, Action::FinishMuster);
    ok(&mut s, 1, Action::FinishMuster);
    // Deploy everything: red at the top, blue at the bottom.
    let red = [(0usize, line(20.0, 6.0, 6)), (2, line(5.0, 6.0, 3))];
    let blue = [(3usize, line(20.0, 38.0, 5)), (4, line(40.0, 38.0, 5))];
    let mut r = red.iter();
    let mut b = blue.iter();
    while s.phase == Phase::Deployment {
        let p = s.active;
        let next = if p == 0 { r.next() } else { b.next() };
        let (unit, pos) = next.expect("ran out of units to deploy");
        ok(&mut s, p, Action::Deploy { unit: *unit, positions: pos.clone() });
    }
    assert_eq!(s.phase, Phase::PreBattle);
    // Scouts move 6" toward the middle.
    if s.active == 0 {
        ok(&mut s, 0, Action::ScoutMove { unit: 2, positions: line(5.0, 12.0, 3) });
    }
    let p = s.active;
    ok(&mut s, p, Action::EndPhase);
    if s.phase == Phase::PreBattle {
        if s.active == 0 {
            ok(&mut s, 0, Action::ScoutMove { unit: 2, positions: line(5.0, 12.0, 3) });
        }
        let p = s.active;
        ok(&mut s, p, Action::EndPhase);
    }
    assert_eq!((s.round, s.phase), (1, Phase::Command));

    let mut turns = 0;
    while s.phase != Phase::Ended {
        turns += 1;
        assert!(turns < 40, "game did not end");
        let p = s.active;
        ok(&mut s, p, Action::EndPhase); // command → movement
        // Move every unit 6" toward the enemy, keeping formation.
        let ids: Vec<usize> = s.units.iter().filter(|u| u.owner == p && u.on_board()).map(|u| u.id).collect();
        for id in ids {
            let dy: f32 = if p == 0 { 6.0 } else { -6.0 };
            let pos: Vec<Vec2> = s.units[id].alive_models().map(|m| m.pos.unwrap()).map(|q| Vec2::new(q.x, (q.y + dy).clamp(1.0, 43.0))).collect();
            if apply(&mut s, p, Action::Move { unit: id, kind: MoveKind::Normal, positions: pos }).is_err() {
                ok(&mut s, p, Action::Move { unit: id, kind: MoveKind::Stationary, positions: vec![] });
            }
        }
        ok(&mut s, p, Action::EndPhase); // → shooting
        let ids: Vec<usize> = s.units.iter().filter(|u| u.owner == p && u.on_board()).map(|u| u.id).collect();
        let enemies: Vec<usize> = s.units.iter().filter(|u| u.owner != p && u.on_board()).map(|u| u.id).collect();
        for id in ids {
            for &e in &enemies {
                if apply(&mut s, p, Action::Shoot { unit: id, assignments: vec![(0, e)] }).is_ok() {
                    break;
                }
            }
        }
        ok(&mut s, p, Action::EndPhase); // → charge
        ok(&mut s, p, Action::EndPhase); // → fight
        while let Some(next) = engine::combat::next_fighter(&s) {
            let owner = s.units[next].owner;
            let target = engine::rules::enemies_in_engagement_range(&s, next).next().unwrap().id;
            ok(&mut s, owner, Action::Fight { unit: next, target, pile_in: None, consolidate: None });
        }
        ok(&mut s, p, Action::EndPhase); // → next turn
    }
    assert!(s.round == 5);
    assert!(s.winner.is_some());
    let text: Vec<String> = s.log.iter().map(|e| e.text.clone()).collect();
    assert!(text.iter().any(|t| t.contains("Battle over")), "{}", text.join("\n"));
    assert!(text.iter().any(|t| t.contains("attacks")), "somebody should have shot");
    assert!(s.players[0].vp + s.players[1].vp > 0, "somebody should have scored: {}", text.join("\n"));
}

#[test]
fn same_seed_same_battle() {
    let run = |seed| {
        let mut s = new_game(seed);
        ok(&mut s, 0, Action::FinishMuster);
        ok(&mut s, 1, Action::FinishMuster);
        (s.first_player, s.rng)
    };
    assert_eq!(run(3), run(3));
}

/// Charge into a unit five inches away, then fight: chargers first, then the
/// defender swings back.
#[test]
fn charge_and_fight() {
    let mut s = new_game(11);
    ok(&mut s, 0, Action::FinishMuster);
    ok(&mut s, 1, Action::FinishMuster);
    // Deploy minimal armies so the fight is the whole story.
    let (p0, p1) = (s.first_player, 1 - s.first_player);
    let units_of = |s: &GameState, p: usize| -> Vec<usize> { s.units.iter().filter(|u| u.owner == p && u.merged_into.is_none()).map(|u| u.id).collect() };
    let (y0, y1) = if p0 == 0 { (6.0, 38.0) } else { (38.0, 6.0) };
    let mut i0 = 0;
    let mut i1 = 0;
    while s.phase == Phase::Deployment {
        let p = s.active;
        if p == p0 {
            let id = units_of(&s, p0)[i0];
            let n = s.units[id].model_count() as usize;
            ok(&mut s, p, Action::Deploy { unit: id, positions: line(5.0 + i0 as f32 * 12.0, y0, n) });
            i0 += 1;
        } else {
            let id = units_of(&s, p1)[i1];
            let n = s.units[id].model_count() as usize;
            ok(&mut s, p, Action::Deploy { unit: id, positions: line(5.0 + i1 as f32 * 12.0, y1, n) });
            i1 += 1;
        }
    }
    let p = s.active;
    ok(&mut s, p, Action::EndPhase);
    let p = s.active;
    ok(&mut s, p, Action::EndPhase);
    assert_eq!(s.phase, Phase::Command);
    // Teleport for the test: put red troopers (unit 0) 5" from blue troopers (unit 3).
    for (i, m) in s.units[0].models.iter_mut().enumerate() {
        m.pos = Some(Vec2::new(10.0 + i as f32 * 1.5, 20.0));
    }
    for (i, m) in s.units[3].models.iter_mut().enumerate() {
        m.pos = Some(Vec2::new(10.0 + i as f32 * 1.5, 26.0));
    }
    s.active = 0;
    ok(&mut s, 0, Action::EndPhase); // movement
    ok(&mut s, 0, Action::EndPhase); // shooting
    ok(&mut s, 0, Action::EndPhase); // charge
    assert_eq!(s.phase, Phase::Charge);
    assert!(apply(&mut s, 0, Action::DeclareCharge { unit: 0, targets: vec![4] }).is_err(), "unit 4 is too far");
    // Force a roll that reaches: retry seeds until the charge succeeds.
    let mut attempts = 0;
    loop {
        let before = s.clone();
        ok(&mut s, 0, Action::DeclareCharge { unit: 0, targets: vec![3] });
        if s.pending.is_some() {
            break;
        }
        attempts += 1;
        assert!(attempts < 20);
        s = before;
        s.rng = engine::dice::Rng::new(100 + attempts);
    }
    assert!(apply(&mut s, 0, Action::EndPhase).is_err(), "must finish the charge move first");
    // Move into base contact-ish: 1" short of the target line.
    let pos: Vec<Vec2> = (0..5).map(|i| Vec2::new(10.0 + i as f32 * 1.5, 24.3)).collect();
    ok(&mut s, 0, Action::ChargeMove { unit: 0, positions: pos });
    assert!(s.units[0].flags.charged);
    ok(&mut s, 0, Action::EndPhase);
    assert_eq!(s.phase, Phase::Fight);
    assert_eq!(engine::combat::next_fighter(&s), Some(0), "the charger fights first");
    assert!(apply(&mut s, 1, Action::Fight { unit: 3, target: 0, pile_in: None, consolidate: None }).is_err());
    ok(&mut s, 0, Action::Fight { unit: 0, target: 3, pile_in: None, consolidate: None });
    assert_eq!(engine::combat::next_fighter(&s), Some(3), "then the defender");
    ok(&mut s, 1, Action::Fight { unit: 3, target: 0, pile_in: None, consolidate: None });
    assert_eq!(engine::combat::next_fighter(&s), None);
    let text: Vec<String> = s.log.iter().map(|e| e.text.clone()).collect();
    assert!(text.iter().filter(|t| t.contains("x knife at")).count() == 2, "{}", text.join("\n"));
    ok(&mut s, 0, Action::EndPhase);
    assert_eq!(s.active, 1);
}
