//! The content-pack contract: what the engine needs to know about an army and
//! a mission. The content crate builds these from a roster and the data
//! submodules. Nothing here is prose.

use crate::attack::Weapon;
use crate::effects::Effect;
use crate::geom::{Polygon, Vec2};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ArmyPack {
    pub name: String,
    pub faction: String,
    pub detachment: Option<String>,
    pub points: u32,
    pub units: Vec<UnitSpec>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UnitSpec {
    pub name: String,
    pub points: u32,
    pub keywords: Vec<String>,
    pub stats: Stats,
    pub models: Vec<ModelSpec>,
    pub weapons: Vec<WeaponSpec>,
    pub core_rules: Vec<CoreRule>,
    pub abilities: Vec<AbilitySpec>,
    /// Unit names this unit may lead, if it has Leader or Support.
    pub can_lead: Vec<String>,
    pub transport_capacity: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stats {
    pub movement: u32,
    pub toughness: u32,
    pub save: u8,
    pub wounds: u32,
    pub leadership: u8,
    pub objective_control: u32,
    pub invuln: Option<u8>,
    pub base_mm: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelSpec {
    pub name: String,
    pub count: u32,
    pub stats: Option<Stats>,
    /// Indexes into `UnitSpec::weapons`, one per weapon each model carries.
    pub weapons: Vec<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WeaponKind {
    Ranged,
    Melee,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WeaponSpec {
    pub kind: WeaponKind,
    pub range: Option<u32>,
    pub profile: Weapon,
    /// Weapons sharing a group are alternative profiles of one weapon: a
    /// model uses only one of them per attack.
    pub profile_group: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum CoreRule {
    DeadlyDemise(String),
    DeepStrike,
    FeelNoPain(u8),
    FightsFirst,
    FiringDeck(u32),
    Infiltrators,
    Leader,
    LoneOperative,
    Scouts(u32),
    Stealth,
    Support,
    Other(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AbilitySpec {
    pub name: String,
    /// Structured effect when the data has one. `None` means the players
    /// apply it by hand; the client lists these so nothing is silently lost.
    pub effect: Option<Effect>,
}

/// Edition-level numbers. Content, not code, so another edition is a file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EditionParams {
    pub board_w: f32,
    pub board_h: f32,
    pub battle_rounds: u32,
    pub engagement_range: f32,
    pub coherency_range: f32,
    pub coherency_two_neighbours_from: u32,
    pub cp_per_command_phase: u32,
    pub modifier_cap: i32,
    pub hidden_range: f32,
    pub lone_operative_range: f32,
    pub reserve_distance: f32,
    pub reserves_from_round: u32,
    pub objective_range: f32,
    pub charge_range: f32,
    pub pile_in: f32,
    pub consolidate: f32,
    pub desperate_escape_fail_on: u8,
    pub hazardous_fail_on: u8,
}

impl Default for EditionParams {
    fn default() -> Self {
        EditionParams {
            board_w: 60.0,
            board_h: 44.0,
            battle_rounds: 5,
            engagement_range: 1.0,
            coherency_range: 2.0,
            coherency_two_neighbours_from: 7,
            cp_per_command_phase: 1,
            modifier_cap: 1,
            hidden_range: 15.0,
            lone_operative_range: 12.0,
            reserve_distance: 8.0,
            reserves_from_round: 2,
            objective_range: 3.0,
            charge_range: 12.0,
            pile_in: 3.0,
            consolidate: 3.0,
            desperate_escape_fail_on: 2,
            hazardous_fail_on: 1,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Mission {
    pub name: String,
    pub deployment_zones: [Polygon; 2],
    pub objectives: Vec<Vec2>,
    /// Index into `objectives` of each player's home objective, if any.
    pub home_objectives: [Option<usize>; 2],
    pub terrain: Vec<Polygon>,
    pub awards: Vec<Award>,
    pub vp_per_round_cap: u32,
    pub vp_per_game_cap: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AwardTiming {
    EndOfCommandPhase,
    EndOfTurn,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum AwardRule {
    /// Flat VP if you control more objectives than the opponent.
    ObjectiveMajority { vp: u32 },
    /// VP per controlled objective.
    PerObjective { vp: u32 },
    /// VP per controlled non-home objective, only while you hold your home.
    PerNonHomeWhileHoldingHome { vp: u32 },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Award {
    pub timing: AwardTiming,
    pub round_min: u32,
    pub round_max: u32,
    pub rule: AwardRule,
}
