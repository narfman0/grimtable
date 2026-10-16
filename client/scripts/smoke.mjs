// Smoke test: load the wasm engine in Node, build the Mechanicus and Necrons
// packs from the fixtures, and play through round 1 with the real action
// JSON the client sends. Run: npm run smoke (after sync-data.sh + wasm-pack).
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const data = path.join(root, 'public', 'data');
const w = await import(pathToFileURL(path.join(root, 'src', 'wasm', 'grimtable_wasm.js')).href);
w.initSync({ module: fs.readFileSync(path.join(root, 'src', 'wasm', 'grimtable_wasm_bg.wasm')) });

const read = (p) => (fs.existsSync(p) ? fs.readFileSync(p, 'utf8') : null);
const core = read(path.join(data, 'enrichment', '_core', 'abilities.json')) ?? '[]';

function buildPack(fixture) {
  const ros = read(path.join(data, 'fixtures', `${fixture}.ros`));
  assert.ok(ros, `missing fixture ${fixture}; run scripts/sync-data.sh`);
  const slug = w.faction_slug(ros);
  const abilities = read(path.join(data, 'enrichment', slug, 'abilities.json')) ?? '[]';
  const detachments = read(path.join(data, 'core', slug, 'detachments.json')) ?? '';
  const json = w.build_pack_with_detachments(ros, abilities, core, detachments);
  const problems = JSON.parse(w.validate_pack(json, 2000));
  const pack = JSON.parse(json);
  console.log(`pack ${pack.name}: ${pack.faction}, ${pack.detachment}, ${pack.points} pts, ${pack.units.length} units, rules ${pack.army_rules.map((r) => r.name).join(', ')}${problems.length ? ' — ' + problems.join('; ') : ''}`);
  return json;
}

const packs = [buildPack('mechanicus'), buildPack('necrons')];
const mission = w.build_mission(read(path.join(data, 'deployment-patterns.json')), 'dawn-of-war');
const game = new w.Game(`[${packs[0]},${packs[1]}]`, mission, 42n);

const state = () => JSON.parse(game.state());
let actions = 0;
// SMOKE_DUMP_DIR=dir writes the raw state JSON after every action, for UI tests.
const dumpDir = process.env.SMOKE_DUMP_DIR;
if (dumpDir) fs.mkdirSync(dumpDir, { recursive: true });
function dump() {
  if (!dumpDir) return;
  const s = state();
  fs.writeFileSync(path.join(dumpDir, `${String(actions).padStart(3, '0')}-${s.phase}${s.pending ? '-pending' : ''}.json`), game.state());
}
function apply(player, action) {
  try {
    game.apply(player, JSON.stringify(action));
    actions++;
    dump();
  } catch (e) {
    throw new Error(`${action.type} by player ${player} failed: ${e.message}\n${JSON.stringify(action).slice(0, 300)}`);
  }
}
const alive = (u) => u.models.filter((m) => m.wounds > 0);
const onBoard = (u) => u.merged_into === null && u.deployed && !u.in_reserve && u.embarked_in === null && alive(u).length > 0;
const radius = (mm) => mm / 25.4 / 2;
const byName = (s, owner, name) => s.units.find((u) => u.owner === owner && u.merged_into === null && u.spec.name.toLowerCase() === name.toLowerCase());

// Formation helpers (same idea as the client's placement tool).
function grid(radii, centre, cols, gap = 0.15) {
  const pitch = Math.max(...radii) * 2 + gap;
  const rows = Math.ceil(radii.length / cols);
  return radii.map((_, i) => {
    const row = Math.floor(i / cols);
    const col = i % cols;
    const inRow = row === rows - 1 ? radii.length - row * cols : cols;
    return { x: centre.x + (col - (inRow - 1) / 2) * pitch, y: centre.y + (row - (rows - 1) / 2) * pitch };
  });
}

// ---------------------------------------------------------------- muster
let s = state();
assert.equal(s.phase, 'Muster');
const dominus = byName(s, 0, 'Tech-Priest Dominus');
const rangers = byName(s, 0, 'Skitarii Rangers');
apply(0, { type: 'attach', leader: dominus.id, bodyguard: rangers.id });
const warden = byName(s, 1, 'Royal Warden');
const warriors = byName(s, 1, 'Necron Warriors');
apply(1, { type: 'attach', leader: warden.id, bodyguard: warriors.id });
s = state();
assert.equal(s.units[dominus.id].merged_into, rangers.id);
assert.equal(s.units[rangers.id].attached_leaders[0], 'Tech-Priest Dominus');
apply(0, { type: 'finish_muster' });
apply(1, { type: 'finish_muster' });
s = state();
assert.equal(s.phase, 'Deployment');
console.log(`muster done; ${s.players[s.first_player].name} deploys first`);

// ------------------------------------------------------------ deployment
const cursors = s.mission.deployment_zones.map((z) => {
  const xs = z.points.map((p) => p.x);
  const ys = z.points.map((p) => p.y);
  const x0 = Math.min(...xs) + 0.5;
  return { x0, x: x0, y: Math.min(...ys) + 0.5, x1: Math.max(...xs) - 0.5, y1: Math.max(...ys) - 0.5, rowH: 0 };
});
function packInZone(player, u) {
  // One row per unit, models side by side at their own base sizes; rows
  // wrap across the zone. Units deploy largest base first so rows stay flat.
  const c = cursors[player];
  const radii = alive(u).map((m) => radius(m.base_mm));
  const gap = 0.15;
  let width = 0;
  for (let i = 0; i < radii.length; i++) width += radii[i] * 2 + (i ? gap : 0);
  const height = Math.max(...radii) * 2;
  if (c.x + width > c.x1) {
    c.x = c.x0;
    c.y += c.rowH + 0.4;
    c.rowH = 0;
  }
  assert.ok(c.y + height <= c.y1, `deployment zone ${player} is full at ${u.spec.name}`);
  const pts = [];
  let x = c.x;
  for (let i = 0; i < radii.length; i++) {
    x += radii[i] + (i ? gap : 0);
    pts.push({ x, y: c.y + height / 2 });
    x += radii[i];
  }
  c.x += width + 0.4;
  c.rowH = Math.max(c.rowH, height);
  return pts;
}
let embarked = null;
const reserved = [0, 0];
while (s.phase === 'Deployment') {
  const p = s.active;
  const todo = s.units
    .filter((u) => u.owner === p && !u.deployed && u.merged_into === null)
    .sort((a, b) => Math.max(...alive(b).map((m) => m.base_mm)) - Math.max(...alive(a).map((m) => m.base_mm)));
  assert.ok(todo.length > 0, 'active player has nothing to deploy');
  const u = todo[0];
  const transport = s.units.find((t) => t.owner === p && t.spec.transport_capacity !== null);
  if (p === 0 && embarked === null && transport && u.spec.name === 'Skitarii Vanguard') {
    apply(p, { type: 'embark', unit: u.id, transport: transport.id });
    embarked = { unit: u.id, transport: transport.id };
  } else if (u.spec.core_rules.includes('DeepStrike') && reserved[p] < 1) {
    apply(p, { type: 'reserve', unit: u.id });
    reserved[p]++;
  } else {
    apply(p, { type: 'deploy', unit: u.id, positions: packInZone(p, u) });
  }
  s = state();
}
assert.equal(s.phase, 'PreBattle');
assert.ok(embarked, 'expected a Vanguard unit to embark in the Dunerider');
assert.equal(s.units[embarked.unit].embarked_in, embarked.transport);
console.log(`deployed: ${s.units.filter((u) => u.deployed && u.merged_into === null).length} units (${reserved[0] + reserved[1]} in reserve, 1 embarked)`);

// Scout move with the first Scouts unit, 2" straight ahead, then both players pass.
const scout = s.units.find((u) => u.owner === s.active && u.spec.core_rules.some((r) => typeof r === 'object' && 'Scouts' in r) && u.deployed && !u.in_reserve);
if (scout) {
  const dir = s.active === 0 ? 2 : -2;
  apply(s.active, { type: 'scout_move', unit: scout.id, positions: alive(scout).map((m) => ({ x: m.pos.x, y: m.pos.y + dir })) });
  console.log(`scout move: ${scout.spec.name}`);
}
apply(s.active, { type: 'end_phase' });
s = state();
apply(s.active, { type: 'end_phase' });
s = state();
assert.equal(s.phase, 'Command');
assert.equal(s.round, 1);

// --------------------------------------------------------------- round 1
let shotsResolved = 0;
let shotsRefused = [];
let movesDone = 0;
for (let turn = 0; turn < 2; turn++) {
  const p = s.active;
  assert.equal(s.phase, 'Command');
  // Army-rule choice if one is pending.
  const own = s.units.filter((u) => u.owner === p && u.merged_into === null && u.deployed);
  const pending = JSON.parse(game.unit_buffs(own[0].id)).choices_pending;
  if (pending.length) {
    apply(p, { type: 'select_stance', name: pending[0][0], option: 0 });
    s = state();
    assert.equal(s.players[p].stances[pending[0][0]], 0);
    console.log(`${s.players[p].name} chose ${pending[0][1][0]} for ${pending[0][0]}`);
  }
  apply(p, { type: 'end_phase' });
  s = state();
  assert.equal(s.phase, 'Movement');
  const dir = p === 0 ? 1 : -1;
  // Move the transport forward and disembark its passengers next to it.
  if (p === 0 && embarked) {
    const t = s.units[embarked.transport];
    const m = Math.min(t.spec.stats.movement, 11);
    apply(p, { type: 'move', unit: t.id, kind: 'Normal', positions: alive(t).map((x) => ({ x: x.pos.x, y: x.pos.y + dir * m })) });
    s = state();
    const tm = alive(s.units[t.id])[0];
    const pass = s.units[embarked.unit];
    const radii = alive(pass).map((x) => radius(x.base_mm));
    const pts = grid(radii, { x: tm.pos.x, y: tm.pos.y + dir * (radius(tm.base_mm) + 0.4 + radii[0] + (radii[0] * 2 + 0.15) / 2) }, 5);
    apply(p, { type: 'disembark', unit: pass.id, positions: pts });
    s = state();
    assert.equal(s.units[pass.id].embarked_in, null);
    assert.ok(s.units[pass.id].disembarked_after_move);
    console.log('transport moved and passengers disembarked');
  }
  // Every other unit that can: a normal move straight ahead, falling back to stationary if refused.
  for (const u of s.units.filter((x) => x.owner === p && x.merged_into === null && x.deployed && !x.in_reserve && x.embarked_in === null && !x.flags.moved && alive(x).length)) {
    const m = Math.min(u.spec.stats.movement, 5);
    try {
      game.apply(p, JSON.stringify({ type: 'move', unit: u.id, kind: 'Normal', positions: alive(u).map((x) => ({ x: x.pos.x, y: x.pos.y + dir * m })) }));
      movesDone++;
    } catch {
      apply(p, { type: 'move', unit: u.id, kind: 'Stationary', positions: [] });
    }
  }
  s = state();
  apply(p, { type: 'end_phase' });
  s = state();
  assert.equal(s.phase, 'Shooting');
  for (const u of s.units.filter((x) => x.owner === p && x.merged_into === null && x.deployed && !x.in_reserve && x.embarked_in === null && alive(x).length)) {
    const visible = JSON.parse(game.visible_enemies(u.id));
    if (!visible.length) continue;
    const ranged = [...new Set(alive(u).flatMap((m) => m.weapons))].filter((wi) => u.spec.weapons[wi].kind === 'Ranged');
    const groups = new Set();
    const assignments = [];
    for (const wi of ranged) {
      const g = u.spec.weapons[wi].profile_group ?? `w${wi}`;
      if (groups.has(g)) continue;
      groups.add(g);
      assignments.push([wi, visible[0]]);
    }
    if (!assignments.length) continue;
    try {
      game.apply(p, JSON.stringify({ type: 'shoot', unit: u.id, assignments }));
      shotsResolved++;
    } catch (e) {
      shotsRefused.push(`${u.spec.name}: ${e.message}`);
    }
  }
  s = state();
  apply(p, { type: 'end_phase' });
  s = state();
  assert.equal(s.phase, 'Charge');
  // Declare a charge if anything is within 12"; complete or accept the failed roll.
  const er = s.params.engagement_range;
  const edge = (a, b) => Math.min(...alive(a).flatMap((x) => alive(b).map((y) => Math.max(0, Math.hypot(x.pos.x - y.pos.x, x.pos.y - y.pos.y) - radius(x.base_mm) - radius(y.base_mm)))));
  const enemies = s.units.filter((x) => x.owner !== p && x.merged_into === null && x.deployed && !x.in_reserve && x.embarked_in === null && alive(x).length);
  let charged = false;
  for (const u of s.units.filter((x) => x.owner === p && x.merged_into === null && x.deployed && !x.in_reserve && x.embarked_in === null && !x.disembarked_after_move && alive(x).length)) {
    const near = enemies.filter((e) => edge(u, e) <= 12);
    if (!near.length) continue;
    apply(p, { type: 'declare_charge', unit: u.id, targets: [near[0].id] });
    s = state();
    if (s.pending) {
      const roll = s.pending.ChargeMove.roll;
      const target = near[0];
      // Move each model straight toward the nearest target model, stopping just inside engagement range.
      const pts = alive(u).map((m) => {
        const t = alive(target).reduce((best, y) => (Math.hypot(m.pos.x - y.pos.x, m.pos.y - y.pos.y) < Math.hypot(m.pos.x - best.pos.x, m.pos.y - best.pos.y) ? y : best));
        const d = Math.hypot(t.pos.x - m.pos.x, t.pos.y - m.pos.y);
        const want = d - radius(m.base_mm) - radius(t.base_mm) - er * 0.5;
        const step = Math.max(0, Math.min(roll, want));
        return { x: m.pos.x + ((t.pos.x - m.pos.x) / d) * step, y: m.pos.y + ((t.pos.y - m.pos.y) / d) * step };
      });
      try {
        game.apply(p, JSON.stringify({ type: 'charge_move', unit: u.id, positions: pts }));
        charged = true;
        console.log(`${u.spec.name} charged ${target.spec.name} with a roll of ${roll}`);
      } catch (e) {
        console.log(`charge move refused (${e.message}); leaving the charge pending is not allowed, so this is a test gap`);
        throw e;
      }
      s = state();
      break;
    }
    console.log(`${u.spec.name} failed its charge against ${near[0].spec.name}`);
    break;
  }
  apply(p, { type: 'end_phase' });
  s = state();
  assert.equal(s.phase, 'Fight');
  let fights = 0;
  while (game.next_fighter() >= 0) {
    const id = game.next_fighter();
    const owner = s.units[id].owner;
    assert.equal(game.player_to_act(), owner);
    const u = s.units[id];
    const target = s.units.find((e) => e.owner !== owner && onBoard(e) && edge(u, e) <= er);
    assert.ok(target, 'next fighter has no enemy in engagement range');
    apply(owner, { type: 'fight', unit: id, target: target.id, pile_in: null, consolidate: null });
    s = state();
    fights++;
  }
  if (charged) assert.ok(fights > 0, 'a charge should lead to a fight');
  if (fights) console.log(`fight phase: ${fights} units fought`);
  apply(p, { type: 'end_phase' });
  s = state();
}
assert.equal(s.round, 2);
assert.equal(s.phase, 'Command');
console.log(`round 1 complete: ${movesDone} moves, ${shotsResolved} shooting attacks resolved, ${shotsRefused.length} refused`);
for (const r of shotsRefused.slice(0, 3)) console.log('  refused:', r);

// Reserves arrive in round 2.
const p = s.active;
apply(p, { type: 'end_phase' });
s = state();
const res = s.units.find((u) => u.owner === p && u.in_reserve);
assert.ok(res, 'expected a unit in reserve for the active player');
if (res) {
  const radii = alive(res).map((m) => radius(m.base_mm));
  const pts = grid(radii, { x: 3, y: p === 0 ? 3 : 41 }, 3);
  apply(p, { type: 'arrive_from_reserve', unit: res.id, positions: pts });
  s = state();
  assert.equal(s.units[res.id].in_reserve, false);
  console.log(`${res.spec.name} arrived from reserve`);
}

// Save/restore round trip: the raw state string survives untouched.
const json = game.state();
const restored = w.Game.restore(json);
assert.equal(restored.state(), json);
assert.equal(restored.player_to_act(), game.player_to_act());

// Rule errors come back as exceptions and leave the state untouched.
assert.throws(() => game.apply(1 - s.active, JSON.stringify({ type: 'end_phase' })), /not your turn/);
assert.equal(game.state(), json);

console.log(`OK: ${actions} actions applied, ${s.log.length} log lines, VP ${s.players[0].vp}-${s.players[1].vp}`);
console.log('log tail:');
for (const l of s.log.slice(-6)) console.log('  ', l.text);
