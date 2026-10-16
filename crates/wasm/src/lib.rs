//! JSON in, JSON out. The client holds a `Game` and drives it with actions.

use engine::actions::Action;
use engine::content::{ArmyPack, EditionParams, Mission};
use engine::state::GameState;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct Game {
    state: GameState,
}

#[wasm_bindgen]
impl Game {
    /// `packs` is a JSON array of two ArmyPacks, `mission` a JSON Mission.
    #[wasm_bindgen(constructor)]
    pub fn new(packs: &str, mission: &str, seed: u64) -> Result<Game, JsError> {
        let packs: [ArmyPack; 2] = serde_json::from_str(packs).map_err(|e| JsError::new(&format!("packs: {e}")))?;
        let mission: Mission = serde_json::from_str(mission).map_err(|e| JsError::new(&format!("mission: {e}")))?;
        Ok(Game { state: GameState::new(packs, mission, EditionParams::default(), seed) })
    }

    /// A small synthetic battle for client development: two faction-neutral
    /// armies on a plain board. Replaced by real packs once content loads.
    pub fn demo(seed: u64) -> Game {
        Game { state: engine::demo::demo_game(seed) }
    }

    /// Restore from a state JSON produced by `state()`.
    pub fn restore(json: &str) -> Result<Game, JsError> {
        Ok(Game { state: serde_json::from_str(json).map_err(|e| JsError::new(&e.to_string()))? })
    }

    pub fn state(&self) -> String {
        serde_json::to_string(&self.state).unwrap()
    }

    /// Apply an action JSON (`{"type":"move",...}`) for `player`. Returns the
    /// rule error text on failure and leaves the state untouched.
    pub fn apply(&mut self, player: usize, action: &str) -> Result<(), JsError> {
        let action: Action = serde_json::from_str(action).map_err(|e| JsError::new(&format!("bad action: {e}")))?;
        let mut next = self.state.clone();
        engine::actions::apply(&mut next, player, action).map_err(|e| JsError::new(&e.0))?;
        self.state = next;
        Ok(())
    }

    /// Who acts next: the active player, or whoever picks the next fighter.
    pub fn player_to_act(&self) -> usize {
        if self.state.phase == engine::state::Phase::Fight {
            engine::combat::fight_owner_to_act(&self.state).unwrap_or(self.state.active)
        } else {
            self.state.active
        }
    }

    /// Unit id that must fight next in the fight phase, or -1.
    pub fn next_fighter(&self) -> i32 {
        engine::combat::next_fighter(&self.state).map(|u| u as i32).unwrap_or(-1)
    }

    /// Buffs JSON for a unit as it stands now (FNP, invuln, modifiers, and
    /// the list of abilities the engine leaves to the players).
    pub fn unit_buffs(&self, unit: usize) -> String {
        let u = &self.state.units[unit];
        serde_json::to_string(&engine::rules::unit_buffs(&self.state, u, None, None)).unwrap()
    }

    /// Which enemy units a unit can currently see with any model, as JSON ids.
    pub fn visible_enemies(&self, unit: usize) -> String {
        let u = &self.state.units[unit];
        let ids: Vec<usize> = self
            .state
            .enemies_of(u.owner)
            .filter(|e| u.alive_models().any(|m| m.pos.map(|p| engine::rules::can_see_unit(&self.state, p, e)).unwrap_or(false)))
            .map(|e| e.id)
            .collect();
        serde_json::to_string(&ids).unwrap()
    }
}

/// Parse a `.ros` export and build its content pack. `abilities_json` is the
/// text of the faction's 40kdc `abilities.json` (may be an empty array).
#[wasm_bindgen]
pub fn build_pack(ros_xml: &str, abilities_json: &str, core_abilities_json: &str) -> Result<String, JsError> {
    let army = roster::parse_ros(ros_xml).map_err(|e| JsError::new(&format!("roster: {e}")))?;
    let mut map = std::collections::HashMap::new();
    for text in [core_abilities_json, abilities_json] {
        if text.trim().is_empty() {
            continue;
        }
        let list: Vec<serde_json::Value> = serde_json::from_str(text).map_err(|e| JsError::new(&format!("abilities: {e}")))?;
        for a in list {
            if let (Some(name), Some(effect)) = (a.get("name").and_then(|v| v.as_str()), a.get("effect")) {
                map.entry(name.to_ascii_lowercase()).or_insert_with(|| effect.clone());
            }
        }
    }
    let pack = content::pack::build_pack(&army, &map);
    Ok(serde_json::to_string(&pack).unwrap())
}

/// The 40kdc faction slug for a roster's catalogue name, so the client knows
/// which abilities file to fetch.
#[wasm_bindgen]
pub fn faction_slug(ros_xml: &str) -> Result<String, JsError> {
    let army = roster::parse_ros(ros_xml).map_err(|e| JsError::new(&format!("roster: {e}")))?;
    Ok(content::enrich::faction_slug(&army.faction).unwrap_or("_core").to_string())
}

/// Build a mission from the text of 40kdc's `deployment-patterns.json`.
#[wasm_bindgen]
pub fn build_mission(patterns_json: &str, pattern_id: &str) -> Result<String, JsError> {
    let m = content::mission::mission_from_patterns(patterns_json, pattern_id).map_err(|e| JsError::new(&e.to_string()))?;
    Ok(serde_json::to_string(&m).unwrap())
}
