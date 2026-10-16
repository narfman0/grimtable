# Design

Decisions made on 2026-10-16 after surveying what exists. Each one records the
alternative that was rejected.

## Goal

A two-player, rules-enforcing game of the current (11th) edition in a browser,
kept current with balance updates. Also a tool for learning the rules: every
roll is explained by the rule that drove it.

## What exists and what we use

| Project | What it is | Our use |
|---|---|---|
| [40kdc-data](https://github.com/Tabletop-Developer-Consortium/40kdc-data) | Community schema and dataset. 3,920 abilities encoded as a trigger / condition / effect / scope tree. Missions, force dispositions, deployment and terrain data. Rust crate `wh40kdc` exposes the types. Hand-authored, so it lags balance updates. | The ability effect language, missions. Pinned via our fork. |
| [BSData wh40k-11e](https://github.com/BSData/wh40k-11e) | Community datafiles, New Recruit JSON. Updated within days of each balance update. Abilities are prose. | Stats, points, detachment points, wargear options. |
| [OpenHammer](https://github.com/jstrebeck/openhammer) | TypeScript, 10th edition, two factions. Rules-as-content, fixed hook vocabulary, server-authoritative. No license file. | Design reference only. No code reuse. |
| 40 Carrot, TTS mods, terminal tutorials | Virtual tables or partial simulators. | Nothing. |

No rules engine for 11th edition exists in any language. We write ours.

## Architecture

```
40kdc fork (pinned) ──┐
                      ├─► content build (Rust) ──► content pack (JSON, not committed)
BSData 11e (pinned) ──┘                                   │
                                                          ▼
                     ┌──────────────── engine crate (pure Rust, no I/O) ────────────────┐
                     │  game state · hook vocabulary · DSL interpreter · seeded dice    │
                     └───────────────┬──────────────────────────────┬───────────────────┘
                                     │ native                       │ wasm
                                     ▼                              ▼
                      server (axum + WebSocket)            browser client (TypeScript)
                      authoritative, rooms, replay         board, proposals, local preview
```

### Engine

- Pure functions over game state. A client proposes an action; the engine
  validates it against the content pack and returns the new state and a log of
  dice and triggered effects.
- **Rules are content, not code** (from OpenHammer). The engine is a generic
  interpreter over a fixed hook vocabulary. An engine-neutrality test fails if
  the engine ever names a faction.
- Hooks, initial list: `attack.attacksCount`, `attack.beforeHitRoll`,
  `attack.afterHitRoll`, `attack.beforeWoundRoll`, `attack.afterWoundRoll`,
  `attack.allocate`, `attack.beforeSaveRoll`, `attack.afterSaveRoll`,
  `attack.damage`, `attack.feelNoPain`, `attack.resolved`, `move.distance`,
  `move.advanceRoll`, `move.chargeRoll`, `move.completed`, `charge.declared`,
  `charge.completed`, `fight.unitSelected`, `fight.unitFought`,
  `unit.modelDestroyed`, `unit.destroyed`, `command.start`,
  `command.battleShockTest`, `command.battleShockFailed`, `deploy.setup`,
  `deploy.reserves`, `score.primary`.
- Deterministic resolution order: set-value effects before modifiers; ties by
  active player, then declaration order. The net modifier cap on hit and wound
  rolls is an edition parameter applied after summing. Re-rolls and critical
  thresholds are not modifiers.
- Effects carry a duration and are swept at phase, turn, round and battle
  boundaries. Once-per-X counters are tracked centrally by the engine.
- Script escape hatch: an ability the DSL cannot express references a script id.
  Scripts still declare their hook in data. Unimplemented scripts are visible
  and never offered in play.
- Seeded RNG, no external randomness. A game is its seed plus its action log.

### Content build

- Reads both submodules. 40kdc supplies effect trees and missions. BSData
  supplies stats, points, detachment points and wargear. Joined on the BSData
  ids that 40kdc units carry in `external_refs`.
- Any unit that fails to join, or any ability that fails to parse, fails the
  build. A `report` subcommand prints every disagreement between the sources so
  fixes can be sent upstream.
- Output is numbers, keywords and effect trees. No prose. The pack is a build
  artifact, never committed.
- Effect types are generated from the 40kdc JSON Schema with `typify` once, then
  owned here. A schema bump upstream is a deliberate regeneration.

### Server

- Thin. Rooms, seats, one WebSocket per player. Calls the engine. Owns dice and
  state so the client cannot cheat.
- Persistence is the seed plus an append-only action log. Replay rebuilds state.
  That gives reconnect, undo, spectators and bug reports for free. Postgres
  from the first hosted box (a scale-to-zero machine has no disk worth
  keeping); sticky routing by game id only once there is more than one node.

### Client

- TypeScript with a 2D canvas board at true scale. Chosen over a Rust frontend
  framework for tooling. Nothing is lost: the engine runs in the browser as wasm
  for instant legality preview, and the server runs the same engine to decide.
- Hotseat in one tab first. Two-player over the server last.

## Data policy

- Our 40kdc fork: `main` mirrors upstream. Our changes live on `game`, rebased
  onto `main`, one entity per commit so each can be a pull request. Things
  upstream will not accept (the BSData join, game-specific formats) stay here.
- BSData is pinned to a commit per release so a game is reproducible. Bump after
  each balance update.
- Attribution: "Powered by 40kdc-data" with a link, per its data license.

## Legal stance

Mechanics are not copyrightable; the publisher's text and art are. The engine,
server and client contain nothing of theirs. The content pack holds numbers,
keywords and effect trees only, which is the stance BSData and 40kdc have held
publicly for years. No official wording, no artwork, no trademarks in the project
name. A public hosted service, or charging money, raises the risk and is a
separate decision.

## MVP scope and simplifications

What the first playable version enforces, and where it knowingly cuts a
corner. Each corner is a candidate for the roadmap, not a design decision.

Enforced: army construction (points limit, detachment, Warlord candidate,
Epic Hero and datasheet duplicates), leader and support attachment at
muster, roll-off, alternating deployment inside zones, Infiltrators, Scouts,
reserves and Deep Strike arrival limits, every move type with coherency and
engagement checks, Desperate Escape, the full attack sequence with all
universal weapon abilities, range, line of sight, the hidden rule, Lone
Operative, cover, Hazardous, Deadly Demise, charges with 2D6 rolls and the
must-reach-every-target rule, fight ordering (chargers first, then
alternation), pile in and consolidate limits, battle-shock, objective
control including sticky objectives, primary scoring with caps, army-wide
stances (Doctrina Imperatives, Ka'tah stance, Martial Mastery) and recovery
pools (Reanimation Protocols). Abilities the DSL interpreter cannot apply are
reported per unit so the players apply them by hand; nothing is silently
dropped.

Simplified, for now:

- Movement distance is straight-line per model; terrain is passable area
  terrain that blocks line of sight through it.
- Wound allocation is automatic: wounded models first, then the closest to
  the attacker. Leaders are not allocated to while bodyguards live.
  Precision is declared but inert.
- No transports (units deploy on foot), no overwatch, no stratagems yet.
- Damaged brackets, auras affecting other units, once-per-battle abilities
  and most "when destroyed" effects are left to the players.
- Hull-based vehicles use a 90mm round stand-in for geometry.
- The default mission is a Take-and-Hold mirror on a 40kdc deployment
  pattern with a hand-authored symmetric ruins layout; force dispositions
  and the 25-cell matchup matrix are not wired in.

## Rejected alternatives

- **Depend on `wh40kdc` as a crate and overlay our fixes locally.** Rejected:
  overrides in our own format can never become upstream pull requests.
- **Hard-fork 40kdc and merge upstream in.** Rejected in favour of a rebased
  branch: merge commits bury our changes and hide what is still unsent.
- **Rust frontend (Leptos/Yew/Dioxus).** Rejected for tooling; wasm sharing of
  the engine gets the benefit without the cost.
- **Peer-to-peer play.** Rejected: server authority is what makes enforcement
  trustworthy.
