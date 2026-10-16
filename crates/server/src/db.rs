//! SQLite storage: one connection behind a mutex, schema created at startup.
//! Nothing here awaits while holding the lock, so a `std` mutex is enough.

use crate::error::{ApiError, ApiResult};
use rusqlite::Connection;
use std::path::Path;
use std::sync::Mutex;

pub struct Db(Mutex<Connection>);

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS users (
    id            INTEGER PRIMARY KEY,
    username      TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    created_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);
CREATE TABLE IF NOT EXISTS sessions (
    token      TEXT PRIMARY KEY,
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);
CREATE TABLE IF NOT EXISTS armies (
    id         INTEGER PRIMARY KEY,
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    kind       TEXT NOT NULL CHECK (kind IN ('def', 'ros')),
    faction_id TEXT,
    points     INTEGER NOT NULL DEFAULT 0,
    body       TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);
CREATE INDEX IF NOT EXISTS armies_user ON armies(user_id);
CREATE TABLE IF NOT EXISTS games (
    id           INTEGER PRIMARY KEY,
    join_code    TEXT NOT NULL UNIQUE,
    visibility   TEXT NOT NULL CHECK (visibility IN ('open', 'closed')),
    status       TEXT NOT NULL CHECK (status IN ('waiting', 'playing', 'finished')),
    pattern      TEXT NOT NULL,
    points_limit INTEGER NOT NULL,
    host_id      INTEGER NOT NULL REFERENCES users(id),
    seed         INTEGER,
    state_json   TEXT,
    winner       INTEGER,
    created_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);
CREATE TABLE IF NOT EXISTS game_players (
    game_id INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    seat    INTEGER NOT NULL CHECK (seat IN (0, 1)),
    user_id INTEGER NOT NULL REFERENCES users(id),
    army_id INTEGER NOT NULL REFERENCES armies(id),
    PRIMARY KEY (game_id, seat)
);
CREATE TABLE IF NOT EXISTS game_actions (
    game_id     INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    seq         INTEGER NOT NULL,
    seat        INTEGER NOT NULL,
    action_json TEXT NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    PRIMARY KEY (game_id, seq)
);
"#;

impl Db {
    pub fn open(path: &Path) -> anyhow::Result<Db> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Db(Mutex::new(conn)))
    }

    /// Run `f` with the connection. Keep it synchronous: no awaits inside.
    pub fn with<T>(&self, f: impl FnOnce(&mut Connection) -> rusqlite::Result<T>) -> ApiResult<T> {
        let mut conn = self.0.lock().map_err(|_| ApiError::internal("database lock poisoned"))?;
        f(&mut conn).map_err(ApiError::from)
    }
}
