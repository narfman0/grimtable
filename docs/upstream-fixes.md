# Fixes to send upstream

Data problems found while building, with where they belong. Each becomes a
small commit on the 40kdc fork's `game` branch or a BSData issue. Nothing
here is patched in grimtable.

## 40kdc-data

- **Points one balance update behind** (September 2026 update). `cargo run
  -p content -- report` lists every disagreement with BSData: 31 on
  2026-10-16 across Adeptus Mechanicus, Adeptus Custodes and Necrons, plus
  detachment points for Cohort Cybernetica, Eradication Cohort, Lords of the
  Forge and Canoptek Court.
- **War-form Mantles** (`adeptus-mechanicus/abilities.json`) targets
  `all-friendly`. After the September update the detachment rule applies to
  Tech-Priest units only. Encode the keyword condition.
- **Baffling Data Screed, Data-spike, Data-severed, Searing Conflagaration,
  Emanatus Forcefield (Aura), Vingh's Wafers of Dynamism** have no entries
  for Adeptus Mechanicus; **Turbo Boost** has none for Adeptus Custodes.
- **Damaged brackets** are named per wound range ("Damaged: 1-4 wounds
  remaining") and have no effect encoding; a `wounds-remaining-at-or-below`
  condition exists and would fit.

## BSData wh40k-11e

- **Pteraxii Sterylizor Alpha** has an empty invulnerable save on its own
  profile while the unit profile says 5+.
- Two identical **Canoptek Wraiths** entries price at 95 and 115 in the
  user's export; check whether the catalogue carries a stale cost modifier
  or the export was not refreshed.
