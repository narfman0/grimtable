# grimtable

A rules-enforcing engine and web client for playing the 11th edition of a certain
grimdark tabletop wargame. Rust engine, Rust server, TypeScript client. The engine
compiles to WebAssembly so the browser runs the same rules the server enforces.

**Status:** early. See [docs/design.md](docs/design.md) for the architecture and
[docs/roadmap.md](docs/roadmap.md) for what is built and what is next.

## Layout

| Path | What |
|---|---|
| `crates/engine` | Pure rules engine. No I/O. Game state, hook vocabulary, dice, attack sequence. |
| `crates/content` | Build step: joins the two data submodules into a content pack the engine loads. |
| `crates/server` | Authoritative game server (axum, WebSockets). |
| `client/` | Browser client (TypeScript). |
| `data/40kdc-data` | Submodule: [our fork](https://github.com/narfman0/40kdc-data) of [40kdc-data](https://github.com/Tabletop-Developer-Consortium/40kdc-data), the ability effect DSL and mission data. |
| `data/wh40k-11e` | Submodule: [BSData wh40k-11e](https://github.com/BSData/wh40k-11e), stats and points. |

## Build

```sh
git submodule update --init
cargo test
cargo run -p content -- report   # join the data sources and print discrepancies
```

## Content and licensing

Nothing in this repository is official. The engine, server and client are MIT.
The content pack is never committed: it is built from the pinned submodules, and
it contains numbers, keywords and structured effect data only. No rules prose, no
artwork, no official wording.

Powered by [40kdc-data](https://github.com/Tabletop-Developer-Consortium/40kdc-data)
(data CC BY 4.0, schemas CC0) and the [BSData](https://github.com/BSData) community.
