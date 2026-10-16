//! Player actions and the phase machine. `apply` validates an action against
//! the rules, mutates the state, and logs what happened. Invalid actions
//! return a `RuleError` and leave the state untouched.

use crate::content::{AwardRule, AwardTiming, CoreRule};
use crate::dice::{Dice, DiceExpr};
use crate::geom::Vec2;
use crate::rules::{self, coherent, unit_buffs, update_objective_control};
use crate::state::{GameState, MoveKind, Pending, Phase, PlayerId, TurnFlags, UnitId};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    /// Muster: merge a Leader or Support unit into a bodyguard unit.
    Attach { leader: UnitId, bodyguard: UnitId },
    FinishMuster,
    /// Place every alive model of a unit; positions in model order.
    Deploy { unit: UnitId, positions: Vec<Vec2> },
    /// Hold a unit off the board to arrive later.
    Reserve { unit: UnitId },
    /// Scouts move before the first turn.
    ScoutMove { unit: UnitId, positions: Vec<Vec2> },
    Move { unit: UnitId, kind: MoveKind, positions: Vec<Vec2> },
    ArriveFromReserve { unit: UnitId, positions: Vec<Vec2> },
    /// Each assignment is (weapon index in the unit's spec, target unit).
    Shoot { unit: UnitId, assignments: Vec<(usize, UnitId)> },
    DeclareCharge { unit: UnitId, targets: Vec<UnitId> },
    ChargeMove { unit: UnitId, positions: Vec<Vec2> },
    Fight { unit: UnitId, target: UnitId, pile_in: Option<Vec<Vec2>>, consolidate: Option<Vec<Vec2>> },
    /// Pick an option for an army-wide stance or a choice an ability offers.
    SelectStance { name: String, option: usize },
    /// Deployment: put an undeployed unit inside a transport.
    Embark { unit: UnitId, transport: UnitId },
    /// Movement phase: set up an embarked unit within 3" of its transport.
    Disembark { unit: UnitId, positions: Vec<Vec2> },
    EndPhase,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleError(pub String);

impl fmt::Display for RuleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RuleError {}

pub type RuleResult = Result<(), RuleError>;

fn err<T>(msg: impl Into<String>) -> Result<T, RuleError> {
    Err(RuleError(msg.into()))
}

/// Apply one action for the active player.
pub fn apply(state: &mut GameState, player: PlayerId, action: Action) -> RuleResult {
    if state.phase == Phase::Ended {
        return err("the battle is over");
    }
    let fight_turn = state.phase == Phase::Fight && matches!(action, Action::Fight { .. });
    if player != state.active && !fight_turn && !matches!(action, Action::Attach { .. } | Action::FinishMuster) {
        return err("not your turn");
    }
    if let Some(Pending::ChargeMove { unit, .. }) = state.pending {
        match &action {
            Action::ChargeMove { unit: u, .. } if *u == unit => {}
            _ => return err("finish the charge move first"),
        }
    }
    match action {
        Action::Attach { leader, bodyguard } => attach(state, player, leader, bodyguard),
        Action::FinishMuster => finish_muster(state, player),
        Action::Deploy { unit, positions } => deploy(state, player, unit, positions),
        Action::Reserve { unit } => reserve(state, player, unit),
        Action::ScoutMove { unit, positions } => scout_move(state, player, unit, positions),
        Action::Move { unit, kind, positions } => movement(state, player, unit, kind, positions),
        Action::ArriveFromReserve { unit, positions } => arrive(state, player, unit, positions),
        Action::Shoot { unit, assignments } => crate::combat::shoot(state, player, unit, assignments),
        Action::DeclareCharge { unit, targets } => crate::combat::declare_charge(state, player, unit, targets),
        Action::ChargeMove { unit, positions } => crate::combat::charge_move(state, player, unit, positions),
        Action::Fight { unit, target, pile_in, consolidate } => crate::combat::fight(state, player, unit, target, pile_in, consolidate),
        Action::Embark { unit, transport } => embark(state, player, unit, transport),
        Action::Disembark { unit, positions } => disembark(state, player, unit, positions),
        Action::SelectStance { name, option } => {
            state.players[player].stances.insert(name.clone(), option);
            state.log(Some(player), format!("{} chose option {} for {name}", state.players[player].name, option + 1));
            Ok(())
        }
        Action::EndPhase => end_phase(state),
    }
}

/// End of the Command phase: units with a recovery pool regain wounds, then
/// models, as far as the pool stretches.
fn recovery(state: &mut GameState) {
    let active = state.active;
    let ids: Vec<UnitId> = state.units.iter().filter(|u| u.owner == active && u.on_board()).map(|u| u.id).collect();
    for id in ids {
        let buffs = rules::unit_buffs_at(state, &state.units[id], None, None, Some("end-of-command-phase"));
        let Some(dice) = buffs.recovery else { continue };
        let Ok(expr) = DiceExpr::parse(&dice) else { continue };
        let u = &state.units[id];
        let needs = u.models.iter().any(|m| m.wounds < m.max_wounds);
        if !needs {
            continue;
        }
        let mut pool = state.rng.roll(&expr);
        let rolled = pool;
        let u = &mut state.units[id];
        let mut healed = 0;
        let mut returned = 0;
        // Damaged models first.
        for m in u.models.iter_mut().filter(|m| m.alive() && m.wounds < m.max_wounds) {
            let need = m.max_wounds - m.wounds;
            let give = need.min(pool);
            m.wounds += give;
            pool -= give;
            healed += give;
        }
        // Then destroyed models, each needing its full wounds; they return next
        // to a surviving model.
        let anchor = u.models.iter().find(|m| m.alive()).and_then(|m| m.pos);
        for m in u.models.iter_mut().filter(|m| !m.alive() && !m.is_leader) {
            if pool >= m.max_wounds {
                if let Some(a) = anchor {
                    pool -= m.max_wounds;
                    m.wounds = m.max_wounds;
                    m.pos = Some(Vec2::new(a.x + 0.1 * returned as f32, a.y + 1.2 + 0.3 * returned as f32));
                    returned += 1;
                }
            }
        }
        let name = u.spec.name.clone();
        state.log(Some(active), format!("{name} recovery: rolled {rolled}, healed {healed} wounds, {returned} models returned"));
    }
}

// ---------------------------------------------------------------- muster

fn attach(state: &mut GameState, player: PlayerId, leader: UnitId, bodyguard: UnitId) -> RuleResult {
    if state.phase != Phase::Muster {
        return err("leaders attach during muster");
    }
    let (l, b) = match (state.unit(leader), state.unit(bodyguard)) {
        (Some(l), Some(b)) => (l, b),
        _ => return err("no such unit"),
    };
    if l.owner != player || b.owner != player {
        return err("not your units");
    }
    if l.merged_into.is_some() || b.merged_into.is_some() {
        return err("already attached");
    }
    let kind = if l.spec.core_rules.contains(&CoreRule::Leader) {
        CoreRule::Leader
    } else if l.spec.core_rules.contains(&CoreRule::Support) {
        CoreRule::Support
    } else {
        return err(format!("{} is not a Leader or Support unit", l.spec.name));
    };
    if !l.spec.can_lead.iter().any(|n| n.eq_ignore_ascii_case(&b.spec.name)) {
        return err(format!("{} cannot lead {}", l.spec.name, b.spec.name));
    }
    let already_same_kind = b.attached_leaders.iter().any(|name| {
        state.units.iter().any(|u| &u.spec.name == name && u.merged_into == Some(bodyguard) && u.spec.core_rules.contains(&kind))
    });
    if already_same_kind {
        return err(format!("{} already has a {:?} attached", b.spec.name, kind));
    }
    let leader_name = l.spec.name.clone();
    let mut leader_models = l.models.clone();
    let leader_stats = l.spec.stats.clone();
    let leader_weapons = l.spec.weapons.clone();
    let leader_abilities = l.spec.abilities.clone();
    let leader_rules = l.spec.core_rules.clone();

    let b = &mut state.units[bodyguard];
    let weapon_offset = b.spec.weapons.len();
    b.spec.weapons.extend(leader_weapons);
    for m in &mut leader_models {
        m.id = b.models.len() + m.id;
        m.is_leader = true;
        m.stats_override = Some(leader_stats.clone());
        for w in &mut m.weapons {
            *w += weapon_offset;
        }
    }
    b.models.extend(leader_models);
    b.starting_models = b.models.len() as u32;
    b.attached_leaders.push(leader_name.clone());
    // Leader abilities travel with the leader; the DSL's `is-attached`
    // conditions decide what they grant the unit.
    b.spec.abilities.extend(leader_abilities);
    for r in leader_rules {
        if matches!(r, CoreRule::LoneOperative) {
            continue;
        }
        if !b.spec.core_rules.contains(&r) {
            b.spec.core_rules.push(r);
        }
    }
    state.units[leader].merged_into = Some(bodyguard);
    let bname = state.units[bodyguard].spec.name.clone();
    state.log(Some(player), format!("{leader_name} attached to {bname}"));
    Ok(())
}

fn finish_muster(state: &mut GameState, player: PlayerId) -> RuleResult {
    if state.phase != Phase::Muster {
        return err("not mustering");
    }
    // Reuse `fight_queue` as the muster-done list; it is empty until the fight phase.
    if state.fight_queue.contains(&player) {
        return err("already finished");
    }
    state.fight_queue.push(player);
    if state.fight_queue.len() < 2 {
        return Ok(());
    }
    state.fight_queue.clear();
    // Roll-off for first turn; the winner also deploys first.
    let (a, b) = loop {
        let a = state.rng.d6();
        let b = state.rng.d6();
        if a != b {
            break (a, b);
        }
    };
    state.first_player = if a > b { 0 } else { 1 };
    state.active = state.first_player;
    state.log(None, format!("Roll-off {a} vs {b}: {} deploys first and takes the first turn", state.players[state.first_player].name));
    state.phase = Phase::Deployment;
    Ok(())
}

// ------------------------------------------------------------ deployment

fn undeployed(state: &GameState, player: PlayerId) -> Vec<UnitId> {
    state.units.iter().filter(|u| u.owner == player && !u.deployed && !u.in_reserve && u.merged_into.is_none()).map(|u| u.id).collect()
}

fn next_deployer(state: &mut GameState) {
    let other = 1 - state.active;
    if !undeployed(state, other).is_empty() {
        state.active = other;
    } else if undeployed(state, state.active).is_empty() {
        state.phase = Phase::PreBattle;
        state.active = state.first_player;
        state.log(None, "Deployment complete");
    }
}

/// Validate a full set of model positions for a unit: on the board, no base
/// overlap with anything else, coherent.
pub(crate) fn validate_positions_pub(state: &GameState, unit: UnitId, positions: &[Vec2]) -> Result<Vec<(Vec2, f32)>, RuleError> {
    validate_positions(state, unit, positions)
}

pub(crate) fn check_move_distance_pub(state: &GameState, unit: UnitId, placed: &[(Vec2, f32)], max: f32) -> RuleResult {
    check_move_distance(state, unit, placed, max)
}

pub(crate) fn set_positions_pub(state: &mut GameState, unit: UnitId, placed: &[(Vec2, f32)]) {
    set_positions(state, unit, placed)
}

fn validate_positions(state: &GameState, unit: UnitId, positions: &[Vec2]) -> Result<Vec<(Vec2, f32)>, RuleError> {
    let u = &state.units[unit];
    let alive: Vec<_> = u.alive_models().collect();
    if positions.len() != alive.len() {
        return err(format!("{} needs {} positions, got {}", u.spec.name, alive.len(), positions.len()));
    }
    let placed: Vec<(Vec2, f32)> = alive.iter().zip(positions).map(|(m, &p)| (p, m.radius())).collect();
    for &(p, r) in &placed {
        if p.x - r < 0.0 || p.y - r < 0.0 || p.x + r > state.params.board_w || p.y + r > state.params.board_h {
            return err("model off the board");
        }
    }
    for (i, &(p, r)) in placed.iter().enumerate() {
        for &(q, rq) in &placed[i + 1..] {
            if p.dist(q) < r + rq - 1e-3 {
                return err("models overlap");
            }
        }
    }
    for other in state.units.iter().filter(|o| o.id != unit && o.on_board()) {
        for m in other.alive_models() {
            if let Some(q) = m.pos {
                if placed.iter().any(|&(p, r)| p.dist(q) < r + m.radius() - 1e-3) {
                    return err(format!("overlaps {}", other.spec.name));
                }
            }
        }
    }
    if !coherent(state, &placed) {
        return err("unit is not in coherency");
    }
    Ok(placed)
}

fn min_distance_to_enemies(state: &GameState, player: PlayerId, placed: &[(Vec2, f32)]) -> f32 {
    let mut best = f32::INFINITY;
    for e in state.enemies_of(player) {
        for m in e.alive_models() {
            if let Some(q) = m.pos {
                for &(p, r) in placed {
                    best = best.min(crate::geom::base_distance(p, r, q, m.radius()));
                }
            }
        }
    }
    best
}

fn set_positions(state: &mut GameState, unit: UnitId, placed: &[(Vec2, f32)]) {
    let u = &mut state.units[unit];
    for (m, &(p, _)) in u.models.iter_mut().filter(|m| m.alive()).zip(placed) {
        m.pos = Some(p);
    }
}

fn deploy(state: &mut GameState, player: PlayerId, unit: UnitId, positions: Vec<Vec2>) -> RuleResult {
    if state.phase != Phase::Deployment {
        return err("not deploying");
    }
    let u = state.unit(unit).ok_or(RuleError("no such unit".into()))?;
    if u.owner != player {
        return err("not your unit");
    }
    if u.deployed || u.merged_into.is_some() {
        return err("already deployed");
    }
    let placed = validate_positions(state, unit, &positions)?;
    let infiltrates = u.spec.core_rules.contains(&CoreRule::Infiltrators);
    let zone = &state.mission.deployment_zones[player];
    let enemy_zone = &state.mission.deployment_zones[1 - player];
    if infiltrates {
        if placed.iter().any(|&(p, _)| enemy_zone.contains(p)) {
            return err("infiltrators cannot set up in the enemy deployment zone");
        }
        if min_distance_to_enemies(state, player, &placed) <= state.params.reserve_distance + 1.0 {
            return err("infiltrators must set up more than 9\" from enemy models");
        }
    } else if !placed.iter().all(|&(p, _)| zone.contains(p)) {
        return err("every model must be wholly within your deployment zone");
    }
    set_positions(state, unit, &placed);
    state.units[unit].deployed = true;
    let name = state.units[unit].spec.name.clone();
    state.log(Some(player), format!("{name} deployed"));
    next_deployer(state);
    Ok(())
}

fn reserve(state: &mut GameState, player: PlayerId, unit: UnitId) -> RuleResult {
    if state.phase != Phase::Deployment {
        return err("not deploying");
    }
    let u = state.unit(unit).ok_or(RuleError("no such unit".into()))?;
    if u.owner != player || u.deployed || u.merged_into.is_some() || u.in_reserve {
        return err("cannot reserve that unit");
    }
    state.units[unit].in_reserve = true;
    state.units[unit].deployed = true;
    let name = state.units[unit].spec.name.clone();
    state.log(Some(player), format!("{name} held in reserve"));
    next_deployer(state);
    Ok(())
}

fn scout_move(state: &mut GameState, player: PlayerId, unit: UnitId, positions: Vec<Vec2>) -> RuleResult {
    if state.phase != Phase::PreBattle {
        return err("scout moves happen before the first turn");
    }
    let u = state.unit(unit).ok_or(RuleError("no such unit".into()))?;
    if u.owner != player || !u.on_board() {
        return err("not your unit");
    }
    let Some(dist) = u.spec.core_rules.iter().find_map(|r| if let CoreRule::Scouts(d) = r { Some(*d) } else { None }) else {
        return err("that unit does not have Scouts");
    };
    if u.flags.moved.is_some() {
        return err("already made its scout move");
    }
    let placed = validate_positions(state, unit, &positions)?;
    check_move_distance(state, unit, &placed, dist as f32)?;
    if min_distance_to_enemies(state, player, &placed) <= state.params.reserve_distance + 1.0 {
        return err("scout moves must end more than 9\" from enemy models");
    }
    set_positions(state, unit, &placed);
    state.units[unit].flags.moved = Some(MoveKind::Normal);
    let name = state.units[unit].spec.name.clone();
    state.log(Some(player), format!("{name} made a {dist}\" scout move"));
    Ok(())
}

// ------------------------------------------------------------ transports

fn embark(state: &mut GameState, player: PlayerId, unit: UnitId, transport: UnitId) -> RuleResult {
    if state.phase != Phase::Deployment {
        return err("units embark during deployment");
    }
    let (u, t) = match (state.unit(unit), state.unit(transport)) {
        (Some(u), Some(t)) => (u, t),
        _ => return err("no such unit"),
    };
    if u.owner != player || t.owner != player {
        return err("not your units");
    }
    if u.deployed || u.merged_into.is_some() {
        return err("that unit is already deployed");
    }
    let Some(capacity) = t.spec.transport_capacity else {
        return err(format!("{} is not a transport", t.spec.name));
    };
    if !u.has_keyword("Infantry") {
        return err("only Infantry can embark");
    }
    let aboard: u32 = state.units.iter().filter(|x| x.embarked_in == Some(transport)).map(|x| x.model_count()).sum();
    if aboard + u.model_count() > capacity {
        return err(format!("{} can carry {capacity} models", t.spec.name));
    }
    let (uname, tname) = (u.spec.name.clone(), t.spec.name.clone());
    let u = &mut state.units[unit];
    u.embarked_in = Some(transport);
    u.deployed = true;
    state.log(Some(player), format!("{uname} embarked in {tname}"));
    next_deployer(state);
    Ok(())
}

fn disembark(state: &mut GameState, player: PlayerId, unit: UnitId, positions: Vec<Vec2>) -> RuleResult {
    if state.phase != Phase::Movement {
        return err("units disembark in the movement phase");
    }
    let u = state.unit(unit).ok_or(RuleError("no such unit".into()))?;
    if u.owner != player {
        return err("not your unit");
    }
    let Some(tid) = u.embarked_in else { return err("that unit is not embarked") };
    let t = &state.units[tid];
    if !t.on_board() {
        return err("the transport is not on the battlefield");
    }
    if matches!(t.flags.moved, Some(MoveKind::Advance) | Some(MoveKind::FallBack)) {
        return err("cannot disembark from a transport that advanced or fell back");
    }
    let transport_moved = matches!(t.flags.moved, Some(MoveKind::Normal));
    let hull: Vec<(Vec2, f32)> = t.alive_models().filter_map(|m| m.pos.map(|p| (p, m.radius()))).collect();
    // Temporarily mark the unit as on the board so position checks see it.
    state.units[unit].embarked_in = None;
    let placed = match validate_positions(state, unit, &positions) {
        Ok(p) => p,
        Err(e) => {
            state.units[unit].embarked_in = Some(tid);
            return Err(e);
        }
    };
    let near = placed.iter().all(|&(p, r)| hull.iter().any(|&(h, hr)| crate::geom::base_distance(p, r, h, hr) <= 3.0));
    if !near || min_distance_to_enemies(state, player, &placed) <= state.params.engagement_range {
        state.units[unit].embarked_in = Some(tid);
        return err("disembarking models must be set up within 3\" of the transport and outside engagement range");
    }
    set_positions(state, unit, &placed);
    let u = &mut state.units[unit];
    u.disembarked_after_move = transport_moved;
    // A unit that leaves a stationary transport may still move this phase.
    u.flags.moved = if transport_moved { Some(MoveKind::Normal) } else { None };
    let name = u.spec.name.clone();
    let tname = state.units[tid].spec.name.clone();
    state.log(Some(player), format!("{name} disembarked from {tname}{}", if transport_moved { " after it moved; it cannot charge this turn" } else { "" }));
    Ok(())
}

/// A destroyed transport spills its passengers next to the wreck; each model
/// risks a hazard roll.
pub(crate) fn emergency_disembark(state: &mut GameState, transport: UnitId) {
    let passengers: Vec<UnitId> = state.units.iter().filter(|u| u.embarked_in == Some(transport)).map(|u| u.id).collect();
    if passengers.is_empty() {
        return;
    }
    let centre = state.units[transport].models.iter().find_map(|m| m.pos).unwrap_or_default();
    let fail_on = state.params.desperate_escape_fail_on;
    for (k, pid) in passengers.into_iter().enumerate() {
        let n = state.units[pid].model_count() as usize;
        let mut lost = 0;
        for _ in 0..n {
            if state.rng.d6() <= fail_on {
                lost += 1;
            }
        }
        let u = &mut state.units[pid];
        u.embarked_in = None;
        let r = u.models.iter().map(|m| m.radius()).fold(0.0, f32::max);
        let step = 2.0 * r + 0.3;
        let mut i = 0;
        for m in u.models.iter_mut().filter(|m| m.alive()) {
            let angle = i as f32 * 0.9 + k as f32;
            let ring = 3.5 + step * (i / 7) as f32;
            m.pos = Some(Vec2::new((centre.x + ring * angle.cos()).clamp(1.0, 59.0), (centre.y + ring * angle.sin()).clamp(1.0, 43.0)));
            i += 1;
        }
        for m in u.models.iter_mut().filter(|m| m.alive()).take(lost) {
            m.wounds = 0;
        }
        let name = u.spec.name.clone();
        state.log(None, format!("{name} forced to disembark from the wreck, losing {lost} models"));
    }
}

// -------------------------------------------------------------- movement

fn check_move_distance(state: &GameState, unit: UnitId, placed: &[(Vec2, f32)], max: f32) -> RuleResult {
    let u = &state.units[unit];
    for (m, &(p, _)) in u.alive_models().zip(placed) {
        let from = m.pos.ok_or(RuleError("model has no position".into()))?;
        if from.dist(p) > max + 1e-3 {
            return err(format!("{} moved {:.1}\", limit {max:.1}\"", m.name, from.dist(p)));
        }
    }
    Ok(())
}

fn effective_movement(state: &GameState, unit: UnitId) -> f32 {
    let u = &state.units[unit];
    let buffs = unit_buffs(state, u, None, None);
    (u.spec.stats.movement as i32 + buffs.move_add + buffs.stat("M")).max(0) as f32
}

fn movement(state: &mut GameState, player: PlayerId, unit: UnitId, kind: MoveKind, positions: Vec<Vec2>) -> RuleResult {
    if state.phase != Phase::Movement {
        return err("not the movement phase");
    }
    let u = state.unit(unit).ok_or(RuleError("no such unit".into()))?;
    if u.owner != player || !u.on_board() {
        return err("not your unit");
    }
    if u.flags.moved.is_some() {
        return err("that unit has already moved");
    }
    let engaged = rules::enemies_in_engagement_range(state, unit).next().is_some();
    let m = effective_movement(state, unit);
    let (limit, log_extra) = match kind {
        MoveKind::Stationary => (0.0, String::new()),
        MoveKind::Normal => {
            if engaged {
                return err("a unit in engagement range can only fall back or stay");
            }
            (m, String::new())
        }
        MoveKind::Advance => {
            if engaged {
                return err("a unit in engagement range can only fall back or stay");
            }
            let roll = state.rng.d6() as f32;
            (m + roll, format!(" (advance roll {roll})"))
        }
        MoveKind::FallBack => {
            if !engaged {
                return err("only a unit in engagement range can fall back");
            }
            (m, String::new())
        }
    };
    let placed = if kind == MoveKind::Stationary {
        state.units[unit].alive_models().map(|mm| (mm.pos.unwrap_or_default(), mm.radius())).collect()
    } else {
        validate_positions(state, unit, &positions)?
    };
    check_move_distance(state, unit, &placed, limit)?;
    let ends_engaged = min_distance_to_enemies(state, player, &placed) <= state.params.engagement_range;
    if ends_engaged && kind != MoveKind::Stationary {
        return err("a move cannot end within engagement range of enemy models");
    }
    // Desperate Escape: a battle-shocked unit falling back risks each model.
    let mut lost = 0;
    if kind == MoveKind::FallBack && state.units[unit].battle_shocked {
        let n = state.units[unit].model_count();
        let fail_on = state.params.desperate_escape_fail_on;
        for _ in 0..n {
            if state.rng.d6() <= fail_on {
                lost += 1;
            }
        }
    }
    set_positions(state, unit, &placed);
    if lost > 0 {
        let u = &mut state.units[unit];
        for mm in u.models.iter_mut().filter(|mm| mm.alive()).take(lost) {
            mm.wounds = 0;
        }
    }
    let u = &mut state.units[unit];
    u.flags.moved = Some(kind);
    let name = u.spec.name.clone();
    let verb = match kind {
        MoveKind::Stationary => "remained stationary".to_string(),
        MoveKind::Normal => format!("made a normal move (up to {limit:.0}\")"),
        MoveKind::Advance => format!("advanced (up to {limit:.0}\"){log_extra}"),
        MoveKind::FallBack => format!("fell back{}", if lost > 0 { format!(", losing {lost} models to Desperate Escape") } else { String::new() }),
    };
    state.log(Some(player), format!("{name} {verb}"));
    Ok(())
}

fn arrive(state: &mut GameState, player: PlayerId, unit: UnitId, positions: Vec<Vec2>) -> RuleResult {
    if state.phase != Phase::Movement {
        return err("reserves arrive in the movement phase");
    }
    if state.round < state.params.reserves_from_round {
        return err(format!("reserves cannot arrive before battle round {}", state.params.reserves_from_round));
    }
    let u = state.unit(unit).ok_or(RuleError("no such unit".into()))?;
    if u.owner != player || !u.in_reserve {
        return err("that unit is not in reserve");
    }
    let deep = unit_buffs(state, u, None, None).deep_strike;
    let placed = validate_positions(state, unit, &positions)?;
    if min_distance_to_enemies(state, player, &placed) <= state.params.reserve_distance {
        return err(format!("reserves must arrive more than {}\" from enemy models", state.params.reserve_distance));
    }
    if !deep {
        let w = state.params.board_w;
        let h = state.params.board_h;
        let on_edge = placed.iter().all(|&(p, r)| p.x - r < 6.0 + 1e-3 || p.y - r < 6.0 + 1e-3 || w - p.x - r < 6.0 + 1e-3 || h - p.y - r < 6.0 + 1e-3);
        if !on_edge {
            return err("strategic reserves arrive wholly within 6\" of a battlefield edge");
        }
    }
    set_positions(state, unit, &placed);
    let u = &mut state.units[unit];
    u.in_reserve = false;
    u.flags.moved = Some(MoveKind::Normal);
    u.flags.arrived_this_turn = true;
    let name = u.spec.name.clone();
    state.log(Some(player), format!("{name} arrived from reserve"));
    Ok(())
}

// ------------------------------------------------------------ phase flow

fn end_phase(state: &mut GameState) -> RuleResult {
    match state.phase {
        Phase::Muster | Phase::Deployment | Phase::Ended => err("use the phase's own actions"),
        Phase::PreBattle => {
            // Both players get scout moves; the second player ends it.
            if state.active == state.first_player {
                state.active = 1 - state.first_player;
                Ok(())
            } else {
                start_round(state, 1);
                Ok(())
            }
        }
        Phase::Command => {
            recovery(state);
            score(state, AwardTiming::EndOfCommandPhase);
            state.phase = Phase::Movement;
            Ok(())
        }
        Phase::Movement => {
            state.phase = Phase::Shooting;
            Ok(())
        }
        Phase::Shooting => {
            state.phase = Phase::Charge;
            Ok(())
        }
        Phase::Charge => {
            state.phase = Phase::Fight;
            state.fight_queue.clear();
            Ok(())
        }
        Phase::Fight => {
            if let Some(next) = crate::combat::next_fighter(state) {
                return err(format!("{} must still fight", state.units[next].spec.name));
            }
            end_turn(state)
        }
    }
}

fn start_round(state: &mut GameState, round: u32) {
    state.round = round;
    state.active = state.first_player;
    state.log(None, format!("Battle round {round}"));
    start_turn(state);
}

fn start_turn(state: &mut GameState) {
    state.phase = Phase::Command;
    let active = state.active;
    state.log(Some(active), format!("{}'s turn", state.players[active].name));
    // Both players gain CP.
    for p in &mut state.players {
        p.cp += state.params.cp_per_command_phase;
    }
    state.players[active].vp_this_round = 0;
    for u in state.units.iter_mut().filter(|u| u.owner == active) {
        let shot = u.flags.shot;
        u.flags = TurnFlags { shot_last_turn: shot, ..Default::default() };
        u.charge_targets.clear();
        u.disembarked_after_move = false;
    }
    update_objective_control(state);
    battle_shock(state);
}

fn battle_shock(state: &mut GameState) {
    let active = state.active;
    let ids: Vec<UnitId> = state.units.iter().filter(|u| u.owner == active && u.on_board() && (u.battle_shocked || u.below_half_strength())).map(|u| u.id).collect();
    for id in ids {
        let ld = state.units[id].models.iter().find(|m| m.alive()).and_then(|m| m.stats_override.as_ref().map(|s| s.leadership)).unwrap_or(state.units[id].spec.stats.leadership) as u32;
        let roll = state.rng.roll(&DiceExpr::d6(2));
        let shocked = roll < ld;
        state.units[id].battle_shocked = shocked;
        let name = state.units[id].spec.name.clone();
        state.log(Some(active), format!("{name} battle-shock test: rolled {roll} against Ld {ld}+, {}", if shocked { "failed and is battle-shocked" } else { "passed" }));
    }
}

fn score(state: &mut GameState, timing: AwardTiming) {
    update_objective_control(state);
    let player = state.active;
    let mut gained = 0;
    let awards = state.mission.awards.clone();
    for a in awards.iter().filter(|a| a.timing == timing && state.round >= a.round_min && state.round <= a.round_max) {
        let mine = rules::controlled_count(state, player);
        let theirs = rules::controlled_count(state, 1 - player);
        gained += match &a.rule {
            AwardRule::ObjectiveMajority { vp } => if mine > theirs { *vp } else { 0 },
            AwardRule::PerObjective { vp } => vp * mine,
            AwardRule::PerNonHomeWhileHoldingHome { vp } => {
                let home = state.mission.home_objectives[player];
                let holds_home = home.map(|h| state.objectives[h].controller == Some(player)).unwrap_or(false);
                if holds_home {
                    let non_home = state.objectives.iter().enumerate().filter(|(i, o)| Some(*i) != home && o.controller == Some(player)).count() as u32;
                    vp * non_home
                } else {
                    0
                }
            }
        };
    }
    let cap_round = state.mission.vp_per_round_cap.saturating_sub(state.players[player].vp_this_round);
    let cap_game = state.mission.vp_per_game_cap.saturating_sub(state.players[player].vp);
    let gained = gained.min(cap_round).min(cap_game);
    if gained > 0 {
        state.players[player].vp += gained;
        state.players[player].vp_this_round += gained;
        let total = state.players[player].vp;
        state.log(Some(player), format!("Primary: +{gained} VP ({total} total)"));
    }
}

fn end_turn(state: &mut GameState) -> RuleResult {
    score(state, AwardTiming::EndOfTurn);
    update_objective_control(state);
    let second = 1 - state.first_player;
    if state.active == second {
        if state.round >= state.params.battle_rounds {
            return end_battle(state);
        }
        start_round(state, state.round + 1);
    } else {
        state.active = second;
        start_turn(state);
    }
    Ok(())
}

fn end_battle(state: &mut GameState) -> RuleResult {
    state.phase = Phase::Ended;
    let (a, b) = (state.players[0].vp, state.players[1].vp);
    state.winner = Some(if a > b { Some(0) } else if b > a { Some(1) } else { None });
    let msg = match state.winner.unwrap() {
        Some(p) => format!("Battle over: {} wins {a} to {b}", state.players[p].name),
        None => format!("Battle over: a draw at {a} each"),
    };
    state.log(None, msg);
    Ok(())
}
