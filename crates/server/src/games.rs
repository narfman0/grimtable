//! Lobbies, games and the WebSocket protocol. The engine decides every
//! action; this module only knows seats, persistence and fan-out.

use crate::armies;
use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::AppState;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use axum::Json;
use engine::combat::fight_owner_to_act;
use engine::content::EditionParams;
use engine::state::Phase;
use engine::{Action, GameState};
use rand::Rng as _;
use serde::Deserialize;
use sqlx::postgres::PgRow;
use sqlx::{PgPool, Row as _};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

// ---------------------------------------------------------------------------
// Rooms: per-game fan-out and an in-memory copy of the state.

/// Something every socket on a game should hear about.
#[derive(Clone)]
pub enum Event {
    /// The lobby changed; each socket re-reads the game for its own viewer.
    Lobby,
    /// A new state; each socket wraps it with its own seat.
    State(Arc<StateFrame>),
}

pub struct StateFrame {
    pub json: String,
    pub to_act: usize,
}

pub struct Room {
    pub tx: broadcast::Sender<Event>,
    /// Loaded from the database on first use after the game starts.
    pub state: tokio::sync::Mutex<Option<GameState>>,
}

#[derive(Default)]
pub struct Rooms(Mutex<HashMap<i64, Arc<Room>>>);

impl Rooms {
    pub fn get(&self, game_id: i64) -> Arc<Room> {
        let mut map = self.0.lock().unwrap_or_else(|e| e.into_inner());
        map.entry(game_id)
            .or_insert_with(|| {
                let (tx, _) = broadcast::channel(64);
                Arc::new(Room { tx, state: tokio::sync::Mutex::new(None) })
            })
            .clone()
    }
}

/// Whose decision the game is waiting on.
pub fn to_act(state: &GameState) -> usize {
    if state.phase == Phase::Fight {
        fight_owner_to_act(state).unwrap_or(state.active)
    } else {
        state.active
    }
}

// ---------------------------------------------------------------------------
// Rows and views.

#[derive(Clone, Debug)]
pub struct GameRow {
    pub id: i64,
    pub join_code: String,
    pub visibility: String,
    pub status: String,
    pub pattern: String,
    pub points_limit: u32,
    pub host_id: i64,
    pub state_json: Option<String>,
    pub winner: Option<i64>,
    pub created_at: String,
}

/// The columns `row` reads, in order.
macro_rules! cols {
    () => {
        "id, join_code, visibility, status, pattern, points_limit, host_id, state_json, winner, created_at"
    };
}

fn row(r: PgRow) -> sqlx::Result<GameRow> {
    Ok(GameRow {
        id: r.try_get(0)?,
        join_code: r.try_get(1)?,
        visibility: r.try_get(2)?,
        status: r.try_get(3)?,
        pattern: r.try_get(4)?,
        points_limit: r.try_get::<i64, _>(5)?.max(0) as u32,
        host_id: r.try_get(6)?,
        state_json: r.try_get(7)?,
        winner: r.try_get(8)?,
        created_at: r.try_get(9)?,
    })
}

async fn load(db: &PgPool, id: i64) -> sqlx::Result<Option<GameRow>> {
    sqlx::query(concat!("SELECT ", cols!(), " FROM games WHERE id = $1")).bind(id).try_map(row).fetch_optional(db).await
}

async fn load_or_404(state: &AppState, id: i64) -> ApiResult<GameRow> {
    load(&state.db, id).await?.ok_or_else(|| ApiError::not_found("no such game"))
}

#[derive(Clone, Debug)]
struct Seat {
    seat: usize,
    user_id: i64,
    username: String,
    army_id: i64,
    army_name: String,
    army_points: u32,
}

async fn seats(db: &PgPool, game_id: i64) -> sqlx::Result<Vec<Seat>> {
    sqlx::query(
        "SELECT p.seat, p.user_id, u.username, p.army_id, a.name, a.points FROM game_players p JOIN users u ON u.id = p.user_id JOIN armies a ON a.id = p.army_id WHERE p.game_id = $1 ORDER BY p.seat",
    )
    .bind(game_id)
    .try_map(|r: PgRow| {
        Ok(Seat {
            seat: r.try_get::<i64, _>(0)? as usize,
            user_id: r.try_get(1)?,
            username: r.try_get(2)?,
            army_id: r.try_get(3)?,
            army_name: r.try_get(4)?,
            army_points: r.try_get::<i64, _>(5)?.max(0) as u32,
        })
    })
    .fetch_all(db)
    .await
}

/// The game as `GET /api/games/:id` shows it to `viewer`. The join code is
/// only shown to the players; for everyone else it is null.
async fn view_in(db: &PgPool, game: &GameRow, viewer: Option<i64>) -> sqlx::Result<Value> {
    let seats = seats(db, game.id).await?;
    let my_seat = viewer.and_then(|v| seats.iter().find(|s| s.user_id == v).map(|s| s.seat));
    let host = seats.iter().find(|s| s.user_id == game.host_id).map(|s| s.username.clone());
    Ok(json!({
        "id": game.id,
        "join_code": if my_seat.is_some() { Value::String(game.join_code.clone()) } else { Value::Null },
        "visibility": game.visibility,
        "status": game.status,
        "pattern": game.pattern,
        "points_limit": game.points_limit,
        "host": host,
        "seats": seats.iter().map(|s| json!({ "seat": s.seat, "username": s.username, "army_id": s.army_id, "army_name": s.army_name, "army_points": s.army_points })).collect::<Vec<_>>(),
        "my_seat": my_seat,
        "winner": game.winner,
        "created_at": game.created_at,
    }))
}

async fn view(state: &AppState, id: i64, viewer: Option<i64>) -> ApiResult<Value> {
    let game = load_or_404(state, id).await?;
    Ok(view_in(&state.db, &game, viewer).await?)
}

async fn seat_of(state: &AppState, game_id: i64, user_id: i64) -> ApiResult<Option<usize>> {
    Ok(seats(&state.db, game_id).await?.iter().find(|s| s.user_id == user_id).map(|s| s.seat))
}

// ---------------------------------------------------------------------------
// HTTP handlers.

const CODE_ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

fn new_code() -> String {
    let mut rng = rand::rng();
    (0..6).map(|_| CODE_ALPHABET[rng.random_range(0..CODE_ALPHABET.len())] as char).collect()
}

fn default_visibility() -> String {
    "closed".into()
}
fn default_pattern() -> String {
    "dawn-of-war".into()
}
fn default_points() -> u32 {
    armies::DEFAULT_POINTS_LIMIT
}

#[derive(Deserialize)]
pub struct CreateBody {
    #[serde(default = "default_visibility")]
    pub visibility: String,
    #[serde(default = "default_pattern")]
    pub pattern: String,
    #[serde(default = "default_points")]
    pub points_limit: u32,
    pub army_id: i64,
}

/// An army the user owns, for taking a seat with.
async fn own_army(state: &AppState, user: &AuthUser, army_id: i64) -> ApiResult<armies::ArmyRow> {
    let army = armies::load(&state.db, army_id).await?.ok_or_else(|| ApiError::bad_request("no such army"))?;
    if army.user_id != user.id {
        return Err(ApiError::bad_request("no such army"));
    }
    Ok(army)
}

pub async fn create(State(state): State<AppState>, user: AuthUser, Json(body): Json<CreateBody>) -> ApiResult<(StatusCode, Json<Value>)> {
    if !matches!(body.visibility.as_str(), "open" | "closed") {
        return Err(ApiError::bad_request("visibility must be \"open\" or \"closed\""));
    }
    if body.points_limit == 0 {
        return Err(ApiError::bad_request("points_limit must be positive"));
    }
    content::mission::default_mission(&state.root, &body.pattern).map_err(|e| ApiError::bad_request(format!("{e:#}")))?;
    let army = own_army(&state, &user, body.army_id).await?;
    let mut tx = state.db.begin().await?;
    // A clash with an existing code is a unique violation that would abort
    // the transaction, so skip codes in use and insert only a free one.
    let id: i64 = loop {
        let code = new_code();
        let inserted: Option<i64> = sqlx::query_scalar(
            "INSERT INTO games (join_code, visibility, status, pattern, points_limit, host_id) VALUES ($1, $2, 'waiting', $3, $4, $5) ON CONFLICT (join_code) DO NOTHING RETURNING id",
        )
        .bind(&code)
        .bind(&body.visibility)
        .bind(&body.pattern)
        .bind(body.points_limit as i64)
        .bind(user.id)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(id) = inserted {
            break id;
        }
    };
    sqlx::query("INSERT INTO game_players (game_id, seat, user_id, army_id) VALUES ($1, 0, $2, $3)").bind(id).bind(user.id).bind(army.id).execute(&mut *tx).await?;
    tx.commit().await?;
    tracing::info!(game = id, host = %user.username, "game created");
    Ok((StatusCode::CREATED, Json(view(&state, id, Some(user.id)).await?)))
}

pub async fn open(State(state): State<AppState>, _user: AuthUser) -> ApiResult<Json<Value>> {
    let list = sqlx::query(
        "SELECT g.id, u.username, g.pattern, g.points_limit, g.created_at FROM games g JOIN users u ON u.id = g.host_id \
         WHERE g.visibility = 'open' AND g.status = 'waiting' AND NOT EXISTS (SELECT 1 FROM game_players p WHERE p.game_id = g.id AND p.seat = 1) \
         ORDER BY g.created_at DESC, g.id DESC",
    )
    .try_map(|r: PgRow| {
        Ok(json!({ "id": r.try_get::<i64, _>(0)?, "host": r.try_get::<String, _>(1)?, "pattern": r.try_get::<String, _>(2)?, "points_limit": r.try_get::<i64, _>(3)?, "created_at": r.try_get::<String, _>(4)? }))
    })
    .fetch_all(&state.db)
    .await?;
    Ok(Json(Value::Array(list)))
}

pub async fn mine(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Value>> {
    let ids: Vec<i64> = sqlx::query_scalar("SELECT g.id FROM games g JOIN game_players p ON p.game_id = g.id WHERE p.user_id = $1 ORDER BY g.updated_at DESC, g.id DESC")
        .bind(user.id)
        .fetch_all(&state.db)
        .await?;
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        out.push(view(&state, id, Some(user.id)).await?);
    }
    Ok(Json(Value::Array(out)))
}

pub async fn get_one(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    Ok(Json(view(&state, id, Some(user.id)).await?))
}

/// Take seat 1 of a waiting game.
async fn take_seat(state: &AppState, user: &AuthUser, game: &GameRow, army_id: i64) -> ApiResult<Value> {
    if game.status != "waiting" {
        return Err(ApiError::conflict("game has already started"));
    }
    if game.host_id == user.id {
        return Err(ApiError::conflict("you host this game"));
    }
    let army = own_army(state, user, army_id).await?;
    // The (game_id, seat) key settles two joins racing for the seat.
    let taken = sqlx::query("INSERT INTO game_players (game_id, seat, user_id, army_id) VALUES ($1, 1, $2, $3) ON CONFLICT DO NOTHING")
        .bind(game.id)
        .bind(user.id)
        .bind(army.id)
        .execute(&state.db)
        .await?
        .rows_affected()
        > 0;
    if !taken {
        return Err(ApiError::conflict("game is full"));
    }
    sqlx::query(concat!("UPDATE games SET updated_at = ", crate::now_sql!(), " WHERE id = $1")).bind(game.id).execute(&state.db).await?;
    tracing::info!(game = game.id, user = %user.username, "joined");
    let _ = state.rooms.get(game.id).tx.send(Event::Lobby);
    view(state, game.id, Some(user.id)).await
}

#[derive(Deserialize)]
pub struct JoinByCode {
    pub join_code: String,
    pub army_id: i64,
}

pub async fn join_by_code(State(state): State<AppState>, user: AuthUser, Json(body): Json<JoinByCode>) -> ApiResult<Json<Value>> {
    let code = body.join_code.trim().to_ascii_uppercase();
    let game = sqlx::query(concat!("SELECT ", cols!(), " FROM games WHERE join_code = $1"))
        .bind(&code)
        .try_map(row)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| ApiError::not_found("no game with that code"))?;
    Ok(Json(take_seat(&state, &user, &game, body.army_id).await?))
}

#[derive(Deserialize)]
pub struct JoinById {
    pub army_id: i64,
}

pub async fn join_by_id(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>, Json(body): Json<JoinById>) -> ApiResult<Json<Value>> {
    let game = load_or_404(&state, id).await?;
    if game.visibility != "open" {
        return Err(ApiError::forbidden("this game needs a join code"));
    }
    Ok(Json(take_seat(&state, &user, &game, body.army_id).await?))
}

/// Problems that stop a lobby from starting: over the points limit, or no
/// detachment. Everything else is reported and left to the players.
/// Leave a game. Waiting: the joiner frees seat 1; the host cancels the
/// game. Playing: the leaver concedes and the opponent wins.
pub async fn leave(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    let game = load_or_404(&state, id).await?;
    let Some(seat) = seat_of(&state, id, user.id).await? else {
        return Err(ApiError::forbidden("you are not seated in this game"));
    };
    match game.status.as_str() {
        "waiting" => {
            let cancelled = seat == 0;
            let mut tx = state.db.begin().await?;
            if cancelled {
                sqlx::query(concat!("UPDATE games SET status = 'cancelled', updated_at = ", crate::now_sql!(), " WHERE id = $1")).bind(id).execute(&mut *tx).await?;
                sqlx::query("DELETE FROM game_players WHERE game_id = $1").bind(id).execute(&mut *tx).await?;
            } else {
                sqlx::query("DELETE FROM game_players WHERE game_id = $1 AND seat = $2").bind(id).bind(seat as i64).execute(&mut *tx).await?;
                sqlx::query(concat!("UPDATE games SET updated_at = ", crate::now_sql!(), " WHERE id = $1")).bind(id).execute(&mut *tx).await?;
            }
            tx.commit().await?;
            tracing::info!(game = id, user = %user.username, cancelled, "left the lobby");
            let _ = state.rooms.get(id).tx.send(Event::Lobby);
            view(&state, id, Some(user.id)).await.map(Json)
        }
        "playing" => {
            let room = state.rooms.get(id);
            concede(&state, &room, id, seat, &user.username).await?;
            view(&state, id, Some(user.id)).await.map(Json)
        }
        other => Err(ApiError::conflict(&format!("the game is {other}"))),
    }
}

/// End a running game in the opponent's favour and tell every socket.
async fn concede(state: &AppState, room: &Room, game_id: i64, seat: usize, username: &str) -> ApiResult<()> {
    let mut cached = room.state.lock().await;
    if cached.is_none() {
        let game = load_or_404(state, game_id).await?;
        let text = game.state_json.ok_or_else(|| ApiError::conflict("the game has not started"))?;
        *cached = Some(serde_json::from_str(&text).map_err(|e| ApiError::internal(format!("stored game state: {e}")))?);
    }
    let gs = cached.as_mut().expect("loaded");
    let winner = 1 - seat;
    gs.phase = Phase::Ended;
    gs.winner = Some(Some(winner));
    let winner_name = gs.players[winner].name.clone();
    gs.log(Some(seat), format!("{username} conceded: {winner_name} wins"));
    let text = serde_json::to_string(&*gs)?;
    sqlx::query(concat!("UPDATE games SET state_json = $1, status = 'finished', winner = $2, updated_at = ", crate::now_sql!(), " WHERE id = $3"))
        .bind(&text)
        .bind(winner as i64)
        .bind(game_id)
        .execute(&state.db)
        .await?;
    tracing::info!(game = game_id, user = %username, winner, "conceded");
    let frame = Arc::new(StateFrame { to_act: to_act(gs), json: text });
    let _ = room.tx.send(Event::State(frame));
    Ok(())
}

fn blocks_start(problem: &str) -> bool {
    (problem.contains("over the") && problem.contains("limit")) || problem == "no detachment selected"
}

pub async fn start(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    let game = load_or_404(&state, id).await?;
    if game.host_id != user.id {
        return Err(ApiError::forbidden("only the host can start the game"));
    }
    if game.status != "waiting" {
        return Err(ApiError::conflict("game has already started"));
    }
    let seats = seats(&state.db, id).await?;
    if seats.len() != 2 {
        return Err(ApiError::conflict("waiting for a second player"));
    }
    let mut packs = Vec::with_capacity(2);
    let mut problems: Vec<Vec<String>> = Vec::with_capacity(2);
    for s in &seats {
        let army = armies::load(&state.db, s.army_id).await?.ok_or_else(|| ApiError::conflict(format!("{}'s army no longer exists", s.username)))?;
        let built = armies::build(&state, &army, game.points_limit).await?;
        packs.push(built.pack);
        problems.push(built.problems);
    }
    if problems.iter().flatten().any(|p| blocks_start(p)) {
        return Err(ApiError::conflict("an army is not legal for this battle").with_extra(json!({ "problems": problems })));
    }
    let mission = content::mission::default_mission(&state.root, &game.pattern)?;
    let seed: u64 = rand::rng().random();
    let p1 = packs.pop().expect("two packs");
    let p0 = packs.pop().expect("two packs");
    let gs = GameState::new([p0, p1], mission, EditionParams::default(), seed);
    let json_text = serde_json::to_string(&gs)?;
    let frame = Arc::new(StateFrame { json: json_text.clone(), to_act: to_act(&gs) });
    let room = state.rooms.get(id);
    {
        let mut cached = room.state.lock().await;
        sqlx::query(concat!("UPDATE games SET status = 'playing', seed = $1, state_json = $2, updated_at = ", crate::now_sql!(), " WHERE id = $3"))
            .bind(seed as i64)
            .bind(&json_text)
            .bind(id)
            .execute(&state.db)
            .await?;
        *cached = Some(gs);
    }
    tracing::info!(game = id, seed, "game started");
    let _ = room.tx.send(Event::State(frame));
    let mut v = view(&state, id, Some(user.id)).await?;
    v["problems"] = json!(problems);
    Ok(Json(v))
}

// ---------------------------------------------------------------------------
// WebSocket.

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientMsg {
    Action { action: Value },
    Ping,
}

fn state_text(frame: &StateFrame, seat: Option<usize>) -> String {
    let seat = seat.map(|s| s.to_string()).unwrap_or_else(|| "null".into());
    format!(r#"{{"type":"state","seat":{seat},"state":{},"to_act":{}}}"#, frame.json, frame.to_act)
}

fn error_text(message: &str) -> String {
    json!({ "type": "error", "message": message }).to_string()
}

/// The room's state, loading it from the database on first use. `None`
/// while the game is still waiting.
async fn load_room_state(state: &AppState, room: &Room, game_id: i64) -> ApiResult<Option<StateFrame>> {
    let mut cached = room.state.lock().await;
    if cached.is_none() {
        let game = load_or_404(state, game_id).await?;
        let Some(text) = game.state_json else { return Ok(None) };
        let gs: GameState = serde_json::from_str(&text).map_err(|e| ApiError::internal(format!("stored game state: {e}")))?;
        let frame = StateFrame { to_act: to_act(&gs), json: text };
        *cached = Some(gs);
        return Ok(Some(frame));
    }
    let gs = cached.as_ref().expect("checked");
    Ok(Some(StateFrame { json: serde_json::to_string(gs)?, to_act: to_act(gs) }))
}

/// Apply one action for `seat`, persist it and tell everyone. The error
/// string is what the sender sees.
async fn apply_action(state: &AppState, room: &Room, game_id: i64, seat: Option<usize>, action: Value) -> Result<(), String> {
    let seat = seat.ok_or_else(|| "spectators cannot act".to_string())?;
    let action: Action = serde_json::from_value(action).map_err(|e| format!("bad action: {e}"))?;
    let action_json = serde_json::to_string(&action).map_err(|e| e.to_string())?;
    let mut cached = room.state.lock().await;
    if cached.is_none() {
        let game = load(&state.db, game_id).await.map_err(|e| ApiError::from(e).message)?.ok_or("no such game")?;
        let text = game.state_json.ok_or("the game has not started")?;
        *cached = Some(serde_json::from_str(&text).map_err(|e| format!("stored game state: {e}"))?);
    }
    let current = cached.as_mut().expect("loaded");
    let mut next = current.clone();
    engine::apply(&mut next, seat, action).map_err(|e| e.0)?;
    let text = serde_json::to_string(&next).map_err(|e| e.to_string())?;
    let finished = next.phase == Phase::Ended;
    let winner = next.winner.flatten().map(|w| w as i64);
    persist_action(&state.db, game_id, seat, &text, finished, winner, &action_json).await.map_err(|e| {
        tracing::error!(game = game_id, "persisting action: {e}");
        "could not save the action".to_string()
    })?;
    let frame = Arc::new(StateFrame { to_act: to_act(&next), json: text });
    *current = next;
    if finished {
        tracing::info!(game = game_id, ?winner, "game finished");
    }
    let _ = room.tx.send(Event::State(frame));
    Ok(())
}

/// The new state and the action that made it, in one transaction.
async fn persist_action(db: &PgPool, game_id: i64, seat: usize, state_json: &str, finished: bool, winner: Option<i64>, action_json: &str) -> sqlx::Result<()> {
    let mut tx = db.begin().await?;
    sqlx::query(concat!("UPDATE games SET state_json = $1, status = CASE WHEN $2 THEN 'finished' ELSE status END, winner = $3, updated_at = ", crate::now_sql!(), " WHERE id = $4"))
        .bind(state_json)
        .bind(finished)
        .bind(winner)
        .bind(game_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO game_actions (game_id, seq, seat, action_json) VALUES ($1, (SELECT COALESCE(MAX(seq), 0) + 1 FROM game_actions WHERE game_id = $1), $2, $3)")
        .bind(game_id)
        .bind(seat as i64)
        .bind(action_json)
        .execute(&mut *tx)
        .await?;
    tx.commit().await
}

pub async fn ws(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>, upgrade: WebSocketUpgrade) -> ApiResult<Response> {
    load_or_404(&state, id).await?;
    let seat = seat_of(&state, id, user.id).await?;
    Ok(upgrade.on_upgrade(move |socket| handle_socket(socket, state, id, user, seat)))
}

async fn send(socket: &mut WebSocket, text: String) -> bool {
    socket.send(Message::Text(text.into())).await.is_ok()
}

async fn lobby_text(state: &AppState, game_id: i64, viewer: i64) -> Option<String> {
    view(state, game_id, Some(viewer)).await.ok().map(|g| json!({ "type": "lobby", "game": g }).to_string())
}

async fn handle_socket(mut socket: WebSocket, state: AppState, game_id: i64, user: AuthUser, seat: Option<usize>) {
    let room = state.rooms.get(game_id);
    // Subscribe before the first read so nothing between them is missed.
    let mut rx = room.tx.subscribe();
    tracing::debug!(game = game_id, user = %user.username, ?seat, "socket connected");

    let first = match load_room_state(&state, &room, game_id).await {
        Ok(Some(frame)) => state_text(&frame, seat),
        Ok(None) => match lobby_text(&state, game_id, user.id).await {
            Some(t) => t,
            None => return,
        },
        Err(e) => error_text(&e.message),
    };
    if !send(&mut socket, first).await {
        return;
    }

    loop {
        tokio::select! {
            incoming = socket.recv() => {
                let text = match incoming {
                    Some(Ok(Message::Text(t))) => t.to_string(),
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    Some(Ok(_)) => continue,
                };
                let reply = match serde_json::from_str::<ClientMsg>(&text) {
                    Ok(ClientMsg::Ping) => Some(json!({ "type": "pong" }).to_string()),
                    Ok(ClientMsg::Action { action }) => apply_action(&state, &room, game_id, seat, action).await.err().map(|m| error_text(&m)),
                    Err(e) => Some(error_text(&format!("bad message: {e}"))),
                };
                if let Some(reply) = reply {
                    if !send(&mut socket, reply).await {
                        break;
                    }
                }
            }
            event = rx.recv() => {
                let text = match event {
                    Ok(Event::State(frame)) => state_text(&frame, seat),
                    Ok(Event::Lobby) => match lobby_text(&state, game_id, user.id).await {
                        Some(t) => t,
                        None => continue,
                    },
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                };
                if !send(&mut socket, text).await {
                    break;
                }
            }
        }
    }
    tracing::debug!(game = game_id, user = %user.username, "socket closed");
}
