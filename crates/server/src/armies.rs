//! Armies: editor definitions (`def`, an `ArmyDef`) and imported New Recruit
//! exports (`ros`, scrubbed XML). Both become an engine `ArmyPack` here.

use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use content::army_def::{self, ArmyDef};
use engine::content::ArmyPack;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;
use serde_json::{json, Value};

/// The standard battle size the editor validates against.
pub const DEFAULT_POINTS_LIMIT: u32 = 2000;
/// Detachment points available at that size.
pub const DP_BUDGET: u32 = 3;

#[derive(Clone, Debug)]
pub struct ArmyRow {
    pub id: i64,
    pub user_id: i64,
    pub name: String,
    pub kind: String,
    pub faction_id: Option<String>,
    pub points: u32,
    pub body: String,
    pub updated_at: String,
}

const COLS: &str = "id, user_id, name, kind, faction_id, points, body, updated_at";

fn row(r: &rusqlite::Row) -> rusqlite::Result<ArmyRow> {
    Ok(ArmyRow {
        id: r.get(0)?,
        user_id: r.get(1)?,
        name: r.get(2)?,
        kind: r.get(3)?,
        faction_id: r.get(4)?,
        points: r.get::<_, i64>(5)?.max(0) as u32,
        body: r.get(6)?,
        updated_at: r.get(7)?,
    })
}

pub fn load(c: &Connection, id: i64) -> rusqlite::Result<Option<ArmyRow>> {
    c.query_row(&format!("SELECT {COLS} FROM armies WHERE id = ?1"), params![id], row).optional()
}

/// An army by id, owned by `user`.
fn load_own(state: &AppState, user: &AuthUser, id: i64) -> ApiResult<ArmyRow> {
    let army = state.db.with(|c| load(c, id))?.ok_or_else(|| ApiError::not_found("no such army"))?;
    if army.user_id != user.id {
        return Err(ApiError::not_found("no such army"));
    }
    Ok(army)
}

fn summary(a: &ArmyRow) -> Value {
    json!({ "id": a.id, "name": a.name, "kind": a.kind, "faction_id": a.faction_id, "points": a.points, "updated_at": a.updated_at })
}

fn parse_def(body: &str) -> ApiResult<ArmyDef> {
    serde_json::from_str(body).map_err(|e| ApiError::internal(format!("stored army definition: {e}")))
}

/// The pack and its construction problems at `points_limit`.
pub struct Built {
    pub pack: ArmyPack,
    pub problems: Vec<String>,
}

/// Build the pack for a stored army and validate it. Validation problems are
/// reported, not errors; only unreadable data is an error.
pub async fn build(state: &AppState, army: &ArmyRow, points_limit: u32) -> ApiResult<Built> {
    match army.kind.as_str() {
        "def" => {
            let def = parse_def(&army.body)?;
            let entry = state.cache.catalogue(&def.faction_id).await?;
            let abilities = state.cache.abilities(&def.faction_id).await?;
            let problems = army_def::validate(&def, &entry.cat, points_limit, DP_BUDGET);
            let pack = army_def::to_pack(&def, &entry.cat, &abilities);
            Ok(Built { pack, problems })
        }
        _ => {
            let parsed = roster::parse_ros(&army.body).map_err(|e| ApiError::internal(format!("stored roster: {e}")))?;
            let slug = content::enrich::faction_slug(&parsed.faction);
            let abilities = state.cache.abilities(slug.unwrap_or("_core")).await?;
            let detachments = slug.and_then(|s| state.cache.detachments_json(s));
            let pack = content::pack::build_pack_with_detachments(&parsed, &abilities, detachments.as_deref());
            let mut problems = content::pack::validate(&pack, points_limit);
            if slug.is_none() {
                problems.push(format!("unknown faction {}; no ability data attached", parsed.faction));
            }
            Ok(Built { pack, problems })
        }
    }
}

async fn full(state: &AppState, army: &ArmyRow) -> ApiResult<Value> {
    let built = build(state, army, DEFAULT_POINTS_LIMIT).await?;
    let mut v = summary(army);
    if army.kind == "def" {
        v["def"] = serde_json::from_str(&army.body)?;
    }
    v["problems"] = json!(built.problems);
    Ok(v)
}

pub async fn list(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Value>> {
    let rows = state.db.with(|c| {
        let mut st = c.prepare(&format!("SELECT {COLS} FROM armies WHERE user_id = ?1 ORDER BY updated_at DESC, id DESC"))?;
        let rows = st.query_map(params![user.id], row)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
    })?;
    Ok(Json(rows.iter().map(summary).collect()))
}

#[derive(Deserialize)]
pub struct CreateBody {
    pub name: String,
    #[serde(default)]
    pub kind: Option<String>,
    pub def: ArmyDef,
}

/// Points and faction of a definition, checking the faction exists.
async fn def_meta(state: &AppState, def: &ArmyDef) -> ApiResult<(String, u32)> {
    if !state.cache.has_faction(&def.faction_id)? {
        return Err(ApiError::bad_request(format!("unknown faction {}", def.faction_id)));
    }
    let entry = state.cache.catalogue(&def.faction_id).await?;
    Ok((def.faction_id.clone(), army_def::total_points(def, &entry.cat)))
}

fn clean_name(name: &str) -> ApiResult<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(ApiError::bad_request("name must not be empty"));
    }
    Ok(name.to_string())
}

pub async fn create(State(state): State<AppState>, user: AuthUser, Json(body): Json<CreateBody>) -> ApiResult<(StatusCode, Json<Value>)> {
    if matches!(body.kind.as_deref(), Some(k) if k != "def") {
        return Err(ApiError::bad_request("kind must be \"def\"; use POST /api/armies/import for a .ros file"));
    }
    let name = clean_name(&body.name)?;
    let (faction_id, points) = def_meta(&state, &body.def).await?;
    let text = serde_json::to_string(&body.def)?;
    let id = state.db.with(|c| {
        c.execute("INSERT INTO armies (user_id, name, kind, faction_id, points, body) VALUES (?1, ?2, 'def', ?3, ?4, ?5)", params![user.id, name, faction_id, points, text])?;
        Ok(c.last_insert_rowid())
    })?;
    let army = load_own(&state, &user, id)?;
    Ok((StatusCode::CREATED, Json(full(&state, &army).await?)))
}

#[derive(Deserialize)]
pub struct ImportBody {
    pub name: String,
    pub ros: String,
}

pub async fn import(State(state): State<AppState>, user: AuthUser, Json(body): Json<ImportBody>) -> ApiResult<(StatusCode, Json<Value>)> {
    let name = clean_name(&body.name)?;
    // Scrub first, then parse the scrubbed text so the stored copy is what we read back.
    let scrubbed = roster::scrub::scrub(&body.ros).map_err(|e| ApiError::bad_request(format!("not a .ros file: {e}")))?;
    let parsed = roster::parse_ros(&scrubbed).map_err(|e| ApiError::bad_request(format!("cannot read roster: {e}")))?;
    let faction_id = content::enrich::faction_slug(&parsed.faction).map(str::to_string);
    let id = state.db.with(|c| {
        c.execute("INSERT INTO armies (user_id, name, kind, faction_id, points, body) VALUES (?1, ?2, 'ros', ?3, ?4, ?5)", params![user.id, name, faction_id, parsed.points, scrubbed])?;
        Ok(c.last_insert_rowid())
    })?;
    let army = load_own(&state, &user, id)?;
    Ok((StatusCode::CREATED, Json(full(&state, &army).await?)))
}

pub async fn get_one(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    let army = load_own(&state, &user, id)?;
    Ok(Json(full(&state, &army).await?))
}

#[derive(Deserialize)]
pub struct UpdateBody {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub def: Option<ArmyDef>,
}

pub async fn update(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>, Json(body): Json<UpdateBody>) -> ApiResult<Json<Value>> {
    let army = load_own(&state, &user, id)?;
    let name = match &body.name {
        Some(n) => clean_name(n)?,
        None => army.name.clone(),
    };
    match body.def {
        Some(def) => {
            if army.kind != "def" {
                return Err(ApiError::bad_request("an imported army has no definition to edit"));
            }
            let (faction_id, points) = def_meta(&state, &def).await?;
            let text = serde_json::to_string(&def)?;
            state.db.with(|c| {
                c.execute(
                    "UPDATE armies SET name = ?1, faction_id = ?2, points = ?3, body = ?4, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?5",
                    params![name, faction_id, points, text, id],
                )
            })?;
        }
        None => {
            state.db.with(|c| c.execute("UPDATE armies SET name = ?1, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?2", params![name, id]))?;
        }
    }
    let army = load_own(&state, &user, id)?;
    Ok(Json(full(&state, &army).await?))
}

pub async fn delete(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    load_own(&state, &user, id)?;
    let in_use: i64 = state.db.with(|c| {
        c.query_row("SELECT count(*) FROM game_players p JOIN games g ON g.id = p.game_id WHERE p.army_id = ?1 AND g.status <> 'finished'", params![id], |r| r.get(0))
    })?;
    if in_use > 0 {
        return Err(ApiError::conflict("army is in a game that has not finished"));
    }
    state.db.with(|c| c.execute("DELETE FROM armies WHERE id = ?1", params![id]))?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn pack(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>) -> ApiResult<Json<ArmyPack>> {
    let army = load_own(&state, &user, id)?;
    Ok(Json(build(&state, &army, DEFAULT_POINTS_LIMIT).await?.pack))
}
