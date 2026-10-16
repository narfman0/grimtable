// Shapes of the JSON the wasm engine produces (serde of the Rust structs in
// crates/engine). Positions are inches; the board origin is top-left.

export interface Vec2 {
  x: number;
  y: number;
}

export interface Polygon {
  points: Vec2[];
}

export interface DiceExpr {
  count: number;
  sides: 'D3' | 'D6';
  add: number;
}

/** Unit variants are strings, tuple/struct variants are one-key objects. */
export type EnumValue = string | Record<string, unknown>;

export interface WeaponProfile {
  name: string;
  attacks: DiceExpr;
  skill: number | null;
  strength: number;
  ap: number;
  damage: DiceExpr;
  abilities: EnumValue[];
}

export interface WeaponSpec {
  kind: 'Ranged' | 'Melee';
  range: number | null;
  profile: WeaponProfile;
  profile_group: string | null;
}

export interface Stats {
  movement: number;
  toughness: number;
  save: number;
  wounds: number;
  leadership: number;
  objective_control: number;
  invuln: number | null;
  base_mm: number;
}

export interface ModelSpec {
  name: string;
  count: number;
  stats: Stats | null;
  weapons: number[];
}

export interface AbilitySpec {
  name: string;
  effect: unknown | null;
}

export interface UnitSpec {
  name: string;
  points: number;
  keywords: string[];
  stats: Stats;
  models: ModelSpec[];
  weapons: WeaponSpec[];
  core_rules: EnumValue[];
  abilities: AbilitySpec[];
  can_lead: string[];
  transport_capacity: number | null;
}

export interface ArmyPack {
  name: string;
  faction: string;
  detachment: string | null;
  points: number;
  units: UnitSpec[];
  army_rules: AbilitySpec[];
}

export interface Model {
  id: number;
  name: string;
  pos: Vec2 | null;
  base_mm: number;
  wounds: number;
  max_wounds: number;
  weapons: number[];
  is_leader: boolean;
  stats_override: Stats | null;
}

export type MoveKind = 'Stationary' | 'Normal' | 'Advance' | 'FallBack';

export interface TurnFlags {
  moved: MoveKind | null;
  shot: boolean;
  shot_last_turn: boolean;
  charged: boolean;
  fought: boolean;
  arrived_this_turn: boolean;
}

export interface Unit {
  id: number;
  owner: number;
  spec: UnitSpec;
  models: Model[];
  starting_models: number;
  in_reserve: boolean;
  deployed: boolean;
  battle_shocked: boolean;
  flags: TurnFlags;
  attached_leaders: string[];
  merged_into: number | null;
  charge_targets: number[];
  /** The transport this unit is embarked in, if any. */
  embarked_in: number | null;
  disembarked_after_move: boolean;
}

export interface Player {
  name: string;
  faction: string;
  detachment: string | null;
  cp: number;
  vp: number;
  vp_this_round: number;
  /** Army-wide rules (faction and detachment). */
  rules: AbilitySpec[];
  /** Chosen option index per rule or choice name. */
  stances: Record<string, number>;
}

export interface Objective {
  pos: Vec2;
  controller: number | null;
  secured_by: number | null;
}

export type Phase =
  | 'Muster'
  | 'Deployment'
  | 'PreBattle'
  | 'Command'
  | 'Movement'
  | 'Shooting'
  | 'Charge'
  | 'Fight'
  | 'Ended';

export interface LogEvent {
  round: number;
  phase: Phase;
  player: number | null;
  text: string;
}

export interface Mission {
  name: string;
  deployment_zones: [Polygon, Polygon];
  objectives: Vec2[];
  home_objectives: [number | null, number | null];
  terrain: Polygon[];
  vp_per_round_cap: number;
  vp_per_game_cap: number;
}

export interface EditionParams {
  board_w: number;
  board_h: number;
  battle_rounds: number;
  engagement_range: number;
  coherency_range: number;
  coherency_two_neighbours_from: number;
  reserve_distance: number;
  reserves_from_round: number;
  objective_range: number;
  charge_range: number;
  pile_in: number;
  consolidate: number;
}

export type Pending = { ChargeMove: { unit: number; roll: number } } | null;

export interface GameState {
  params: EditionParams;
  mission: Mission;
  round: number;
  phase: Phase;
  active: number;
  first_player: number;
  players: [Player, Player];
  units: Unit[];
  objectives: Objective[];
  pending: Pending;
  fight_queue: number[];
  log: LogEvent[];
  /** serde flattens Option<Option<_>>: null is "no winner yet" or a draw. */
  winner: number | null;
}

export interface Buffs {
  feel_no_pain: number | null;
  invuln: number | null;
  hit_modifier: number;
  wound_modifier: number;
  reroll_hits: 'None' | 'Ones' | 'Failed' | 'Any';
  reroll_wounds: 'None' | 'Ones' | 'Failed' | 'Any';
  stat_add: [string, number][];
  keywords: string[];
  oc_add: number;
  oc_sticky: boolean;
  move_add: number;
  damage_reduction: number;
  fights_first: boolean;
  deep_strike: boolean;
  stealth: boolean;
  lone_operative: boolean;
  /** Granted weapon keywords, e.g. ["Heavy", "Ranged"]. */
  weapon_abilities: [string, 'Ranged' | 'Melee' | null][];
  enemy_melee_hit_modifier: number;
  critical_hit_on: number | null;
  /** Dice healed at the end of the Command phase. */
  recovery: string | null;
  /** Choices an army rule or ability is waiting on: [name, option labels]. */
  choices_pending: [string, string[]][];
  unsupported: string[];
}
