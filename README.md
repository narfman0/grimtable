# grimtable

A rules-enforcing engine and web client for playing the 11th edition of a certain
grimdark tabletop wargame. Rust engine, Rust server, TypeScript client. The engine
compiles to WebAssembly so the browser runs the same rules the server enforces.

**Status:** playable online. Sign in, build an army in the editor or import a
New Recruit export, open a lobby or join one by code, and play a full
five-round battle against another player, with the engine enforcing
deployment, movement, shooting, charges, fights, battle-shock, objectives and
scoring. Abilities the engine cannot apply yet are listed per unit so the
players apply them by hand. See [docs/design.md](docs/design.md) for the
architecture and its simplifications and [docs/roadmap.md](docs/roadmap.md) for
what is built and what is next.

![Movement phase: the Mechanicus and Necron fixture lists on a Dawn of War board](docs/screenshot-movement.png)

## Layout

| Path | What |
|---|---|
| `crates/engine` | Pure rules engine. No I/O. Game state, hook vocabulary, dice, attack sequence. |
| `crates/content` | Build step: joins the two data submodules into a content pack the engine loads. |
| `crates/roster` | Parses New Recruit / BattleScribe `.ros` exports; scrubs prose from fixtures. |
| `crates/wasm` | WebAssembly bindings: the browser runs the same engine. |
| `crates/server` | Authoritative game server (axum, Postgres): accounts, armies, lobbies, WebSocket games. |
| `client/` | Hotseat browser client (TypeScript, Vite, canvas). |
| `crates/roster/fixtures` | Three real lists, scrubbed to numbers, names and keywords. |
| `data/40kdc-data` | Submodule: [our fork](https://github.com/narfman0/40kdc-data) of [40kdc-data](https://github.com/Tabletop-Developer-Consortium/40kdc-data), the ability effect DSL and mission data. |
| `data/wh40k-11e` | Submodule: [BSData wh40k-11e](https://github.com/BSData/wh40k-11e), stats and points. |

## Build and play

```sh
git submodule update --init
cargo test --workspace
cargo run -p content -- report                 # points disagreements between the data sources
cargo run -p content -- pack crates/roster/fixtures/mechanicus.ros   # a content pack as JSON

# the browser client and the server
./scripts/sync-data.sh
wasm-pack build crates/wasm --target web --out-dir ../../client/src/wasm
podman run -d --name grimtable-pg -e POSTGRES_PASSWORD=postgres -p 127.0.0.1:5432:5432 postgres:18
DATABASE_URL=postgres://postgres:postgres@127.0.0.1/postgres cargo run -p server   # http://127.0.0.1:8080
cd client && npm install && npm run dev        # http://localhost:5173, proxies /api to the server
```

Register, build an army in the editor (or import a `.ros` export), create a
game and share its join code, or pick an open lobby. Both players attach
leaders at muster, the engine rolls off for first turn, then deploy and play.
The log explains every roll in plain words. A local hotseat mode is still
there for playing on one screen.

Server tests need Postgres too: `GRIMTABLE_TEST_DATABASE_URL=postgres://…
cargo test -p server` (each test uses a schema of its own). Deploying:
[docs/deploy.md](docs/deploy.md).

## Content and licensing

Nothing in this repository is official. The engine, server and client are MIT.
The content pack is never committed: it is built from the pinned submodules, and
it contains numbers, keywords and structured effect data only. No rules prose, no
artwork, no official wording.

Powered by [40kdc-data](https://github.com/Tabletop-Developer-Consortium/40kdc-data)
(data CC BY 4.0, schemas CC0) and the [BSData](https://github.com/BSData) community.
