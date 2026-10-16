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
use serde::Deserialize;
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Row as _};
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

/// The columns `row` reads, in order.
macro_rules! cols {
    () => {
        "id, user_id, name, kind, faction_id, points, body, updated_at"
    };
}

fn row(r: PgRow) -> sqlx::Result<ArmyRow> {
    Ok(ArmyRow {
        id: r.try_get(0)?,
        user_id: r.try_get(1)?,
        name: r.try_get(2)?,
        kind: r.try_get(3)?,
        faction_id: r.try_get(4)?,
        points: r.try_get::<i64, _>(5)?.max(0) as u32,
        body: r.try_get(6)?,
        updated_at: r.try_get(7)?,
    })
}

pub async fn load(db: &PgPool, id: i64) -> sqlx::Result<Option<ArmyRow>> {
    sqlx::query(concat!("SELECT ", cols!(), " FROM armies WHERE id = $1")).bind(id).try_map(row).fetch_optional(db).await
}

/// An army by id, owned by `user`.
async fn load_own(state: &AppState, user: &AuthUser, id: i64) -> ApiResult<ArmyRow> {
    let army = load(&state.db, id).await?.ok_or_else(|| ApiError::not_found("no such army"))?;
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
    let rows = sqlx::query(concat!("SELECT ", cols!(), " FROM armies WHERE user_id = $1 ORDER BY updated_at DESC, id DESC")).bind(user.id).try_map(row).fetch_all(&state.db).await?;
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
    let id: i64 = sqlx::query_scalar("INSERT INTO armies (user_id, name, kind, faction_id, points, body) VALUES ($1, $2, 'def', $3, $4, $5) RETURNING id")
        .bind(user.id)
        .bind(&name)
        .bind(&faction_id)
        .bind(points as i64)
        .bind(&text)
        .fetch_one(&state.db)
        .await?;
    let army = load_own(&state, &user, id).await?;
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
    let id: i64 = sqlx::query_scalar("INSERT INTO armies (user_id, name, kind, faction_id, points, body) VALUES ($1, $2, 'ros', $3, $4, $5) RETURNING id")
        .bind(user.id)
        .bind(&name)
        .bind(&faction_id)
        .bind(parsed.points as i64)
        .bind(&scrubbed)
        .fetch_one(&state.db)
        .await?;
    let army = load_own(&state, &user, id).await?;
    Ok((StatusCode::CREATED, Json(full(&state, &army).await?)))
}

pub async fn get_one(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    let army = load_own(&state, &user, id).await?;
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
    let army = load_own(&state, &user, id).await?;
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
            sqlx::query(concat!("UPDATE armies SET name = $1, faction_id = $2, points = $3, body = $4, updated_at = ", crate::now_sql!(), " WHERE id = $5"))
                .bind(&name)
                .bind(&faction_id)
                .bind(points as i64)
                .bind(&text)
                .bind(id)
                .execute(&state.db)
                .await?;
        }
        None => {
            sqlx::query(concat!("UPDATE armies SET name = $1, updated_at = ", crate::now_sql!(), " WHERE id = $2")).bind(&name).bind(id).execute(&state.db).await?;
        }
    }
    let army = load_own(&state, &user, id).await?;
    Ok(Json(full(&state, &army).await?))
}

pub async fn delete(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>) -> ApiResult<StatusCode> {
    load_own(&state, &user, id).await?;
    let in_use: i64 = sqlx::query_scalar("SELECT count(*) FROM game_players p JOIN games g ON g.id = p.game_id WHERE p.army_id = $1 AND g.status <> 'finished'")
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    if in_use > 0 {
        return Err(ApiError::conflict("army is in a game that has not finished"));
    }
    sqlx::query("DELETE FROM armies WHERE id = $1").bind(id).execute(&state.db).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn pack(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>) -> ApiResult<Json<ArmyPack>> {
    let army = load_own(&state, &user, id).await?;
    Ok(Json(build(&state, &army, DEFAULT_POINTS_LIMIT).await?.pack))
}
