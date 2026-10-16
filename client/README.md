# client

Browser client for grimtable: online two-player battles with every rule
enforced by the Rust engine (on the server, and compiled to WebAssembly in
the browser for read-only queries), plus a local hotseat mode. Plain
TypeScript and Vite, no framework; the board is a canvas at true scale.

## Run

From the repository root:

```sh
./scripts/sync-data.sh                                            # runtime data -> client/public/data (gitignored)
wasm-pack build crates/wasm --target web --out-dir ../../client/src/wasm   # engine -> client/src/wasm (gitignored)
cd client && npm install
```

Against the real server (serves `/api` on `127.0.0.1:8080`):

```sh
cargo run -p server          # from the repo root, in one terminal
npm run dev                  # http://localhost:5173; Vite proxies /api (and the WebSocket) to :8080
```

`npm run build` type-checks and bundles to `client/dist`, which the server
also serves directly.

### Mock server

`client/scripts/mock-server.mjs` implements the contract in
`docs/multiplayer.md` in memory (accounts, armies, catalogues, lobbies, and
games driven by the wasm engine) so the client can be developed without the
Rust server. It reads catalogues from `public/data/catalogue/<faction>.json`;
generate them once per faction:

```sh
for f in adeptus-mechanicus adeptus-custodes necrons; do
  cargo run -p content -- catalogue $f > client/public/data/catalogue/$f.json
done
```

Then `npm run mock` (port 8080, or `PORT=...`) and `npm run dev`. `.ros`
imports are built with the wasm pack builder; editor armies use a JS port of
`army_def.rs` (`scripts/army-def.mjs`). Nothing is persisted.

### Scripts

- `npm run smoke`: plays round 1 of Mechanicus vs Necrons in Node against the wasm build.
- `npm run e2e`: spawns the mock and drives the contract end to end: register two
  accounts, build an editor army, import the Necron fixture, create a closed
  game, join by code, start, both sockets receive state, muster to deployment.
  `E2E_BASE=http://127.0.0.1:8080 npm run e2e` runs the same against a
  server that is already up (the real one with `GRIMTABLE_DB=/tmp/e2e.db`).
- `npm run ui-smoke` (after `npm run build`): loads the built bundle in jsdom
  against the mock and clicks through login, home, the editor, lobby, muster
  and battle, then the hotseat path.

## Screens

1. **Login / register**: username and password; `GET /api/me` restores the
   session cookie on load. "Local hotseat" works without an account.
2. **Home**: my armies (name, faction, points, editor/imported badge, problem
   count, pack preview, delete), New army (faction picker) and Import .ros;
   my games with Open/Resume; create a game (open or closed, deployment
   pattern, points limit, army); join by six-character code; open lobbies.
3. **Army editor** (editor armies, `kind: "def"`): datasheet browser with
   search, grouped by role, showing points tiers; click adds a unit with
   minimum models and default weapons. The list has detachments (multi-select
   with detachment points against a budget of 3 for Strike Force or 2 for
   Incursion), army name, per-unit model-row counts clamped to the datasheet,
   a per-row weapon loadout editor (defaults, loadout variants, wargear
   replacements, with a compact profile), enhancements from the chosen
   detachments with cost, a single Warlord, duplicate/remove, running points
   against the limit. Save posts to the server and shows its validation
   problems; Pack preview fetches the engine pack.
4. **Lobby**: join code (copyable), visibility, pattern, both seats with army
   and points, Start for the host once both seats are filled (army problems
   that block or accompany the start are listed), Leave. The game socket
   delivers lobby updates and then the first state.
5. **Muster**: attach Leaders and Support units to the units they can lead;
   online, only your own army is editable and both players may finish at any
   time; the engine rolls off for first turn.
6. **Battle**: board on the left (wheel zooms, drag pans, click a model to
   select its unit), phase panel on the right. Every engine action is
   reachable: deploy / reserve / embark, scout moves, normal / advance / fall
   back / stationary moves, arriving from reserve, disembarking, shooting with
   per-weapon targets (alternative profiles as radio buttons), charge
   declaration and the charge move, fights with optional pile-in and
   consolidation, army-rule choices, and ending the phase. Online, actions go
   to the server as `{"type":"action",...}` and the answer is the next `state`
   or an `error` toast; a local wasm mirror restored from every state keeps
   buffs, visibility and fight order available. The panel only offers actions
   when it is your seat's turn and shows the opponent and connection status.

The placement tool places a whole unit in a blob, line or column around the
click (or one model at a time), lets you drag single models, clamps them to
the board and to the move limit ring, and nudges overlapping bases apart.

Hotseat games are written to `localStorage` after every action and can be
resumed from the hotseat setup screen; online games live on the server.

## Data

Everything the client fetches lives under `public/data` (never committed):
numbers, keywords and effect trees only. Powered by 40kdc-data and BSData.
