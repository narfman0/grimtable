// End-to-end run against the mock server: two accounts, an editor army and
// an imported one, a closed lobby joined by code, start, and both sockets
// seeing muster through to deployment. Run: npm run e2e (after sync-data.sh,
// wasm-pack, and the three catalogue files; see README).
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import WebSocket from 'ws';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
// E2E_BASE=http://127.0.0.1:8080 runs against a server that is already up
// (the real one: cargo run -p server); otherwise the mock is spawned.
const external = process.env.E2E_BASE ?? null;
const PORT = Number(process.env.PORT ?? 8089);
const base = external ?? `http://localhost:${PORT}`;
const wsBase = base.replace(/^http/, 'ws');

// ------------------------------------------------------------ mock server

const server = external ? null : spawn(process.execPath, [path.join(root, 'scripts', 'mock-server.mjs')], { env: { ...process.env, PORT: String(PORT) }, stdio: ['ignore', 'pipe', 'inherit'] });
server?.stdout.on('data', (d) => process.stdout.write(`[mock] ${d}`));
const stop = () => {
  if (server && !server.killed) server.kill();
};
process.on('exit', stop);

async function waitForServer() {
  for (let i = 0; i < 100; i++) {
    try {
      const r = await fetch(`${base}/api/catalogue`);
      if (r.ok) return;
    } catch {
      // not up yet
    }
    await new Promise((r) => setTimeout(r, 100));
  }
  throw new Error('mock server did not start');
}

// ------------------------------------------------------------ a browser-ish client

class Client {
  constructor(name) {
    this.name = name;
    this.cookie = '';
  }
  async call(method, p, body) {
    const r = await fetch(base + p, {
      method,
      headers: { ...(body === undefined ? {} : { 'content-type': 'application/json' }), ...(this.cookie ? { cookie: this.cookie } : {}) },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    const set = r.headers.get('set-cookie');
    if (set) this.cookie = set.split(';')[0];
    const text = await r.text();
    const json = text ? JSON.parse(text) : undefined;
    if (!r.ok) throw new Error(`${this.name}: ${method} ${p} -> ${r.status} ${json?.error ?? text}`);
    return json;
  }
  socket(gameId) {
    const ws = new WebSocket(`${wsBase}/api/games/${gameId}/ws`, { headers: { cookie: this.cookie } });
    const queue = [];
    const waiters = [];
    ws.on('message', (raw) => {
      const msg = JSON.parse(String(raw));
      if (msg.type === 'pong') return;
      if (waiters.length) waiters.shift()(msg);
      else queue.push(msg);
    });
    return {
      ws,
      open: () => new Promise((res, rej) => ws.once('open', res).once('error', rej)),
      next: (type) =>
        new Promise((res, rej) => {
          const t = setTimeout(() => rej(new Error(`${this.name}: no ${type ?? 'message'} within 5s`)), 5000);
          const take = (msg) => {
            if (type && msg.type !== type) {
              if (msg.type === 'error') return rej(new Error(`${this.name}: server error: ${msg.message}`));
              waiters.push(take);
              return;
            }
            clearTimeout(t);
            res(msg);
          };
          if (queue.length) take(queue.shift());
          else waiters.push(take);
        }),
      send: (obj) => ws.send(JSON.stringify(obj)),
      close: () => ws.close(),
    };
  }
}

// ------------------------------------------------------------ the run

await waitForServer();
const A = new Client('A');
const B = new Client('B');
const suffix = Date.now().toString(36);

const ua = await A.call('POST', '/api/register', { username: `alice-${suffix}`, password: 'pw' });
const ub = await B.call('POST', '/api/register', { username: `bob-${suffix}`, password: 'pw' });
assert.equal((await A.call('GET', '/api/me')).username, ua.username);
assert.equal((await B.call('GET', '/api/me')).username, ub.username);
await assert.rejects(A.call('POST', '/api/register', { username: ua.username, password: 'x' }), /409/);
console.log(`registered ${ua.username} and ${ub.username}`);

// A: an editor army from the Mechanicus catalogue.
const factions = await A.call('GET', '/api/catalogue');
assert.ok(factions.some((f) => f.faction_id === 'adeptus-mechanicus'), 'mechanicus catalogue missing; see README');
const cat = await A.call('GET', '/api/catalogue/adeptus-mechanicus');
const sheet = (id) => cat.units.find((u) => u.id === id) ?? assert.fail(`no datasheet ${id}`);
const defaultUnit = (id, extra = {}) => ({
  datasheet_id: id,
  models: sheet(id).models.map((r) => ({ row: r.name, count: r.min, weapon_ids: [...r.default_weapon_ids], variants: [] })),
  enhancement_id: null,
  warlord: false,
  ...extra,
});
const detachment = cat.detachments.find((d) => d.detachment_points <= 3) ?? cat.detachments[0];
const def = {
  name: 'E2E Mechanicus',
  faction_id: 'adeptus-mechanicus',
  detachment_ids: [detachment.id],
  units: [defaultUnit('tech-priest-dominus', { warlord: true }), defaultUnit('skitarii-rangers'), defaultUnit('skitarii-vanguard'), defaultUnit('onager-dunecrawler')],
};
let armyA = await A.call('POST', '/api/armies', { name: def.name, kind: 'def', def });
assert.equal(armyA.kind, 'def');
assert.deepEqual(armyA.problems, [], `editor army should validate: ${armyA.problems}`);
// Edit: a second Rangers unit, saved with PUT.
def.units.push(defaultUnit('skitarii-rangers'));
armyA = await A.call('PUT', `/api/armies/${armyA.id}`, { name: def.name, def });
assert.equal(armyA.def.units.length, 5);
const packA = await A.call('GET', `/api/armies/${armyA.id}/pack`);
assert.equal(packA.units.length, 5);
assert.ok(packA.detachment, 'pack carries the detachment');
console.log(`A built ${armyA.name}: ${armyA.points} pts, ${packA.units.length} units, rules ${packA.army_rules.map((r) => r.name).join(', ')}`);
// Validation reports problems without refusing the save.
const bad = await A.call('POST', '/api/armies', { name: 'bad', kind: 'def', def: { ...def, detachment_ids: [], units: def.units.map((u) => ({ ...u, warlord: false })) } });
assert.ok(bad.problems.some((p) => p.includes('detachment')) && bad.problems.some((p) => p.includes('Warlord')), `expected problems, got ${bad.problems}`);
await A.call('DELETE', `/api/armies/${bad.id}`);
assert.equal((await A.call('GET', '/api/armies')).length, 1);

// B: the Necron fixture.
const ros = fs.readFileSync(path.join(root, 'public', 'data', 'fixtures', 'necrons.ros'), 'utf8');
const armyB = await B.call('POST', '/api/armies/import', { name: 'E2E Necrons', ros });
assert.equal(armyB.kind, 'ros');
assert.ok(armyB.points > 0);
console.log(`B imported ${armyB.name}: ${armyB.points} pts${armyB.problems.length ? ` (${armyB.problems.join('; ')})` : ''}`);

// A closed game; the fixture is over 2000 pts as exported, so the limit is raised.
const limit = Math.max(2000, armyA.points, armyB.points);
const game = await A.call('POST', '/api/games', { visibility: 'closed', pattern: 'dawn-of-war', points_limit: limit, army_id: armyA.id });
assert.equal(game.join_code.length, 6);
assert.equal(game.host, ua.username);
assert.equal((await B.call('GET', `/api/games/${game.id}`)).join_code, null, 'the code is hidden from non-players');
assert.equal(game.my_seat, 0);
assert.equal(game.status, 'waiting');
assert.ok(!(await B.call('GET', '/api/games/open')).some((g) => g.id === game.id), 'closed games are not listed');

// A watches the lobby over the socket while B joins by code.
const wsA = A.socket(game.id);
await wsA.open();
const lobby0 = await wsA.next('lobby');
assert.equal(lobby0.game.seats.length, 1);
const joined = await B.call('POST', '/api/games/join', { join_code: game.join_code, army_id: armyB.id });
assert.equal(joined.my_seat, 1);
const lobby1 = await wsA.next('lobby');
assert.equal(lobby1.game.seats.length, 2);
assert.equal(lobby1.game.seats[1].username, ub.username);
assert.equal((await B.call('GET', '/api/games/mine')).length, 1);
await assert.rejects(B.call('POST', `/api/games/${game.id}/start`), /40[0-9]/, 'only the host starts');
await assert.rejects(B.call('DELETE', `/api/armies/${armyB.id}`), /409/, 'a seated army cannot be deleted');
console.log(`lobby ${game.join_code}: ${lobby1.game.seats.map((s) => `${s.username} (${s.army_name}, ${s.army_points} pts)`).join(' vs ')}`);

const wsB = B.socket(game.id);
await wsB.open();
await wsB.next('lobby');
const started = await A.call('POST', `/api/games/${game.id}/start`);
assert.equal(started.status, 'playing');
assert.ok(Array.isArray(started.problems) && started.problems.length === 2, 'start reports per-seat problems');
const sA = await wsA.next('state');
const sB = await wsB.next('state');
assert.equal(sA.seat, 0);
assert.equal(sB.seat, 1);
assert.equal(sA.state.phase, 'Muster');
assert.equal(sB.state.phase, 'Muster');
assert.equal(sA.state.players.length, 2);
assert.ok(sA.state.players[0].name && sA.state.players[1].name);
assert.equal((await A.call('GET', `/api/games/${game.id}`)).status, 'playing');

// A seat cannot act for the other seat: the server takes the seat from the cookie.
wsA.send({ type: 'action', action: { type: 'finish_muster' } });
const a1 = await wsA.next('state');
assert.ok(a1.state.fight_queue.includes(0), 'A is done mustering');
const b1 = await wsB.next('state');
assert.ok(b1.state.fight_queue.includes(0), 'B sees A done');
// An illegal action is refused with an error and no state change.
wsB.send({ type: 'action', action: { type: 'attach', leader: 0, bodyguard: 1 } });
const err = await wsB.next();
assert.equal(err.type, 'error', `expected an error, got ${JSON.stringify(err).slice(0, 200)}`);
wsB.send({ type: 'action', action: { type: 'finish_muster' } });
const b2 = await wsB.next('state');
const a2 = await wsA.next('state');
assert.equal(b2.state.phase, 'Deployment');
assert.equal(a2.state.phase, 'Deployment');
assert.equal(typeof a2.to_act, 'number');
console.log(`deployment reached; ${a2.state.players[a2.state.first_player].name} deploys first; log: ${a2.state.log.map((l) => l.text.trim()).slice(-2).join(' | ')}`);

// The game state is restorable by a fresh socket.
const wsA2 = A.socket(game.id);
await wsA2.open();
const again = await wsA2.next('state');
assert.equal(again.state.phase, 'Deployment');
wsA2.close();

wsA.close();
wsB.close();
stop();
console.log('e2e ok');
