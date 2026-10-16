# Multiplayer contract

Shared by the server, the client and the content crate. Change it here first.

## Accounts

- Username and password. Passwords hashed with argon2. A random session
  token in an `HttpOnly` cookie `grimtable_session`, stored server-side.
- Sessions expire after 30 days without use (`GRIMTABLE_SESSION_DAYS`
  overrides). Any request in the last day of a session's life renews it;
  an expired session answers 401 `{"error":"session expired"}` and is
  deleted. The cookie carries a matching `Max-Age`.
- `POST /api/register {username, password}` → 201 `{id, username}`; 409 taken.
- `POST /api/login {username, password}` → 200 `{id, username}`; 401.
- `POST /api/logout` → 204. `GET /api/me` → `{id, username}` or 401.

## Armies

An army is either an editor-built definition (`kind: "def"`, body is an
`ArmyDef`, see `crates/content/src/army_def.rs`) or an imported New Recruit
export (`kind: "ros"`, body is the scrubbed `.ros` XML). Both produce an
engine `ArmyPack` on the server.

- `GET /api/armies` → `[{id, name, kind, faction_id, points, updated_at}]` (own armies).
- `POST /api/armies {name, kind:"def", def: ArmyDef}` → 201 full record.
- `POST /api/armies/import {name, ros: "<xml>"}` → 201 record (`kind: "ros"`); server scrubs prose before storing.
- `GET /api/armies/:id` → `{id, name, kind, faction_id, points, def?, problems: [string]}`.
- `PUT /api/armies/:id {name?, def?}` → 200 record. `DELETE /api/armies/:id` → 204.
- `GET /api/armies/:id/pack` → the `ArmyPack` JSON (for preview).
- `GET /api/catalogue` → `[{faction_id, name}]`. `GET /api/catalogue/:faction_id` → `Catalogue` JSON (see `catalogue.rs`): datasheets with stats, points by size, model rows with loadout variants, wargear options, weapons, enhancements, detachments with detachment points, leader eligibility. Numbers, names and keywords only.

Validation (`content::army_def::validate`) returns a list of problems; the
server stores an army even with problems and reports them. A lobby refuses to
start with an army over the points limit or without a detachment.

## Lobbies and games

- `POST /api/games {visibility: "open"|"closed", pattern: "dawn-of-war", points_limit: 2000, army_id}` → 201 `{id, join_code, ...}`; the creator takes seat 0.
- `GET /api/games/open` → open games waiting for a player: `[{id, host, pattern, points_limit, created_at}]`.
- `POST /api/games/join {join_code, army_id}` → 200 game; takes seat 1. Also `POST /api/games/:id/join {army_id}` for open games.
- `GET /api/games/:id` → `{id, join_code, visibility, status: "waiting"|"playing"|"finished", pattern, points_limit, seats: [{seat, username, army_name, army_points}], my_seat, winner}`.
- `POST /api/games/:id/start` (host, both seats filled) → builds both packs, seeds from OS randomness, `GameState::new`, status `playing`.
- `POST /api/games/:id/leave` → 200 game view. In a waiting lobby the joiner frees seat 1 and the lobby is open again; the host cancels the game (status `cancelled`). In a playing game the leaver concedes: status `finished`, the opponent is the winner, the engine state is ended with a log line, and every socket receives the final `state`. 403 when not seated, 409 when already finished or cancelled.
- `GET /api/games/mine` → games the user is in.
- `GET /api/games/:id/ws` → WebSocket, cookie-authenticated.

### WebSocket protocol (JSON text frames)

Client → server:
- `{"type":"action","action":{...engine Action JSON...}}`
- `{"type":"ping"}`

Server → client:
- `{"type":"state","seat":0|1|null,"state":{...GameState JSON...},"to_act":0|1}` on connect, after every accepted action, and whenever the other player acts. `seat` is `null` for spectators.
- `{"type":"error","message":"<rule error text>"}` for a refused action; state unchanged.
- `{"type":"lobby","game":{...as GET /api/games/:id...}}` while waiting.
- `{"type":"pong"}`

The server applies `engine::actions::apply(state, seat, action)`; the seat is
the authenticated player's, never taken from the message. Every accepted
action is appended to `game_actions` (game_id, seq, seat, action JSON) and
the state is persisted, so a game can be replayed from seed plus actions.

## Roll-off

Unchanged from the engine: when both players finish muster the engine rolls
off, the winner deploys first and takes the first turn.

## Deferred

- CPU opponent: a seat held by the server that picks actions. Needs a
  legal-move generator in the engine first.
