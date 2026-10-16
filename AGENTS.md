# grimtable: notes for agents

Read `docs/design.md` before changing anything. It records the architecture
decisions and why they were made. `docs/roadmap.md` tracks what is done, with
evidence, and what is next.

## Rules that are not negotiable

- **Rules are content, not code.** The engine never names a faction, a unit or an
  ability. Everything the publisher prints lives in the content pack, which is
  built from `data/` and never committed.
- **No rules prose anywhere in this repo.** Numbers, keywords and effect trees
  only. Tests may describe a rule in our own words. Never paste official wording.
- **Deterministic.** Same state, same seed, same actions, same result. All
  randomness goes through the engine's seeded RNG.
- **Done means tested.** A rule is done when a test pins its behaviour, and a
  feature is done when a player can reach it in the client.
- **Fixes to shared data go upstream.** A wrong stat, points value or ability
  encoding gets fixed in `data/40kdc-data` on its `game` branch as a small,
  self-contained commit, then sent upstream as a pull request. Not patched here.

## Data submodules

- `data/40kdc-data`: our fork. `main` mirrors upstream, never commit to it.
  Work on `game`, rebased onto `main`. Pin grimtable to a `game` commit.
- `data/wh40k-11e`: BSData, pinned to a commit. Bump after each balance update
  and re-run `cargo run -p content -- report` to see what moved.

## Git

Commit small, directly to `main`. Pull before starting.
