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
    pack::build_pack(&army, &abilities)
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
