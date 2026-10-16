//! Derived rules over game state: what a unit's abilities add up to right
//! now, who can see whom, cover, engagement, coherency, objective control.

use crate::content::CoreRule;
use crate::effects::{self, AttackType, Buffs, Ctx};
use crate::geom::{base_distance, Vec2};
use crate::state::{GameState, Phase, PlayerId, Unit, UnitId};

/// Everything a unit's core rules and abilities grant it in the given situation.
pub fn unit_buffs(state: &GameState, unit: &Unit, target: Option<&Unit>, attack_type: Option<AttackType>) -> Buffs {
    unit_buffs_at(state, unit, target, attack_type, None)
}

/// Same, with a timing marker such as "end-of-command-phase".
pub fn unit_buffs_at(state: &GameState, unit: &Unit, target: Option<&Unit>, attack_type: Option<AttackType>, timing: Option<&str>) -> Buffs {
    let mut buffs = Buffs::default();
    for rule in &unit.spec.core_rules {
        match rule {
            CoreRule::FeelNoPain(t) => buffs.merge(Buffs { feel_no_pain: Some(*t), ..Default::default() }),
            CoreRule::Stealth => buffs.stealth = true,
            CoreRule::LoneOperative => buffs.lone_operative = true,
            CoreRule::DeepStrike => buffs.deep_strike = true,
            CoreRule::FightsFirst => buffs.fights_first = true,
            // Printed rule names the army rules already cover, or that are
            // structural (Leader, Support), are not left to the players.
            CoreRule::Other(name) => {
                let covered = state.players[unit.owner].rules.iter().any(|r| r.name.eq_ignore_ascii_case(name));
                if !covered {
                    buffs.unsupported.push(name.clone());
                }
            }
            _ => {}
        }
    }
    let target_keywords = target.map(|t| t.spec.keywords.as_slice());
    let near_friendly = |keyword: &str, range: f32| -> bool {
        state.friends_of(unit.owner).filter(|f| f.id != unit.id).any(|f| {
            keyword.split_whitespace().all(|w| f.has_keyword(w)) && unit_distance(unit, f).map(|d| d <= range).unwrap_or(false)
        })
    };
    let ctx = Ctx {
        phase: Some(state.phase),
        my_turn: state.active == unit.owner,
        attached: unit.is_attached(),
        unit_keywords: &unit.spec.keywords,
        target_keywords,
        attack_type,
        battle_shocked: unit.battle_shocked,
        below_half_strength: unit.below_half_strength(),
        charged_this_turn: unit.flags.charged,
        within_range_of_objective: nearest_objective_distance(state, unit).map(|d| d <= state.params.objective_range).unwrap_or(false),
        in_engagement_range: enemies_in_engagement_range(state, unit.id).next().is_some(),
        timing,
        stances: Some(&state.players[unit.owner].stances),
        near_friendly: Some(&near_friendly),
    };
    for ability in &unit.spec.abilities {
        match &ability.effect {
            Some(e) => buffs.merge(effects::evaluate(&ability.name, e, &ctx)),
            None => buffs.unsupported.push(ability.name.clone()),
        }
    }
    for rule in &state.players[unit.owner].rules {
        match &rule.effect {
            Some(e) => buffs.merge(effects::evaluate(&rule.name, e, &ctx)),
            None => buffs.unsupported.push(rule.name.clone()),
        }
    }
    buffs.unsupported.retain(|n| !matches!(n.as_str(), "Leader" | "Support"));
    let mut seen: Vec<String> = Vec::new();
    buffs.unsupported.retain(|n| {
        let key = n.to_ascii_lowercase();
        if seen.contains(&key) {
            false
        } else {
            seen.push(key);
            true
        }
    });
    buffs
}

pub fn nearest_objective_distance(state: &GameState, unit: &Unit) -> Option<f32> {
    let mut best: Option<f32> = None;
    for m in unit.alive_models() {
        if let Some(p) = m.pos {
            for o in &state.objectives {
                let d = (p.dist(o.pos) - m.radius()).max(0.0);
                best = Some(best.map_or(d, |b: f32| b.min(d)));
            }
        }
    }
    best
}

/// Closest edge-to-edge distance between two units, if both are on the board.
pub fn unit_distance(a: &Unit, b: &Unit) -> Option<f32> {
    let mut best: Option<f32> = None;
    for ma in a.alive_models() {
        for mb in b.alive_models() {
            if let (Some(pa), Some(pb)) = (ma.pos, mb.pos) {
                let d = base_distance(pa, ma.radius(), pb, mb.radius());
                best = Some(best.map_or(d, |x: f32| x.min(d)));
            }
        }
    }
    best
}

pub fn enemies_in_engagement_range(state: &GameState, unit: UnitId) -> impl Iterator<Item = &Unit> {
    let u = &state.units[unit];
    let er = state.params.engagement_range;
    state.enemies_of(u.owner).filter(move |e| unit_distance(u, e).map(|d| d <= er).unwrap_or(false))
}

/// Line of sight from one point to another: blocked when the segment crosses
/// a terrain piece that neither end is inside.
pub fn visible_point(state: &GameState, from: Vec2, to: Vec2) -> bool {
    !state.mission.terrain.iter().any(|t| t.crosses(from, to) && !t.contains(from) && !t.contains(to))
}

pub fn in_terrain(state: &GameState, p: Vec2) -> bool {
    state.mission.terrain.iter().any(|t| t.contains(p))
}

/// Can `model_pos` of `shooter` see any model of `target`, applying the
/// hidden rule for infantry wholly inside terrain?
pub fn can_see_unit(state: &GameState, from: Vec2, target: &Unit) -> bool {
    let hidden = target.has_keyword("Infantry")
        && !target.flags.shot
        && !target.flags.shot_last_turn
        && target.alive_models().all(|m| m.pos.map(|p| in_terrain(state, p)).unwrap_or(false));
    target.alive_models().any(|m| {
        m.pos
            .map(|p| visible_point(state, from, p) && (!hidden || from.dist(p) <= state.params.hidden_range))
            .unwrap_or(false)
    })
}

/// Cover: every model in the target is inside terrain, or the unit has Stealth.
pub fn target_in_cover(state: &GameState, target: &Unit, target_buffs: &Buffs) -> bool {
    target_buffs.stealth || target.alive_models().all(|m| m.pos.map(|p| in_terrain(state, p)).unwrap_or(false))
}

/// Unit coherency: each model within range of at least one other (two, in
/// units of seven or more). Single models are always coherent.
pub fn coherent(state: &GameState, positions: &[(Vec2, f32)]) -> bool {
    let n = positions.len();
    if n <= 1 {
        return true;
    }
    let need = if n as u32 >= state.params.coherency_two_neighbours_from { 2 } else { 1 };
    positions.iter().enumerate().all(|(i, &(p, r))| {
        let neighbours = positions
            .iter()
            .enumerate()
            .filter(|&(j, &(q, rq))| j != i && base_distance(p, r, q, rq) <= state.params.coherency_range)
            .count();
        neighbours >= need.min(n - 1)
    })
}

/// A unit's objective control, after battle-shock and ability modifiers.
pub fn objective_control(state: &GameState, unit: &Unit) -> u32 {
    if unit.battle_shocked {
        return 0;
    }
    let buffs = unit_buffs(state, unit, None, None);
    let mut total: i32 = 0;
    for m in unit.alive_models() {
        let oc = m.stats_override.as_ref().map(|s| s.objective_control).unwrap_or(unit.spec.stats.objective_control) as i32;
        total += (oc + buffs.oc_add).max(0);
    }
    total.max(0) as u32
}

/// Recompute who controls each objective. Sticky objectives stay with the
/// last controller while nobody else out-controls them.
pub fn update_objective_control(state: &mut GameState) {
    let range = state.params.objective_range;
    let mut results = Vec::new();
    for (i, o) in state.objectives.iter().enumerate() {
        let mut oc = [0u32; 2];
        let mut sticky_present = [false; 2];
        for u in state.units.iter().filter(|u| u.on_board()) {
            let near = u.alive_models().any(|m| m.pos.map(|p| (p.dist(o.pos) - m.radius()).max(0.0) <= range).unwrap_or(false));
            if near {
                oc[u.owner] += objective_control(state, u);
                if unit_buffs(state, u, None, None).oc_sticky {
                    sticky_present[u.owner] = true;
                }
            }
        }
        let holder: Option<PlayerId> = if oc[0] > oc[1] {
            Some(0)
        } else if oc[1] > oc[0] {
            Some(1)
        } else {
            None
        };
        results.push((i, holder, sticky_present));
    }
    for (i, holder, sticky_present) in results {
        let o = &mut state.objectives[i];
        match holder {
            Some(p) => {
                o.controller = Some(p);
                o.secured_by = if sticky_present[p] { Some(p) } else { None };
            }
            None => {
                o.controller = o.secured_by;
            }
        }
    }
}

pub fn controlled_count(state: &GameState, player: PlayerId) -> u32 {
    state.objectives.iter().filter(|o| o.controller == Some(player)).count() as u32
}

pub fn phase_name(phase: Phase) -> &'static str {
    match phase {
        Phase::Muster => "Muster",
        Phase::Deployment => "Deployment",
        Phase::PreBattle => "Pre-battle",
        Phase::Command => "Command",
        Phase::Movement => "Movement",
        Phase::Shooting => "Shooting",
        Phase::Charge => "Charge",
        Phase::Fight => "Fight",
        Phase::Ended => "Ended",
    }
}
