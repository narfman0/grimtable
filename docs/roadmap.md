# Roadmap

`[x]` done means a test or a client path proves it. Cite the evidence.

## 1. Engine: attack sequence

- [x] Seeded deterministic dice and dice expressions ("D6", "2D6+1", "D3") — `crates/engine/src/dice.rs`, tests `same_seed_same_rolls`, `parses_expressions`
- [x] Hit, wound, save, damage pipeline with the strength-vs-toughness table — `crates/engine/src/attack.rs`, tests `wound_table`, `plain_sequence_hit_wound_fail_save_damage`
- [x] Net modifier cap on hit and wound rolls — test `modifiers_are_capped_to_plus_minus_one`
- [~] Weapon abilities. Done, each with a test in `attack.rs`: Sustained Hits,
      Lethal Hits, Devastating Wounds, Torrent, Twin-linked, Rapid Fire, Blast,
      Melta, Anti-X, Heavy, Ignores Cover. Declared but inert until the phases
      that use them exist: Lance, Hazardous, Precision, Indirect Fire, Assault,
      Pistol, Extra Attacks, Psychic
- [x] Invulnerable saves, Feel No Pain, cover — tests `invulnerable_save_is_used_when_better`, `feel_no_pain_rolls_per_point_of_damage`, `heavy_and_cover_cancel_out`
- [~] Per-wound allocation and model removal — sequential allocation to one unit with spill for mortal wounds (`devastating_wounds_become_mortals_that_spill_over`, `normal_damage_does_not_spill`). Defender choice of model and Precision not yet.
- [ ] Hook vocabulary and effect interpreter over the 40kdc DSL

## 2. Content build

- [~] Load 40kdc units, weapons, abilities, detachments, stratagems — units and detachments only, `crates/content/src/main.rs`
- [x] Load BSData faction files; join on `external_refs` — `load_bsdata`, 117 of 130 units join; the 13 unjoined are Combat Patrol variants with no BSData ref
- [~] `report`: points and detachment-point disagreements (`cargo run -p content -- report`, 31 found on 2026-10-16, all consistent with the September balance update). Stats not compared yet. Units priced per model in BSData show as `None`.
- [ ] Emit a content pack for Adeptus Mechanicus, Adeptus Custodes, Necrons
- [ ] Import a New Recruit roster and match every unit; totals equal New Recruit

## 3. Engine: the rest of the turn

- [ ] Board, bases, measurement, engagement range
- [ ] Line of sight over terrain
- [ ] Movement: normal, advance, fall back, Desperate Escape, reserves
- [ ] Charge: declaration, roll, overwatch window
- [ ] Fight: fights-first ordering, pile in, consolidate
- [ ] Command phase: CP, battle-shock
- [ ] Stratagems with reactive windows
- [ ] Objective control and primary scoring; force disposition missions
- [ ] Detachment rules and enhancements

## 4. Client

- [ ] wasm build of the engine
- [ ] Hotseat: board, unit cards, propose-and-validate flow, narrated dice

## 5. Server

- [ ] Rooms, seats, WebSocket protocol
- [ ] Seed + action log persistence, reconnect, replay
- [ ] Two browsers play a full game
