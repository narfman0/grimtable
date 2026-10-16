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
use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;
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

const COLS: &str = "id, join_code, visibility, status, pattern, points_limit, host_id, state_json, winner, created_at";

fn row(r: &rusqlite::Row) -> rusqlite::Result<GameRow> {
    Ok(GameRow {
        id: r.get(0)?,
        join_code: r.get(1)?,
        visibility: r.get(2)?,
        status: r.get(3)?,
        pattern: r.get(4)?,
        points_limit: r.get::<_, i64>(5)?.max(0) as u32,
        host_id: r.get(6)?,
        state_json: r.get(7)?,
        winner: r.get(8)?,
        created_at: r.get(9)?,
    })
}

fn load(c: &Connection, id: i64) -> rusqlite::Result<Option<GameRow>> {
    c.query_row(&format!("SELECT {COLS} FROM games WHERE id = ?1"), params![id], row).optional()
}

fn load_or_404(state: &AppState, id: i64) -> ApiResult<GameRow> {
    state.db.with(|c| load(c, id))?.ok_or_else(|| ApiError::not_found("no such game"))
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

fn seats(c: &Connection, game_id: i64) -> rusqlite::Result<Vec<Seat>> {
    let mut st = c.prepare(
        "SELECT p.seat, p.user_id, u.username, p.army_id, a.name, a.points FROM game_players p JOIN users u ON u.id = p.user_id JOIN armies a ON a.id = p.army_id WHERE p.game_id = ?1 ORDER BY p.seat",
    )?;
    let rows = st.query_map(params![game_id], |r| {
        Ok(Seat { seat: r.get::<_, i64>(0)? as usize, user_id: r.get(1)?, username: r.get(2)?, army_id: r.get(3)?, army_name: r.get(4)?, army_points: r.get::<_, i64>(5)?.max(0) as u32 })
    })?;
    rows.collect()
}

/// The game as `GET /api/games/:id` shows it to `viewer`. The join code is
/// only shown to the players; for everyone else it is null.
fn view_in(c: &Connection, game: &GameRow, viewer: Option<i64>) -> rusqlite::Result<Value> {
    let seats = seats(c, game.id)?;
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

fn view(state: &AppState, id: i64, viewer: Option<i64>) -> ApiResult<Value> {
    let game = load_or_404(state, id)?;
    state.db.with(|c| view_in(c, &game, viewer))
}

fn seat_of(state: &AppState, game_id: i64, user_id: i64) -> ApiResult<Option<usize>> {
    state.db.with(|c| seats(c, game_id)).map(|s| s.iter().find(|s| s.user_id == user_id).map(|s| s.seat))
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
fn own_army(state: &AppState, user: &AuthUser, army_id: i64) -> ApiResult<armies::ArmyRow> {
    let army = state.db.with(|c| armies::load(c, army_id))?.ok_or_else(|| ApiError::bad_request("no such army"))?;
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
    let army = own_army(&state, &user, body.army_id)?;
    let id = state.db.with(|c| {
        let tx = c.transaction()?;
        let code = loop {
            let code = new_code();
            let taken: i64 = tx.query_row("SELECT count(*) FROM games WHERE join_code = ?1", params![code], |r| r.get(0))?;
            if taken == 0 {
                break code;
            }
        };
        tx.execute(
            "INSERT INTO games (join_code, visibility, status, pattern, points_limit, host_id) VALUES (?1, ?2, 'waiting', ?3, ?4, ?5)",
            params![code, body.visibility, body.pattern, body.points_limit, user.id],
        )?;
        let id = tx.last_insert_rowid();
        tx.execute("INSERT INTO game_players (game_id, seat, user_id, army_id) VALUES (?1, 0, ?2, ?3)", params![id, user.id, army.id])?;
        tx.commit()?;
        Ok(id)
    })?;
    tracing::info!(game = id, host = %user.username, "game created");
    Ok((StatusCode::CREATED, Json(view(&state, id, Some(user.id))?)))
}

pub async fn open(State(state): State<AppState>, _user: AuthUser) -> ApiResult<Json<Value>> {
    let list = state.db.with(|c| {
        let mut st = c.prepare(
            "SELECT g.id, u.username, g.pattern, g.points_limit, g.created_at FROM games g JOIN users u ON u.id = g.host_id \
             WHERE g.visibility = 'open' AND g.status = 'waiting' AND NOT EXISTS (SELECT 1 FROM game_players p WHERE p.game_id = g.id AND p.seat = 1) \
             ORDER BY g.created_at DESC, g.id DESC",
        )?;
        let rows = st.query_map([], |r| {
            Ok(json!({ "id": r.get::<_, i64>(0)?, "host": r.get::<_, String>(1)?, "pattern": r.get::<_, String>(2)?, "points_limit": r.get::<_, i64>(3)?, "created_at": r.get::<_, String>(4)? }))
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
    })?;
    Ok(Json(Value::Array(list)))
}

pub async fn mine(State(state): State<AppState>, user: AuthUser) -> ApiResult<Json<Value>> {
    let ids = state.db.with(|c| {
        let mut st = c.prepare("SELECT g.id FROM games g JOIN game_players p ON p.game_id = g.id WHERE p.user_id = ?1 ORDER BY g.updated_at DESC, g.id DESC")?;
        let rows = st.query_map(params![user.id], |r| r.get::<_, i64>(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
    })?;
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        out.push(view(&state, id, Some(user.id))?);
    }
    Ok(Json(Value::Array(out)))
}

pub async fn get_one(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    Ok(Json(view(&state, id, Some(user.id))?))
}

/// Take seat 1 of a waiting game.
fn take_seat(state: &AppState, user: &AuthUser, game: &GameRow, army_id: i64) -> ApiResult<Value> {
    if game.status != "waiting" {
        return Err(ApiError::conflict("game has already started"));
    }
    if game.host_id == user.id {
        return Err(ApiError::conflict("you host this game"));
    }
    let army = own_army(state, user, army_id)?;
    let taken = state.db.with(|c| {
        let full: i64 = c.query_row("SELECT count(*) FROM game_players WHERE game_id = ?1 AND seat = 1", params![game.id], |r| r.get(0))?;
        if full > 0 {
            return Ok(false);
        }
        c.execute("INSERT INTO game_players (game_id, seat, user_id, army_id) VALUES (?1, 1, ?2, ?3)", params![game.id, user.id, army.id])?;
        c.execute("UPDATE games SET updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?1", params![game.id])?;
        Ok(true)
    })?;
    if !taken {
        return Err(ApiError::conflict("game is full"));
    }
    tracing::info!(game = game.id, user = %user.username, "joined");
    let _ = state.rooms.get(game.id).tx.send(Event::Lobby);
    view(state, game.id, Some(user.id))
}

#[derive(Deserialize)]
pub struct JoinByCode {
    pub join_code: String,
    pub army_id: i64,
}

pub async fn join_by_code(State(state): State<AppState>, user: AuthUser, Json(body): Json<JoinByCode>) -> ApiResult<Json<Value>> {
    let code = body.join_code.trim().to_ascii_uppercase();
    let game = state
        .db
        .with(|c| c.query_row(&format!("SELECT {COLS} FROM games WHERE join_code = ?1"), params![code], row).optional())?
        .ok_or_else(|| ApiError::not_found("no game with that code"))?;
    Ok(Json(take_seat(&state, &user, &game, body.army_id)?))
}

#[derive(Deserialize)]
pub struct JoinById {
    pub army_id: i64,
}

pub async fn join_by_id(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>, Json(body): Json<JoinById>) -> ApiResult<Json<Value>> {
    let game = load_or_404(&state, id)?;
    if game.visibility != "open" {
        return Err(ApiError::forbidden("this game needs a join code"));
    }
    Ok(Json(take_seat(&state, &user, &game, body.army_id)?))
}

/// Problems that stop a lobby from starting: over the points limit, or no
/// detachment. Everything else is reported and left to the players.
fn blocks_start(problem: &str) -> bool {
    (problem.contains("over the") && problem.contains("limit")) || problem == "no detachment selected"
}

pub async fn start(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>) -> ApiResult<Json<Value>> {
    let game = load_or_404(&state, id)?;
    if game.host_id != user.id {
        return Err(ApiError::forbidden("only the host can start the game"));
    }
    if game.status != "waiting" {
        return Err(ApiError::conflict("game has already started"));
    }
    let seats = state.db.with(|c| seats(c, id))?;
    if seats.len() != 2 {
        return Err(ApiError::conflict("waiting for a second player"));
    }
    let mut packs = Vec::with_capacity(2);
    let mut problems: Vec<Vec<String>> = Vec::with_capacity(2);
    for s in &seats {
        let army = state.db.with(|c| armies::load(c, s.army_id))?.ok_or_else(|| ApiError::conflict(format!("{}'s army no longer exists", s.username)))?;
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
        state.db.with(|c| {
            c.execute(
                "UPDATE games SET status = 'playing', seed = ?1, state_json = ?2, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?3",
                params![seed as i64, json_text, id],
            )
        })?;
        *cached = Some(gs);
    }
    tracing::info!(game = id, seed, "game started");
    let _ = room.tx.send(Event::State(frame));
    let mut v = view(&state, id, Some(user.id))?;
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
        let game = load_or_404(state, game_id)?;
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
        let game = state.db.with(|c| load(c, game_id)).map_err(|e| e.message)?.ok_or("no such game")?;
        let text = game.state_json.ok_or("the game has not started")?;
        *cached = Some(serde_json::from_str(&text).map_err(|e| format!("stored game state: {e}"))?);
    }
    let current = cached.as_mut().expect("loaded");
    let mut next = current.clone();
    engine::apply(&mut next, seat, action).map_err(|e| e.0)?;
    let text = serde_json::to_string(&next).map_err(|e| e.to_string())?;
    let finished = next.phase == Phase::Ended;
    let winner = next.winner.flatten().map(|w| w as i64);
    state
        .db
        .with(|c| {
            let tx = c.transaction()?;
            tx.execute(
                "UPDATE games SET state_json = ?1, status = CASE WHEN ?2 THEN 'finished' ELSE status END, winner = ?3, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?4",
                params![text, finished, winner, game_id],
            )?;
            tx.execute(
                "INSERT INTO game_actions (game_id, seq, seat, action_json) VALUES (?1, (SELECT COALESCE(MAX(seq), 0) + 1 FROM game_actions WHERE game_id = ?1), ?2, ?3)",
                params![game_id, seat as i64, action_json],
            )?;
            tx.commit()
        })
        .map_err(|e| {
            tracing::error!(game = game_id, "persisting action: {}", e.message);
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

pub async fn ws(State(state): State<AppState>, user: AuthUser, Path(id): Path<i64>, upgrade: WebSocketUpgrade) -> ApiResult<Response> {
    load_or_404(&state, id)?;
    let seat = seat_of(&state, id, user.id)?;
    Ok(upgrade.on_upgrade(move |socket| handle_socket(socket, state, id, user, seat)))
}

async fn send(socket: &mut WebSocket, text: String) -> bool {
    socket.send(Message::Text(text.into())).await.is_ok()
}

async fn lobby_text(state: &AppState, game_id: i64, viewer: i64) -> Option<String> {
    view(state, game_id, Some(viewer)).ok().map(|g| json!({ "type": "lobby", "game": g }).to_string())
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
