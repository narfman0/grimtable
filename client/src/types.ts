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

// ------------------------------------------------------------ catalogue
// Mirrors crates/content/src/catalogue.rs: a faction's army-construction data.

export interface PointsTier {
  models: number;
  cost: number;
}

export interface LoadoutVariant {
  name: string;
  weapon_ids: string[];
  max_count: number | null;
}

export interface ModelRow {
  name: string;
  min: number;
  max: number;
  default_weapon_ids: string[];
  loadout_variants: LoadoutVariant[];
  is_leader_model: boolean;
  stats: Stats | null;
}

/** Raw 40kdc wargear option; only the fields the editor reads are typed. */
export interface WargearOption {
  id?: string;
  is_free?: boolean;
  model_constraint?: { model_name?: string; any_number?: boolean; max_count?: number } | null;
  replaces?: string[];
  replacement?: string[];
  replacement_choice?: string[][];
  [key: string]: unknown;
}

export interface Datasheet {
  id: string;
  name: string;
  role: string;
  keywords: string[];
  stats: Stats;
  points: PointsTier[];
  models: ModelRow[];
  weapon_ids: string[];
  wargear_options: WargearOption[];
  ability_ids: string[];
  can_lead_ids: string[];
  transport_capacity: number | null;
  is_epic_hero: boolean;
  is_battleline: boolean;
  is_dedicated_transport: boolean;
  bsdata_id: string | null;
}

export interface CatalogueWeaponProfile {
  name: string;
  range: number | null;
  attacks: string;
  skill: number | null;
  strength: number;
  ap: number;
  damage: string;
  keywords: string[];
}

export interface CatalogueWeapon {
  id: string;
  name: string;
  kind: 'ranged' | 'melee' | string;
  profiles: CatalogueWeaponProfile[];
}

export interface Detachment {
  id: string;
  name: string;
  detachment_points: number;
  rule_ability_id: string | null;
  enhancement_ids: string[];
}

export interface Enhancement {
  id: string;
  name: string;
  detachment_id: string;
  cost: number;
  keyword_restrictions: string[];
  ability_id: string | null;
}

export interface Catalogue {
  faction_id: string;
  name: string;
  faction_keywords: string[];
  detachments: Detachment[];
  enhancements: Enhancement[];
  units: Datasheet[];
  weapons: CatalogueWeapon[];
  ability_names: Record<string, string>;
  points_source: string;
}

// ------------------------------------------------------------ army def
// Mirrors crates/content/src/army_def.rs: what the editor saves.

export interface ModelRowDef {
  row: string;
  count: number;
  weapon_ids: string[];
  variants: [string, number][];
}

export interface UnitDef {
  datasheet_id: string;
  models: ModelRowDef[];
  enhancement_id: string | null;
  warlord: boolean;
}

export interface ArmyDef {
  name: string;
  faction_id: string;
  detachment_ids: string[];
  units: UnitDef[];
}

// ------------------------------------------------------------ server API
// Mirrors docs/multiplayer.md. Ids are opaque; they are only echoed back.

export type Id = string | number;

export interface User {
  id: Id;
  username: string;
}

export type ArmyKind = 'def' | 'ros';

export interface ArmySummary {
  id: Id;
  name: string;
  kind: ArmyKind;
  faction_id: string;
  points: number;
  updated_at: string;
  /** Not in the list contract; used when the server includes it. */
  problems?: string[];
}

export interface ArmyRecord extends ArmySummary {
  def?: ArmyDef;
  problems: string[];
}

export interface FactionSummary {
  faction_id: string;
  name: string;
}

export type Visibility = 'open' | 'closed';
export type GameStatus = 'waiting' | 'playing' | 'finished' | 'cancelled';

export interface GameSeat {
  seat: number;
  username: string;
  army_name: string;
  army_points: number;
  army_id?: Id;
}

export interface GameRecord {
  id: Id;
  /** null for viewers who are not seated. */
  join_code: string | null;
  visibility: Visibility;
  status: GameStatus;
  pattern: string;
  points_limit: number;
  seats: GameSeat[];
  my_seat: number | null;
  /** 0 or 1; null before the end and for a draw. */
  winner: number | null;
  host?: string;
  created_at?: string;
  /** On the start response: per-seat army problems (informational unless the start was refused). */
  problems?: string[][];
}

export interface OpenGame {
  id: Id;
  host: string;
  pattern: string;
  points_limit: number;
  created_at: string;
}

export type ServerMessage =
  | { type: 'state'; seat: number | null; state: GameState; to_act: number }
  | { type: 'error'; message: string }
  | { type: 'lobby'; game: GameRecord }
  | { type: 'pong' };
