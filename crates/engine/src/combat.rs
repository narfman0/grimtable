//! Shooting, charging and fighting on top of the attack sequence.

use crate::actions::{RuleError, RuleResult};
use crate::attack::{self, Situation, Target, Weapon, WeaponAbility};
use crate::content::{CoreRule, WeaponKind};
use crate::dice::{Dice, DiceExpr};
use crate::effects::{AttackType, Buffs};
use crate::geom::{base_distance, Vec2};
use crate::rules::{self, unit_buffs};
use crate::state::{GameState, MoveKind, Pending, Phase, PlayerId, UnitId};

fn err<T>(msg: impl Into<String>) -> Result<T, RuleError> {
    Err(RuleError(msg.into()))
}

/// A weapon profile after stat modifiers.
fn modified_weapon(base: &Weapon, buffs: &Buffs) -> Weapon {
    let mut w = base.clone();
    w.attacks.add += buffs.stat("A");
    w.strength = (w.strength as i32 + buffs.stat("S")).max(1) as u32;
    w.ap = (w.ap as i32 + buffs.stat("AP")).max(0) as u32;
    w.damage.add += buffs.stat("D");
    if let Some(s) = w.skill {
        let delta = buffs.stat("BS") + buffs.stat("WS");
        w.skill = Some((s as i32 - delta).clamp(2, 6) as u8);
    }
    w
}

/// Build the abstract target the attack sequence works on. Leaders are not
/// allocated wounds while bodyguards live, so they are left out until then.
fn abstract_target(state: &GameState, target: UnitId, target_buffs: &Buffs) -> Target {
    let t = &state.units[target];
    let bodyguards: Vec<_> = t.alive_models().filter(|m| !m.is_leader).collect();
    let pool: Vec<_> = if bodyguards.is_empty() { t.alive_models().collect() } else { bodyguards };
    let Some(&first) = pool.first() else {
        return Target { toughness: 1, save: 7, invuln: None, feel_no_pain: None, wounds_per_model: 1, models: 0, damaged_model_wounds: None, keywords: vec![], in_cover: false };
    };
    let stats = first.stats_override.as_ref().unwrap_or(&t.spec.stats);
    let damaged = pool.iter().find(|m| m.wounds < m.max_wounds).map(|m| m.wounds);
    let invuln = match (stats.invuln, target_buffs.invuln) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    };
    Target {
        toughness: (stats.toughness as i32 + target_buffs.stat("T")).max(1) as u32,
        save: (stats.save as i32 - target_buffs.stat("Sv")).clamp(2, 7) as u8,
        invuln,
        feel_no_pain: target_buffs.feel_no_pain,
        wounds_per_model: first.max_wounds,
        models: pool.len() as u32,
        damaged_model_wounds: damaged,
        keywords: t.spec.keywords.clone(),
        in_cover: rules::target_in_cover(state, t, target_buffs),
    }
}

/// Write the abstract result back onto real models: remove `destroyed`
/// models nearest to `from`, then set the wounds of the damaged model.
fn apply_casualties(state: &mut GameState, target: UnitId, from: Vec2, destroyed: u32, damaged: Option<u32>) {
    let t = &mut state.units[target];
    let mut order: Vec<usize> = t.models.iter().enumerate().filter(|(_, m)| m.alive() && !m.is_leader).map(|(i, _)| i).collect();
    if order.is_empty() {
        order = t.models.iter().enumerate().filter(|(_, m)| m.alive()).map(|(i, _)| i).collect();
    }
    // Wounded models die first, then the closest.
    order.sort_by(|&a, &b| {
        let (ma, mb) = (&t.models[a], &t.models[b]);
        let wounded = (mb.wounds < mb.max_wounds).cmp(&(ma.wounds < ma.max_wounds));
        wounded.then(
            ma.pos.map(|p| p.dist(from)).unwrap_or(f32::MAX).partial_cmp(&mb.pos.map(|p| p.dist(from)).unwrap_or(f32::MAX)).unwrap(),
        )
    });
    for &i in order.iter().take(destroyed as usize) {
        t.models[i].wounds = 0;
    }
    if let Some(w) = damaged {
        if let Some(&i) = order.get(destroyed as usize) {
            t.models[i].wounds = w;
        }
    }
}

/// After damage: if the unit just died, Deadly Demise may go off.
fn check_destroyed(state: &mut GameState, target: UnitId) {
    let (name, owner, demise) = {
        let t = &state.units[target];
        if t.model_count() > 0 {
            return;
        }
        let demise = t.spec.core_rules.iter().find_map(|r| if let CoreRule::DeadlyDemise(d) = r { Some(d.clone()) } else { None });
        (t.spec.name.clone(), t.owner, demise)
    };
    state.log(None, format!("{name} destroyed"));
    if let Some(d) = demise {
        let roll = state.rng.d6();
        if roll == 6 {
            let centre = state.units[target].models.iter().find_map(|m| m.pos).unwrap_or_default();
            let dice = DiceExpr::parse(&d).unwrap_or(DiceExpr::flat(1));
            let victims: Vec<UnitId> = state.units.iter().filter(|u| u.id != target && u.on_board()).filter(|u| u.alive_models().any(|m| m.pos.map(|p| p.dist(centre) <= 6.0).unwrap_or(false))).map(|u| u.id).collect();
            for v in victims {
                let n = state.rng.roll(&dice);
                mortal_wounds(state, v, n, centre);
                let vn = state.units[v].spec.name.clone();
                state.log(Some(owner), format!("Deadly Demise: {vn} suffers {n} mortal wounds"));
            }
        } else {
            state.log(Some(owner), format!("Deadly Demise roll {roll}: nothing happens"));
        }
    }
}

fn mortal_wounds(state: &mut GameState, target: UnitId, n: u32, from: Vec2) {
    let buffs = unit_buffs(state, &state.units[target], None, None);
    let mut abstract_t = abstract_target(state, target, &buffs);
    let mut remaining = n;
    let mut destroyed = 0;
    while remaining > 0 && abstract_t.models > 0 {
        if let Some(f) = abstract_t.feel_no_pain {
            if state.rng.d6() >= f {
                remaining -= 1;
                continue;
            }
        }
        let left = abstract_t.damaged_model_wounds.unwrap_or(abstract_t.wounds_per_model);
        if left <= 1 {
            abstract_t.models -= 1;
            abstract_t.damaged_model_wounds = None;
            destroyed += 1;
        } else {
            abstract_t.damaged_model_wounds = Some(left - 1);
        }
        remaining -= 1;
    }
    apply_casualties(state, target, from, destroyed, abstract_t.damaged_model_wounds);
    check_destroyed(state, target);
}

// --------------------------------------------------------------- shooting

pub fn shoot(state: &mut GameState, player: PlayerId, unit: UnitId, assignments: Vec<(usize, UnitId)>) -> RuleResult {
    if state.phase != Phase::Shooting {
        return err("not the shooting phase");
    }
    let u = state.unit(unit).ok_or(RuleError("no such unit".into()))?;
    if u.owner != player || !u.on_board() {
        return err("not your unit");
    }
    if u.flags.shot {
        return err("that unit has already shot");
    }
    if u.flags.moved == Some(MoveKind::FallBack) {
        return err("a unit that fell back cannot shoot");
    }
    if assignments.is_empty() {
        return err("choose at least one weapon and target");
    }
    let advanced = u.flags.moved == Some(MoveKind::Advance);
    let engaged: Vec<UnitId> = rules::enemies_in_engagement_range(state, unit).map(|e| e.id).collect();
    let shooter_name = u.spec.name.clone();

    // Validate every assignment before rolling anything.
    struct Plan {
        weapon: usize,
        target: UnitId,
        firing: Vec<(Vec2, bool)>, // model position, half range
    }
    let mut plans = Vec::new();
    let mut groups_used: Vec<String> = Vec::new();
    for (wi, target) in assignments {
        let u = &state.units[unit];
        let spec = u.spec.weapons.get(wi).ok_or(RuleError("no such weapon".into()))?;
        if spec.kind != WeaponKind::Ranged {
            return err(format!("{} is not a ranged weapon", spec.profile.name));
        }
        if let Some(g) = &spec.profile_group {
            if groups_used.contains(g) {
                return err(format!("{} can only fire one of its profiles", g));
            }
            groups_used.push(g.clone());
        }
        let pistol = spec.profile.has(|a| matches!(a, WeaponAbility::Pistol));
        let assault = spec.profile.has(|a| matches!(a, WeaponAbility::Assault));
        if advanced && !assault {
            return err(format!("{} advanced; only Assault weapons can shoot", shooter_name));
        }
        if !engaged.is_empty() && !pistol {
            return err(format!("{} is in engagement range; only Pistols can shoot", shooter_name));
        }
        let t = state.unit(target).ok_or(RuleError("no such target".into()))?;
        if t.owner == player || !t.on_board() {
            return err("target must be an enemy unit on the battlefield");
        }
        if !engaged.is_empty() && !engaged.contains(&target) {
            return err("a unit in engagement range can only shoot the units it is engaged with");
        }
        if engaged.is_empty() {
            let friendly_engaged = state.friends_of(player).any(|f| rules::unit_distance(f, t).map(|d| d <= state.params.engagement_range).unwrap_or(false));
            if friendly_engaged && !pistol {
                return err(format!("{} is in engagement range of your units; only Pistols can target it", t.spec.name));
            }
        }
        let t_buffs = unit_buffs(state, t, None, None);
        let lone = t_buffs.lone_operative && !t.is_attached();
        let range = spec.range.unwrap_or(0) as f32;
        let mut firing = Vec::new();
        for m in u.alive_models().filter(|m| m.weapons.contains(&wi)) {
            let Some(p) = m.pos else { continue };
            let nearest = t.alive_models().filter_map(|tm| tm.pos.map(|q| base_distance(p, m.radius(), q, tm.radius()))).fold(f32::INFINITY, f32::min);
            if nearest > range {
                continue;
            }
            if lone && nearest > state.params.lone_operative_range {
                continue;
            }
            if !rules::can_see_unit(state, p, t) {
                continue;
            }
            firing.push((p, nearest <= range / 2.0));
        }
        if firing.is_empty() {
            return err(format!("no model with {} is in range and sight of {}", spec.profile.name, t.spec.name));
        }
        plans.push(Plan { weapon: wi, target, firing });
    }

    // Resolve.
    let mut hazardous_checks: Vec<(usize, u32)> = Vec::new();
    for plan in plans {
        let shooter = &state.units[unit];
        let target = &state.units[plan.target];
        let s_buffs = unit_buffs(state, shooter, Some(target), Some(AttackType::Ranged));
        let t_buffs = unit_buffs(state, target, None, None);
        let spec = shooter.spec.weapons[plan.weapon].clone();
        let weapon = modified_weapon(&spec.profile, &s_buffs);
        let heavy_ok = matches!(shooter.flags.moved, None | Some(MoveKind::Stationary)) && !shooter.flags.arrived_this_turn;
        let mut abstract_t = abstract_target(state, plan.target, &t_buffs);
        if t_buffs.damage_reduction > 0 {
            // Approximation: flat reduction on the damage expression, never below 1.
            let _ = t_buffs.damage_reduction;
        }
        let mut total = attack::Outcome::default();
        let mut lines = Vec::new();
        let n = plan.firing.len();
        for &(p, half) in &plan.firing {
            if abstract_t.models == 0 {
                break;
            }
            let sit = Situation {
                half_range: half,
                heavy_eligible: heavy_ok,
                hit_modifier: s_buffs.hit_modifier,
                wound_modifier: s_buffs.wound_modifier,
                reroll_hits: s_buffs.reroll_hits,
                reroll_wounds: s_buffs.reroll_wounds,
                modifier_cap: state.params.modifier_cap,
                ..Default::default()
            };
            let o = attack::resolve(&weapon, &mut abstract_t, &sit, &mut state.rng);
            total.attacks += o.attacks;
            total.hits += o.hits;
            total.wounds += o.wounds;
            total.saves_failed += o.saves_failed;
            total.mortal_wounds += o.mortal_wounds;
            total.damage_inflicted += o.damage_inflicted;
            total.models_destroyed += o.models_destroyed;
            lines.extend(o.log);
            let _ = p;
        }
        let from = plan.firing[0].0;
        let tname = state.units[plan.target].spec.name.clone();
        let wname = weapon.name.clone();
        apply_casualties(state, plan.target, from, total.models_destroyed, abstract_t.damaged_model_wounds);
        state.log(
            Some(player),
            format!(
                "{shooter_name}: {n}x {wname} at {tname}: {} attacks, {} hits, {} wounds, {} unsaved, {} damage, {} destroyed",
                total.attacks, total.hits, total.wounds, total.saves_failed, total.damage_inflicted + total.mortal_wounds, total.models_destroyed
            ),
        );
        for l in lines {
            state.log(Some(player), format!("  {l}"));
        }
        if spec.profile.has(|a| matches!(a, WeaponAbility::Hazardous)) {
            hazardous_checks.push((plan.weapon, n as u32));
        }
        check_destroyed(state, plan.target);
    }

    // Hazardous: one roll per hazardous weapon fired.
    for (wi, count) in hazardous_checks {
        for _ in 0..count {
            let roll = state.rng.d6();
            if roll <= state.params.hazardous_fail_on {
                let u = &mut state.units[unit];
                let Some(idx) = u.models.iter().position(|m| m.alive() && m.weapons.contains(&wi)) else { continue };
                let big = u.has_keyword("Character") || u.has_keyword("Vehicle") || u.has_keyword("Monster");
                let m = &mut u.models[idx];
                if big {
                    m.wounds = m.wounds.saturating_sub(3);
                } else {
                    m.wounds = 0;
                }
                let mname = m.name.clone();
                state.log(Some(player), format!("Hazardous: rolled {roll}, {mname} {}", if big { "suffers 3 mortal wounds" } else { "is destroyed" }));
                check_destroyed(state, unit);
            }
        }
    }
    state.units[unit].flags.shot = true;
    Ok(())
}

// ---------------------------------------------------------------- charge

pub fn declare_charge(state: &mut GameState, player: PlayerId, unit: UnitId, targets: Vec<UnitId>) -> RuleResult {
    if state.phase != Phase::Charge {
        return err("not the charge phase");
    }
    let u = state.unit(unit).ok_or(RuleError("no such unit".into()))?;
    if u.owner != player || !u.on_board() {
        return err("not your unit");
    }
    if u.flags.charged || u.charge_targets.len() > 0 {
        return err("that unit has already charged");
    }
    if matches!(u.flags.moved, Some(MoveKind::Advance) | Some(MoveKind::FallBack)) {
        return err("a unit that advanced or fell back cannot charge");
    }
    if rules::enemies_in_engagement_range(state, unit).next().is_some() {
        return err("a unit in engagement range cannot charge");
    }
    if targets.is_empty() {
        return err("declare at least one target");
    }
    let mut farthest: f32 = 0.0;
    for &t in &targets {
        let tu = state.unit(t).ok_or(RuleError("no such target".into()))?;
        if tu.owner == player || !tu.on_board() {
            return err("charge targets must be enemy units on the battlefield");
        }
        let d = rules::unit_distance(u, tu).unwrap_or(f32::INFINITY);
        if d > state.params.charge_range {
            return err(format!("{} is more than {}\" away", tu.spec.name, state.params.charge_range));
        }
        farthest = farthest.max(d);
    }
    let name = u.spec.name.clone();
    let names: Vec<String> = targets.iter().map(|&t| state.units[t].spec.name.clone()).collect();
    let roll = state.rng.roll(&DiceExpr::d6(2));
    if (roll as f32) < farthest - state.params.engagement_range {
        state.log(Some(player), format!("{name} charged {}: rolled {roll}, needed about {:.0}\", failed", names.join(" and "), farthest - state.params.engagement_range));
        state.units[unit].flags.charged = false;
        state.units[unit].charge_targets = vec![UnitId::MAX];
        return Ok(());
    }
    state.log(Some(player), format!("{name} charged {}: rolled {roll}", names.join(" and ")));
    state.units[unit].charge_targets = targets;
    state.pending = Some(Pending::ChargeMove { unit, roll });
    Ok(())
}

pub fn charge_move(state: &mut GameState, player: PlayerId, unit: UnitId, positions: Vec<Vec2>) -> RuleResult {
    let Some(Pending::ChargeMove { unit: pu, roll }) = state.pending else {
        return err("no charge to complete");
    };
    if pu != unit || state.units[unit].owner != player {
        return err("that unit is not charging");
    }
    let placed = crate::actions::validate_positions_pub(state, unit, &positions)?;
    crate::actions::check_move_distance_pub(state, unit, &placed, roll as f32)?;
    let er = state.params.engagement_range;
    let targets = state.units[unit].charge_targets.clone();
    let within = |state: &GameState, t: UnitId| -> f32 {
        let tu = &state.units[t];
        let mut best = f32::INFINITY;
        for m in tu.alive_models() {
            if let Some(q) = m.pos {
                for &(p, r) in &placed {
                    best = best.min(base_distance(p, r, q, m.radius()));
                }
            }
        }
        best
    };
    for &t in &targets {
        if within(state, t) > er {
            return err(format!("the charge must end within engagement range of {}", state.units[t].spec.name));
        }
    }
    for e in state.enemies_of(player).map(|e| e.id).collect::<Vec<_>>() {
        if !targets.contains(&e) && within(state, e) <= er {
            return err(format!("the charge cannot end within engagement range of {}, which was not a target", state.units[e].spec.name));
        }
    }
    crate::actions::set_positions_pub(state, unit, &placed);
    state.units[unit].flags.charged = true;
    state.pending = None;
    let name = state.units[unit].spec.name.clone();
    state.log(Some(player), format!("{name} completed its charge"));
    Ok(())
}

// ----------------------------------------------------------------- fight

fn eligible_fighters(state: &GameState) -> Vec<UnitId> {
    state
        .units
        .iter()
        .filter(|u| u.on_board() && !u.flags.fought)
        .filter(|u| rules::enemies_in_engagement_range(state, u.id).next().is_some())
        .map(|u| u.id)
        .collect()
}

/// Which unit fights next, if any. Chargers and Fights First units go first
/// (active player before opponent), then the players alternate starting with
/// the player whose turn it is not.
pub fn next_fighter(state: &GameState) -> Option<UnitId> {
    let eligible = eligible_fighters(state);
    if eligible.is_empty() {
        return None;
    }
    let first_wave: Vec<UnitId> = eligible
        .iter()
        .copied()
        .filter(|&id| {
            let u = &state.units[id];
            (u.flags.charged && u.owner == state.active) || unit_buffs(state, u, None, Some(AttackType::Melee)).fights_first
        })
        .collect();
    if !first_wave.is_empty() {
        let active_first = first_wave.iter().copied().find(|&id| state.units[id].owner == state.active);
        return active_first.or(first_wave.first().copied());
    }
    let last_owner = state.fight_queue.last().map(|&id| state.units[id].owner);
    let preferred = match last_owner {
        Some(o) => 1 - o,
        None => 1 - state.active,
    };
    eligible.iter().copied().find(|&id| state.units[id].owner == preferred).or(eligible.first().copied())
}

pub fn fight_owner_to_act(state: &GameState) -> Option<PlayerId> {
    next_fighter(state).map(|id| state.units[id].owner)
}

fn validate_short_move(state: &GameState, unit: UnitId, positions: &[Vec2], max: f32, what: &str) -> Result<Vec<(Vec2, f32)>, RuleError> {
    let placed = crate::actions::validate_positions_pub(state, unit, positions)?;
    crate::actions::check_move_distance_pub(state, unit, &placed, max).map_err(|e| RuleError(format!("{what}: {e}")))?;
    // Each model must end no farther from the nearest enemy than it started.
    let u = &state.units[unit];
    let enemies: Vec<(Vec2, f32)> = state.enemies_of(u.owner).flat_map(|e| e.alive_models().filter_map(|m| m.pos.map(|p| (p, m.radius()))).collect::<Vec<_>>()).collect();
    for (m, &(p, r)) in u.alive_models().zip(&placed) {
        let before = m.pos.map(|q| enemies.iter().map(|&(e, re)| base_distance(q, r, e, re)).fold(f32::INFINITY, f32::min)).unwrap_or(0.0);
        let after = enemies.iter().map(|&(e, re)| base_distance(p, r, e, re)).fold(f32::INFINITY, f32::min);
        if after > before + 1e-3 {
            return err(format!("{what}: {} must end as close or closer to the nearest enemy", m.name));
        }
    }
    Ok(placed)
}

pub fn fight(state: &mut GameState, player: PlayerId, unit: UnitId, target: UnitId, pile_in: Option<Vec<Vec2>>, consolidate: Option<Vec<Vec2>>) -> RuleResult {
    if state.phase != Phase::Fight {
        return err("not the fight phase");
    }
    let Some(next) = next_fighter(state) else { return err("no unit can fight") };
    let required_owner = state.units[next].owner;
    if player != required_owner {
        return err(format!("{} chooses the next unit to fight", state.players[required_owner].name));
    }
    let u = state.unit(unit).ok_or(RuleError("no such unit".into()))?;
    if u.owner != player || !eligible_fighters(state).contains(&unit) {
        return err("that unit cannot fight now");
    }
    // Chargers and Fights First go before anything else.
    let wave_first = {
        let n = &state.units[next];
        (n.flags.charged && n.owner == state.active) || unit_buffs(state, n, None, Some(AttackType::Melee)).fights_first
    };
    if wave_first {
        let mine_first = (u.flags.charged && u.owner == state.active) || unit_buffs(state, u, None, Some(AttackType::Melee)).fights_first;
        if !mine_first {
            return err(format!("{} must fight first", state.units[next].spec.name));
        }
    }
    let t = state.unit(target).ok_or(RuleError("no such target".into()))?;
    if t.owner == player || !t.on_board() {
        return err("target must be an enemy unit");
    }
    let name = u.spec.name.clone();
    let tname = t.spec.name.clone();

    if let Some(p) = pile_in {
        let placed = validate_short_move(state, unit, &p, state.params.pile_in, "pile in")?;
        crate::actions::set_positions_pub(state, unit, &placed);
    }
    let er = state.params.engagement_range;
    if rules::unit_distance(&state.units[unit], &state.units[target]).map(|d| d > er).unwrap_or(true) {
        return err(format!("{tname} is not within engagement range after piling in"));
    }

    // Which models can fight: within engagement range of the target, or within
    // half an inch of a friendly model that is.
    let (fighters, from) = {
        let u = &state.units[unit];
        let t = &state.units[target];
        let in_range: Vec<usize> = u
            .alive_models()
            .filter(|m| m.pos.map(|p| t.alive_models().any(|tm| tm.pos.map(|q| base_distance(p, m.radius(), q, tm.radius()) <= er).unwrap_or(false))).unwrap_or(false))
            .map(|m| m.id)
            .collect();
        let supported: Vec<usize> = u
            .alive_models()
            .filter(|m| !in_range.contains(&m.id))
            .filter(|m| {
                m.pos
                    .map(|p| in_range.iter().any(|&i| u.models[i].pos.map(|q| base_distance(p, m.radius(), q, u.models[i].radius()) <= 0.5).unwrap_or(false)))
                    .unwrap_or(false)
            })
            .map(|m| m.id)
            .collect();
        let mut all = in_range;
        all.extend(supported);
        let from = u.models[all[0]].pos.unwrap_or_default();
        (all, from)
    };

    // Group fighters by the weapons they swing: the first melee weapon, plus
    // every Extra Attacks weapon.
    let mut groups: Vec<(usize, u32)> = Vec::new();
    {
        let u = &state.units[unit];
        for &mi in &fighters {
            let m = &u.models[mi];
            let melee: Vec<usize> = m.weapons.iter().copied().filter(|&w| u.spec.weapons[w].kind == WeaponKind::Melee).collect();
            let mut chosen: Vec<usize> = melee.iter().copied().filter(|&w| u.spec.weapons[w].profile.has(|a| matches!(a, WeaponAbility::ExtraAttacks))).collect();
            if let Some(&main) = melee.iter().find(|&&w| !u.spec.weapons[w].profile.has(|a| matches!(a, WeaponAbility::ExtraAttacks))) {
                chosen.push(main);
            }
            for w in chosen {
                match groups.iter_mut().find(|(gw, _)| *gw == w) {
                    Some(g) => g.1 += 1,
                    None => groups.push((w, 1)),
                }
            }
        }
    }
    if groups.is_empty() {
        return err(format!("{name} has no model able to fight {tname}"));
    }

    let s_buffs = unit_buffs(state, &state.units[unit], Some(&state.units[target]), Some(AttackType::Melee));
    let t_buffs = unit_buffs(state, &state.units[target], None, None);
    let mut abstract_t = abstract_target(state, target, &t_buffs);
    let charged = state.units[unit].flags.charged;
    for (wi, count) in groups {
        let weapon = modified_weapon(&state.units[unit].spec.weapons[wi].profile, &s_buffs);
        let lance = charged && weapon.has(|a| matches!(a, WeaponAbility::Lance));
        let mut total = attack::Outcome::default();
        let mut lines = Vec::new();
        for _ in 0..count {
            if abstract_t.models == 0 {
                break;
            }
            let sit = Situation {
                hit_modifier: s_buffs.hit_modifier,
                wound_modifier: s_buffs.wound_modifier + if lance { 1 } else { 0 },
                reroll_hits: s_buffs.reroll_hits,
                reroll_wounds: s_buffs.reroll_wounds,
                modifier_cap: state.params.modifier_cap,
                ..Default::default()
            };
            let o = attack::resolve(&weapon, &mut abstract_t, &sit, &mut state.rng);
            total.attacks += o.attacks;
            total.hits += o.hits;
            total.wounds += o.wounds;
            total.saves_failed += o.saves_failed;
            total.mortal_wounds += o.mortal_wounds;
            total.damage_inflicted += o.damage_inflicted;
            total.models_destroyed += o.models_destroyed;
            lines.extend(o.log);
        }
        state.log(
            Some(player),
            format!(
                "{name}: {count}x {} at {tname}: {} attacks, {} hits, {} wounds, {} unsaved, {} damage, {} destroyed",
                weapon.name, total.attacks, total.hits, total.wounds, total.saves_failed, total.damage_inflicted + total.mortal_wounds, total.models_destroyed
            ),
        );
        for l in lines {
            state.log(Some(player), format!("  {l}"));
        }
        apply_casualties(state, target, from, total.models_destroyed, abstract_t.damaged_model_wounds);
        // Re-derive the pool so the next weapon group allocates correctly.
        abstract_t = abstract_target(state, target, &t_buffs);
        if abstract_t.models == 0 {
            break;
        }
    }
    check_destroyed(state, target);

    if let Some(c) = consolidate {
        if state.units[unit].model_count() > 0 {
            let placed = validate_short_move(state, unit, &c, state.params.consolidate, "consolidate")?;
            crate::actions::set_positions_pub(state, unit, &placed);
        }
    }
    state.units[unit].flags.fought = true;
    state.fight_queue.push(unit);
    Ok(())
}
