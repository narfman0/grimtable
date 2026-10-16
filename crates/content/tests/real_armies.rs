//! The user's real lists through a scripted battle. Deliberately dumb
//! play: deploy in grids, walk at the enemy, shoot everything, charge when
//! close, fight whatever is in reach. The point is that the real data never
//! trips the engine.

use content::{enrich, mission, pack};
use engine::actions::{apply, Action};
use engine::geom::Vec2;
use engine::state::{GameState, MoveKind, Phase};
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

fn load(name: &str) -> engine::content::ArmyPack {
    let root = root();
    let army = roster::parse_ros_file(root.join("crates/roster/fixtures").join(format!("{name}.ros"))).unwrap();
    let slug = enrich::faction_slug(&army.faction).unwrap();
    let abilities = enrich::load_abilities(&root, slug).unwrap();
    let detachments = std::fs::read_to_string(root.join("data/40kdc-data/data/core").join(slug).join("detachments.json")).ok();
    pack::build_pack_with_detachments(&army, &abilities, detachments.as_deref())
}

fn ok(state: &mut GameState, player: usize, action: Action) {
    if let Err(e) = apply(state, player, action.clone()) {
        let tail: Vec<String> = state.log.iter().rev().take(15).map(|e| e.text.clone()).collect();
        panic!("{action:?} failed: {e}\nrecent log:\n{}", tail.join("\n"));
    }
}

/// Grid positions for `n` models of radius `r` starting at (x, y), rows of 5,
/// spaced so coherency holds and bases do not touch.
fn grid(x: f32, y: f32, n: usize, r: f32) -> Vec<Vec2> {
    let step = (2.0 * r + 0.4).max(1.2);
    (0..n).map(|i| Vec2::new(x + (i % 5) as f32 * step, y + (i / 5) as f32 * step)).collect()
}

fn deploy_all(s: &mut GameState) {
    // Scan each zone's bounding box and let the engine say whether a spot is
    // free; the first accepted placement wins. Works for any pattern.
    let bounds = |poly: &engine::geom::Polygon| {
        let xs = poly.points.iter().map(|p| p.x);
        let ys = poly.points.iter().map(|p| p.y);
        (xs.clone().fold(f32::MAX, f32::min), ys.clone().fold(f32::MAX, f32::min), xs.fold(f32::MIN, f32::max), ys.fold(f32::MIN, f32::max))
    };
    while s.phase == Phase::Deployment {
        let p = s.active;
        let id = s.units.iter().find(|u| u.owner == p && !u.deployed && u.merged_into.is_none()).map(|u| u.id).expect("unit to deploy");
        let (x0, y0, x1, y1) = bounds(&s.mission.deployment_zones[p]);
        let r = s.units[id].alive_models().map(|m| m.radius()).fold(0.0, f32::max);
        let n = s.units[id].model_count() as usize;
        let mut placed = false;
        let mut cy = y0 + r + 0.2;
        'scan: while cy < y1 {
            let mut cx = x0 + r + 0.2;
            while cx < x1 {
                let pos = grid(cx, cy, n, r);
                if apply(s, p, Action::Deploy { unit: id, positions: pos }).is_ok() {
                    placed = true;
                    break 'scan;
                }
                cx += 1.0;
            }
            cy += 1.0;
        }
        assert!(placed, "no room to deploy {}", s.units[id].spec.name);
    }
}

fn step_toward(s: &GameState, id: usize, dist: f32) -> Vec<Vec2> {
    let u = &s.units[id];
    let centre = u.centre().unwrap();
    let target = s.enemies_of(u.owner).filter_map(|e| e.centre()).min_by(|a, b| a.dist(centre).partial_cmp(&b.dist(centre)).unwrap()).unwrap_or(Vec2::new(30.0, 22.0));
    let d = centre.dist(target).max(0.01);
    let step = dist.min((d - 3.0).max(0.0));
    let dx = (target.x - centre.x) / d * step;
    let dy = (target.y - centre.y) / d * step;
    u.alive_models().map(|m| m.pos.unwrap()).map(|p| Vec2::new((p.x + dx).clamp(1.0, 59.0), (p.y + dy).clamp(1.0, 43.0))).collect()
}

fn play(mut s: GameState) -> GameState {
    ok(&mut s, 0, Action::FinishMuster);
    ok(&mut s, 1, Action::FinishMuster);
    deploy_all(&mut s);
    let p = s.active;
    ok(&mut s, p, Action::EndPhase);
    let p = s.active;
    ok(&mut s, p, Action::EndPhase);
    assert_eq!(s.phase, Phase::Command);
    let mut guard = 0;
    while s.phase != Phase::Ended {
        guard += 1;
        assert!(guard < 200, "stuck");
        let p = s.active;
        ok(&mut s, p, Action::EndPhase); // movement
        let ids: Vec<usize> = s.units.iter().filter(|u| u.owner == p && u.on_board()).map(|u| u.id).collect();
        for id in ids {
            let m = s.units[id].spec.stats.movement as f32;
            let pos = step_toward(&s, id, m);
            if apply(&mut s, p, Action::Move { unit: id, kind: MoveKind::Normal, positions: pos }).is_err() {
                ok(&mut s, p, Action::Move { unit: id, kind: MoveKind::Stationary, positions: vec![] });
            }
        }
        ok(&mut s, p, Action::EndPhase); // shooting
        let ids: Vec<usize> = s.units.iter().filter(|u| u.owner == p && u.on_board()).map(|u| u.id).collect();
        for id in ids {
            let enemies: Vec<usize> = s.units.iter().filter(|u| u.owner != p && u.on_board()).map(|u| u.id).collect();
            let ranged: Vec<usize> = s.units[id].spec.weapons.iter().enumerate().filter(|(_, w)| w.kind == engine::content::WeaponKind::Ranged).map(|(i, _)| i).collect();
            'outer: for &e in &enemies {
                for &w in &ranged {
                    if apply(&mut s, p, Action::Shoot { unit: id, assignments: vec![(w, e)] }).is_ok() {
                        break 'outer;
                    }
                }
            }
        }
        ok(&mut s, p, Action::EndPhase); // charge
        let ids: Vec<usize> = s.units.iter().filter(|u| u.owner == p && u.on_board()).map(|u| u.id).collect();
        for id in ids {
            let enemies: Vec<usize> = s.units.iter().filter(|u| u.owner != p && u.on_board()).map(|u| u.id).collect();
            for &e in &enemies {
                if apply(&mut s, p, Action::DeclareCharge { unit: id, targets: vec![e] }).is_ok() {
                    if s.pending.is_some() {
                        // Try to reach: step straight at the target up to the roll.
                        let roll = match s.pending { Some(engine::state::Pending::ChargeMove { roll, .. }) => roll, _ => 0 } as f32;
                        let pos = step_toward(&s, id, roll);
                        if apply(&mut s, p, Action::ChargeMove { unit: id, positions: pos }).is_err() {
                            // Could not make it work; clear the pending charge by moving nowhere.
                            let stay: Vec<Vec2> = s.units[id].alive_models().map(|m| m.pos.unwrap()).collect();
                            if apply(&mut s, p, Action::ChargeMove { unit: id, positions: stay }).is_err() {
                                s.pending = None; // test-only escape hatch
                                s.units[id].charge_targets.clear();
                            }
                        }
                    }
                    break;
                }
            }
        }
        ok(&mut s, p, Action::EndPhase); // fight
        while let Some(next) = engine::combat::next_fighter(&s) {
            let owner = s.units[next].owner;
            let target = engine::rules::enemies_in_engagement_range(&s, next).next().unwrap().id;
            ok(&mut s, owner, Action::Fight { unit: next, target, pile_in: None, consolidate: None });
        }
        ok(&mut s, p, Action::EndPhase);
    }
    s
}

#[test]
fn packs_build_for_all_fixtures() {
    for name in ["mechanicus", "custodes", "necrons"] {
        let p = load(name);
        assert!(p.units.len() >= 12, "{name}");
        for u in &p.units {
            assert!(!u.models.is_empty(), "{} has no models", u.name);
            assert!(u.stats.base_mm > 0, "{} has no base", u.name);
            for m in &u.models {
                for &w in &m.weapons {
                    assert!(w < u.weapons.len(), "{}: weapon index out of range", u.name);
                }
            }
        }
    }
}

#[test]
fn mechanicus_versus_necrons_runs_to_the_end() {
    let mech = load("mechanicus");
    let necrons = load("necrons");
    let mission = mission::default_mission(&root(), "dawn-of-war").unwrap();
    let mut s = GameState::new([mech, necrons], mission, engine::content::EditionParams::default(), 2026);
    // Attach the leaders the way the army notes say.
    let find = |s: &GameState, owner: usize, name: &str, nth: usize| s.units.iter().filter(|u| u.owner == owner && u.spec.name == name && u.merged_into.is_none()).nth(nth).map(|u| u.id).unwrap_or_else(|| panic!("{name}"));
    let dominus = find(&s, 0, "Tech-Priest Dominus", 0);
    let rangers = find(&s, 0, "Skitarii Rangers", 0);
    ok(&mut s, 0, Action::Attach { leader: dominus, bodyguard: rangers });
    let marshal = find(&s, 0, "Skitarii Marshal", 0);
    let vanguard = find(&s, 0, "Skitarii Vanguard", 0);
    ok(&mut s, 0, Action::Attach { leader: marshal, bodyguard: vanguard });
    let datasmith = find(&s, 0, "Cybernetica Datasmith", 0);
    let kastelans = find(&s, 0, "Kastelan Robots", 0);
    ok(&mut s, 0, Action::Attach { leader: datasmith, bodyguard: kastelans });
    let s = play(s);
    assert!(s.winner.is_some());
    let text: Vec<String> = s.log.iter().map(|e| e.text.clone()).collect();
    assert!(text.iter().any(|t| t.contains(" at ") && t.contains("attacks")), "nobody attacked");
    assert!(s.players[0].vp + s.players[1].vp > 0, "nobody scored\n{}", text.join("\n"));
    eprintln!("{}", text.iter().filter(|t| !t.starts_with("  ")).cloned().collect::<Vec<_>>().join("\n"));
}

#[test]
fn custodes_versus_mechanicus_runs_to_the_end() {
    let a = load("custodes");
    let b = load("mechanicus");
    let mission = mission::default_mission(&root(), "hammer-and-anvil").unwrap();
    let s = GameState::new([a, b], mission, engine::content::EditionParams::default(), 99);
    let s = play(s);
    assert!(s.winner.is_some());
}

fn fresh(a: &str, b: &str) -> GameState {
    let mission = mission::default_mission(&root(), "dawn-of-war").unwrap();
    GameState::new([load(a), load(b)], mission, engine::content::EditionParams::default(), 5)
}

#[test]
fn army_rules_offer_stances_and_apply_once_chosen() {
    let mut s = fresh("mechanicus", "necrons");
    let rangers = s.units.iter().find(|u| u.owner == 0 && u.spec.name == "Skitarii Rangers").unwrap().id;
    let before = engine::rules::unit_buffs(&s, &s.units[rangers], None, Some(engine::effects::AttackType::Ranged));
    assert!(before.choices_pending.iter().any(|(name, opts)| name == "Doctrina Imperatives" && opts.len() == 2), "{:?}", before.choices_pending);
    assert_eq!(before.stat("BS"), 0);
    ok(&mut s, 0, Action::SelectStance { name: "Doctrina Imperatives".into(), option: 0 });
    let after = engine::rules::unit_buffs(&s, &s.units[rangers], None, Some(engine::effects::AttackType::Ranged));
    assert!(after.choices_pending.iter().all(|(n, _)| n != "Doctrina Imperatives"));
    assert_eq!(after.stat("BS"), 1, "{after:?}");
    assert!(after.weapon_abilities.iter().any(|(k, _)| k == "Heavy"), "{:?}", after.weapon_abilities);
    assert_eq!(after.enemy_melee_hit_modifier, -1, "Battleline get -1 to be hit in melee");
}

#[test]
fn reanimation_restores_wounds_at_end_of_command_phase() {
    let mut s = fresh("necrons", "mechanicus");
    let warriors = s.units.iter().find(|u| u.owner == 0 && u.spec.name == "Necron Warriors").unwrap().id;
    let buffs = engine::rules::unit_buffs_at(&s, &s.units[warriors], None, None, Some("end-of-command-phase"));
    assert_eq!(buffs.recovery.as_deref(), Some("D3"), "{buffs:?}");
    // Skip straight to a command phase of player 0 with two Warriors dead.
    s.phase = Phase::Command;
    s.round = 1;
    s.active = 0;
    for m in &mut s.units[warriors].models {
        m.pos = Some(Vec2::new(10.0, 10.0));
    }
    s.units[warriors].deployed = true;
    s.units[warriors].models[0].wounds = 0;
    s.units[warriors].models[1].wounds = 0;
    let alive_before = s.units[warriors].model_count();
    ok(&mut s, 0, Action::EndPhase);
    let alive_after = s.units[warriors].model_count();
    assert!(alive_after > alive_before, "{} -> {}", alive_before, alive_after);
    assert!(s.log.iter().any(|e| e.text.contains("recovery")));
}

#[test]
fn katah_stance_grants_lethal_hits_in_the_fight_phase() {
    let mut s = fresh("custodes", "mechanicus");
    let guard = s.units.iter().find(|u| u.owner == 0 && u.spec.name == "Custodian Guard").unwrap().id;
    s.phase = Phase::Fight;
    let pending = engine::rules::unit_buffs(&s, &s.units[guard], None, Some(engine::effects::AttackType::Melee));
    assert!(pending.choices_pending.iter().any(|(n, _)| n == "Ka'tah Stance"), "{:?}", pending.choices_pending);
    s.players[0].stances.insert("Ka'tah Stance".into(), 1);
    let chosen = engine::rules::unit_buffs(&s, &s.units[guard], None, Some(engine::effects::AttackType::Melee));
    assert!(chosen.weapon_abilities.iter().any(|(k, _)| k == "Lethal Hits"), "{:?}", chosen.weapon_abilities);
}

#[test]
fn report_what_the_engine_leaves_to_the_players() {
    for (a, b) in [("mechanicus", "necrons"), ("necrons", "mechanicus"), ("custodes", "mechanicus")] {
        let s = fresh(a, b);
        let mut notes: Vec<String> = Vec::new();
        for u in s.units.iter().filter(|u| u.owner == 0) {
            let bf = engine::rules::unit_buffs(&s, u, None, None);
            notes.extend(bf.unsupported);
        }
        notes.sort();
        notes.dedup();
        eprintln!("{a}: {} abilities left to the players: {}", notes.len(), notes.join(" | "));
    }
}

#[test]
fn detachment_rules_ride_on_the_pack() {
    let mech = load("mechanicus");
    assert!(mech.army_rules.iter().any(|r| r.name == "War-form Mantles"), "{:?}", mech.army_rules.iter().map(|r| &r.name).collect::<Vec<_>>());
    let cust = load("custodes");
    assert!(cust.army_rules.iter().any(|r| r.name == "Martial Mastery"));
    let nec = load("necrons");
    assert!(nec.army_rules.iter().any(|r| r.name == "Command Protocols"));

    let s = fresh("mechanicus", "necrons");
    let rangers = s.units.iter().find(|u| u.owner == 0 && u.spec.name == "Skitarii Rangers").unwrap();
    let b = engine::rules::unit_buffs(&s, rangers, None, None);
    // As the data currently encodes it: invulnerable save and Feel No Pain army-wide.
    assert_eq!((b.invuln, b.feel_no_pain), (Some(4), Some(5)), "{b:?}");

    let s = fresh("custodes", "necrons");
    let guard = s.units.iter().find(|u| u.owner == 0 && u.spec.name == "Custodian Guard").unwrap();
    let b = engine::rules::unit_buffs(&s, guard, None, Some(engine::effects::AttackType::Melee));
    assert!(b.choices_pending.iter().any(|(n, _)| n == "Martial Mastery"), "{:?}", b.choices_pending);
}

#[test]
fn vanguard_ride_the_dunerider() {
    let mut s = fresh("mechanicus", "necrons");
    ok(&mut s, 0, Action::FinishMuster);
    ok(&mut s, 1, Action::FinishMuster);
    s.first_player = 0;
    s.active = 0;
    let vanguard = s.units.iter().find(|u| u.owner == 0 && u.spec.name == "Skitarii Vanguard").unwrap().id;
    let dunerider = s.units.iter().find(|u| u.owner == 0 && u.spec.name == "Skorpius Dunerider").unwrap().id;
    let cawl = s.units.iter().find(|u| u.owner == 0 && u.spec.name == "Belisarius Cawl").unwrap().id;
    assert!(apply(&mut s, 0, Action::Embark { unit: vanguard, transport: cawl }).is_err(), "Cawl is not a transport");
    ok(&mut s, 0, Action::Embark { unit: vanguard, transport: dunerider });
    assert!(!s.units[vanguard].on_board());
    // Necrons deploy something, then the Dunerider goes down.
    let p = s.active;
    let warriors = s.units.iter().find(|u| u.owner == 1 && u.spec.name == "Necron Warriors").unwrap().id;
    let n = s.units[warriors].model_count() as usize;
    let wr = s.units[warriors].models[0].radius();
    ok(&mut s, p, Action::Deploy { unit: warriors, positions: grid(5.0, 36.0, n, wr) });
    ok(&mut s, 0, Action::Deploy { unit: dunerider, positions: vec![Vec2::new(30.0, 6.0)] });
    // Jump to the movement phase of player 0.
    s.phase = Phase::Movement;
    s.round = 1;
    s.active = 0;
    for u in s.units.iter_mut() {
        if !u.deployed && u.merged_into.is_none() {
            u.deployed = true;
            u.in_reserve = true;
        }
    }
    let r = s.units[vanguard].models[0].radius();
    assert!(apply(&mut s, 0, Action::Disembark { unit: vanguard, positions: grid(45.0, 6.0, 10, r) }).is_err(), "too far from the transport");
    // Transport moves first, then the squad hops out: no charge this turn.
    ok(&mut s, 0, Action::Move { unit: dunerider, kind: MoveKind::Normal, positions: vec![Vec2::new(30.0, 14.0)] });
    ok(&mut s, 0, Action::Disembark { unit: vanguard, positions: grid(27.5, 16.6, 10, r) });
    assert!(s.units[vanguard].on_board());
    assert!(s.units[vanguard].disembarked_after_move);
    s.phase = Phase::Charge;
    let e = apply(&mut s, 0, Action::DeclareCharge { unit: vanguard, targets: vec![warriors] }).unwrap_err();
    assert!(e.0.contains("disembarked") || e.0.contains("away"), "{e}");
}
