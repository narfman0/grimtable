# client

Browser client for grimtable: a hotseat (one browser, two players taking
turns) battle with every rule enforced by the Rust engine compiled to
WebAssembly. Plain TypeScript and Vite, no framework; the board is a canvas
at true scale.

## Run

From the repository root:

```sh
./scripts/sync-data.sh                                            # runtime data -> client/public/data (gitignored)
wasm-pack build crates/wasm --target web --out-dir ../../client/src/wasm   # engine -> client/src/wasm (gitignored)
cd client && npm install && npm run dev                           # http://localhost:5173
```

`npm run build` type-checks and bundles to `client/dist`. `npm run smoke`
plays round 1 of Mechanicus vs Necrons in Node against the same wasm build.

## Screens

1. **Setup**: pick a fixture list (Mechanicus, Custodes, Necrons) or upload a
   New Recruit `.ros` for each player, choose the deployment pattern and a
   seed. Army-construction warnings (points over the limit) are shown but do
   not block. "Demo armies" starts the engine's synthetic battle. A saved game
   can be resumed.
2. **Muster**: attach Leaders and Support units to the units they can lead;
   each player marks done and the engine rolls off for first turn.
3. **Battle**: board on the left (wheel zooms, drag pans, click a model to
   select its unit), phase panel on the right. Every action the engine takes
   is reachable: deploy / reserve / embark, scout moves, normal / advance /
   fall back / stationary moves, arriving from reserve, disembarking,
   shooting with per-weapon targets (alternative profiles as radio buttons),
   charge declaration and the charge move, fights with optional pile-in and
   consolidation, army-rule choices (stances, imperatives), and ending the
   phase. Rule violations come back as a toast and the state is unchanged.

The placement tool places a whole unit in a blob, line or column around the
click (or one model at a time), lets you drag single models, clamps them to
the board and to the move limit ring, and nudges overlapping bases apart.

The state JSON is written to `localStorage` after every action.

## Data

Everything the client fetches lives under `public/data` (never committed):
numbers, keywords and effect trees only. Powered by 40kdc-data and BSData.
