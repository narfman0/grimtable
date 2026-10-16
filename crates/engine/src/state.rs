//! Game state. Everything the client renders and everything a replay needs.
//! Positions are in inches on the board; see `geom`.

use crate::content::{ArmyPack, EditionParams, Mission, UnitSpec};
use crate::dice::Rng;
use crate::geom::{base_radius_in, Vec2};
use serde::{Deserialize, Serialize};

pub type UnitId = usize;
pub type ModelId = usize;
pub type PlayerId = usize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    /// Muster: attach leaders, choose who deploys first.
    Muster,
    Deployment,
    /// Pre-battle moves (Scouts).
    PreBattle,
    Command,
    Movement,
    Shooting,
    Charge,
    Fight,
    Ended,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MoveKind {
    Stationary,
    Normal,
    Advance,
    FallBack,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub id: ModelId,
    pub name: String,
    /// `None` while in reserve or not yet deployed.
    pub pos: Option<Vec2>,
    pub base_mm: u32,
    pub wounds: u32,
    pub max_wounds: u32,
    /// Index into the unit's `weapons`.
    pub weapons: Vec<usize>,
    pub is_leader: bool,
    pub stats_override: Option<crate::content::Stats>,
}

impl Model {
    pub fn radius(&self) -> f32 {
        base_radius_in(self.base_mm)
    }

    pub fn alive(&self) -> bool {
        self.wounds > 0
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TurnFlags {
    pub moved: Option<MoveKind>,
    pub shot: bool,
    pub shot_last_turn: bool,
    pub charged: bool,
    pub fought: bool,
    pub arrived_this_turn: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Unit {
    pub id: UnitId,
    pub owner: PlayerId,
    pub spec: UnitSpec,
    pub models: Vec<Model>,
    pub starting_models: u32,
    pub in_reserve: bool,
    pub deployed: bool,
    pub battle_shocked: bool,
    pub flags: TurnFlags,
    /// Leader units merged into this one at muster, by original unit name.
    pub attached_leaders: Vec<String>,
    /// Set when this unit was a leader that merged into another unit.
    pub merged_into: Option<UnitId>,
    /// Units this unit has declared a charge against this turn.
    pub charge_targets: Vec<UnitId>,
}

impl Unit {
    pub fn alive_models(&self) -> impl Iterator<Item = &Model> {
        self.models.iter().filter(|m| m.alive())
    }

    pub fn model_count(&self) -> u32 {
        self.alive_models().count() as u32
    }

    pub fn is_destroyed(&self) -> bool {
        self.merged_into.is_some() || self.model_count() == 0
    }

    pub fn on_board(&self) -> bool {
        !self.is_destroyed() && !self.in_reserve && self.deployed
    }

    pub fn below_half_strength(&self) -> bool {
        let n = self.model_count();
        if self.starting_models == 1 {
            let m = &self.models[0];
            return m.wounds * 2 < m.max_wounds;
        }
        n * 2 < self.starting_models
    }

    pub fn has_keyword(&self, kw: &str) -> bool {
        self.spec.keywords.iter().any(|k| k.eq_ignore_ascii_case(kw))
            || self.spec.keywords.iter().any(|k| {
                k.strip_prefix("Faction: ").map(|f| f.eq_ignore_ascii_case(kw)).unwrap_or(false)
            })
    }

    pub fn is_attached(&self) -> bool {
        !self.attached_leaders.is_empty()
    }

    pub fn centre(&self) -> Option<Vec2> {
        let pts: Vec<Vec2> = self.alive_models().filter_map(|m| m.pos).collect();
        if pts.is_empty() {
            return None;
        }
        let n = pts.len() as f32;
        Some(Vec2::new(pts.iter().map(|p| p.x).sum::<f32>() / n, pts.iter().map(|p| p.y).sum::<f32>() / n))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Player {
    pub name: String,
    pub faction: String,
    pub detachment: Option<String>,
    pub cp: u32,
    pub vp: u32,
    pub vp_this_round: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Objective {
    pub pos: Vec2,
    pub controller: Option<PlayerId>,
    /// Who keeps it while nobody is on it (sticky objectives).
    pub secured_by: Option<PlayerId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub round: u32,
    pub phase: Phase,
    pub player: Option<PlayerId>,
    pub text: String,
}

/// A decision the engine is waiting on before the game can continue.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Pending {
    /// The active unit must finish its charge move.
    ChargeMove { unit: UnitId, roll: u32 },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GameState {
    pub params: EditionParams,
    pub mission: Mission,
    pub rng: Rng,
    pub round: u32,
    pub phase: Phase,
    pub active: PlayerId,
    /// Who takes the first turn of each round.
    pub first_player: PlayerId,
    pub players: [Player; 2],
    pub units: Vec<Unit>,
    pub objectives: Vec<Objective>,
    pub pending: Option<Pending>,
    /// Units that have fought this phase, in order, for alternation.
    pub fight_queue: Vec<UnitId>,
    pub log: Vec<Event>,
    pub winner: Option<Option<PlayerId>>,
}

impl GameState {
    pub fn new(packs: [ArmyPack; 2], mission: Mission, params: EditionParams, seed: u64) -> Self {
        let mut units = Vec::new();
        let mut players = Vec::new();
        for (owner, pack) in packs.into_iter().enumerate() {
            players.push(Player {
                name: pack.name.clone(),
                faction: pack.faction.clone(),
                detachment: pack.detachment.clone(),
                cp: 0,
                vp: 0,
                vp_this_round: 0,
            });
            for spec in pack.units {
                let id = units.len();
                units.push(instantiate(id, owner, spec));
            }
        }
        let objectives = mission.objectives.iter().map(|&pos| Objective { pos, controller: None, secured_by: None }).collect();
        let players: [Player; 2] = [players.remove(0), players.remove(0)];
        GameState {
            params,
            mission,
            rng: Rng::new(seed),
            round: 0,
            phase: Phase::Muster,
            active: 0,
            first_player: 0,
            players,
            units,
            objectives,
            pending: None,
            fight_queue: Vec::new(),
            log: Vec::new(),
            winner: None,
        }
    }

    pub fn log(&mut self, player: Option<PlayerId>, text: impl Into<String>) {
        self.log.push(Event { round: self.round, phase: self.phase, player, text: text.into() });
    }

    pub fn unit(&self, id: UnitId) -> Option<&Unit> {
        self.units.get(id)
    }

    pub fn enemies_of(&self, player: PlayerId) -> impl Iterator<Item = &Unit> {
        self.units.iter().filter(move |u| u.owner != player && u.on_board())
    }

    pub fn friends_of(&self, player: PlayerId) -> impl Iterator<Item = &Unit> {
        self.units.iter().filter(move |u| u.owner == player && u.on_board())
    }
}

fn instantiate(id: UnitId, owner: PlayerId, spec: UnitSpec) -> Unit {
    let mut models = Vec::new();
    for group in &spec.models {
        let stats = group.stats.as_ref().unwrap_or(&spec.stats);
        for _ in 0..group.count {
            models.push(Model {
                id: models.len(),
                name: group.name.clone(),
                pos: None,
                base_mm: stats.base_mm,
                wounds: stats.wounds,
                max_wounds: stats.wounds,
                weapons: group.weapons.clone(),
                is_leader: false,
                stats_override: group.stats.clone(),
            });
        }
    }
    let starting_models = models.len() as u32;
    Unit {
        id,
        owner,
        spec,
        models,
        starting_models,
        in_reserve: false,
        deployed: false,
        battle_shocked: false,
        flags: TurnFlags::default(),
        attached_leaders: Vec::new(),
        merged_into: None,
        charge_targets: Vec::new(),
    }
}
