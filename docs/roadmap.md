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
- [~] Effect interpreter over the 40kdc DSL — `crates/engine/src/effects.rs`: conditional/sequence/stance-select/choice, FNP, invuln, roll and stat modifiers, re-rolls, keyword and weapon-ability grants, sticky objectives, recovery pools; unknown nodes are reported, not dropped. A fixed hook vocabulary is still implicit in the phase code rather than data.

## 2. Content build

- [~] Load 40kdc units, weapons, abilities, detachments, stratagems — units and detachments only, `crates/content/src/main.rs`
- [x] Load BSData faction files; join on `external_refs` — `load_bsdata`, 117 of 130 units join; the 13 unjoined are Combat Patrol variants with no BSData ref
- [~] `report`: points and detachment-point disagreements (`cargo run -p content -- report`, 31 found on 2026-10-16, all consistent with the September balance update). Stats not compared yet. Units priced per model in BSData show as `None`.
- [x] Emit a content pack for the three fixture lists — `cargo run -p content -- pack crates/roster/fixtures/<name>.ros`, test `packs_build_for_all_fixtures`
- [x] Import a New Recruit roster — `crates/roster`, fixtures parse to 1,980 / 1,830 / 2,055 pts as exported; `content::pack::validate` flags the over-limit list
- [x] Army rules from 40kdc faction entries ride on the pack — tests `army_rules_offer_stances_and_apply_once_chosen`, `reanimation_restores_wounds_at_end_of_command_phase`, `katah_stance_grants_lethal_hits_in_the_fight_phase'

## 3. Engine: the rest of the turn

- [x] Board, bases, measurement, engagement range, coherency — `geom.rs`, `rules.rs`
- [~] Line of sight over terrain — segment-versus-polygon, area terrain only; no height
- [x] Movement: normal, advance, fall back, Desperate Escape, reserves and Deep Strike — `actions.rs`, tests in `tests/game.rs`
- [~] Charge: declaration, roll, pending charge move — test `charge_and_fight`; no overwatch
- [x] Fight: chargers and Fights First first, alternation, pile in, consolidate — `combat.rs`
- [x] Command phase: CP, battle-shock, recovery pools
- [ ] Stratagems with reactive windows
- [~] Objective control, sticky objectives and primary scoring with caps — `rules.rs`, `actions.rs::score`; one default mission, no force dispositions
- [~] Detachment rules and enhancements — enhancements arrive as abilities; detachment rules not yet attached (40kdc keys them by detachment id)
- [~] Transports: embark at deployment, disembark in movement, forced disembark from a wreck — test `vanguard_ride_the_dunerider`; no Firing Deck
- [ ] Overwatch, defender wound allocation, Precision, damaged brackets

## 4. Client

- [ ] wasm build of the engine
- [ ] Hotseat: board, unit cards, propose-and-validate flow, narrated dice

## 5. Server

- [ ] Rooms, seats, WebSocket protocol
- [ ] Seed + action log persistence, reconnect, replay
- [ ] Two browsers play a full game

## 6. Multiplayer (in progress, see docs/multiplayer.md)

- [x] Faction catalogue for the editor: datasheets, points by size with BSData overlay, model rows and loadout variants, wargear options, weapons, enhancements, detachments with detachment points, leader eligibility — `crates/content/src/catalogue.rs`, test `catalogue_has_the_mechanicus_essentials`
- [x] Editor army definition, validation and pack building — `crates/content/src/army_def.rs`, test `an_editor_army_validates_and_plays`
- [x] Server: accounts, armies, lobbies with join codes, WebSocket games — `crates/server`, tests in `crates/server/tests/multiplayer.rs`
- [x] Client: login, armies and editor, lobby, networked battle — `client/src/{login,home,editor,game,api,session}.ts`; `npm run e2e` passes against the real server
- [x] Leave-lobby endpoint (free seat, cancel, concede) and session expiry with sliding renewal — tests `leaving_frees_a_seat_cancels_a_lobby_and_concedes_a_game`, `sessions_expire`
- [ ] Reconnection UX, spectator list

## Deferred

- [ ] **CPU opponent.** A server-held seat that picks actions. Needs a legal-move generator in the engine (deployment spots, moves, targets, charges) before any policy can sit on top of it.
