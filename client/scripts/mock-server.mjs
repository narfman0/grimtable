// In-memory mock of the server in docs/multiplayer.md, enough to click
// through every client screen and play a game. Games apply actions with the
// real wasm engine. Catalogues are read from public/data/catalogue/*.json
// (cargo run -p content -- catalogue <faction> > that file); .ros imports are
// built with the wasm pack builder. Nothing is persisted.
//
//   npm run mock              # http://localhost:8080
//   PORT=8089 npm run mock

import crypto from 'node:crypto';
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { WebSocketServer } from 'ws';
import { toPack, validate } from './army-def.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const data = path.join(root, 'public', 'data');
const PORT = Number(process.env.PORT ?? 8080);
const COOKIE = 'grimtable_session';
const DP_BUDGET = 3;
const ARMY_POINTS_LIMIT = 2000;

const w = await import(pathToFileURL(path.join(root, 'src', 'wasm', 'grimtable_wasm.js')).href);
w.initSync({ module: fs.readFileSync(path.join(root, 'src', 'wasm', 'grimtable_wasm_bg.wasm')) });

const read = (p) => (fs.existsSync(p) ? fs.readFileSync(p, 'utf8') : null);
const coreAbilities = read(path.join(data, 'enrichment', '_core', 'abilities.json')) ?? '[]';
const patterns = read(path.join(data, 'deployment-patterns.json'));
if (!patterns) {
  console.error('missing public/data/deployment-patterns.json; run scripts/sync-data.sh');
  process.exit(1);
}

// ------------------------------------------------------------ catalogues

const catalogues = new Map();
const catDir = path.join(data, 'catalogue');
if (fs.existsSync(catDir)) {
  for (const f of fs.readdirSync(catDir).filter((x) => x.endsWith('.json'))) {
    const cat = JSON.parse(fs.readFileSync(path.join(catDir, f), 'utf8'));
    catalogues.set(cat.faction_id, cat);
  }
}
if (!catalogues.size) console.warn('no catalogues in public/data/catalogue; the editor will have no factions');

function abilitiesIndex(slug) {
  const map = new Map();
  for (const text of [coreAbilities, read(path.join(data, 'enrichment', slug, 'abilities.json')) ?? '[]']) {
    for (const a of JSON.parse(text)) if (typeof a.name === 'string' && !map.has(a.name.toLowerCase())) map.set(a.name.toLowerCase(), a);
  }
  return map;
}

// ------------------------------------------------------------ store

const users = new Map(); // id -> {id, username, password}
const sessions = new Map(); // token -> user id
const armies = new Map(); // id -> record (+ owner, def|ros, packJson)
const games = new Map(); // id -> game
let nextId = 1;
const id = () => nextId++;

class HttpError extends Error {
  constructor(status, message) {
    super(message);
    this.status = status;
  }
}

function armyPublic(a, full = false) {
  const out = { id: a.id, name: a.name, kind: a.kind, faction_id: a.faction_id, points: a.points, updated_at: a.updated_at };
  if (full) {
    if (a.kind === 'def') out.def = a.def;
    out.problems = a.problems;
  }
  return out;
}

function buildDefArmy(a, def) {
  const cat = catalogues.get(def.faction_id);
  if (!cat) throw new HttpError(400, `unknown faction ${def.faction_id}`);
  const problems = validate(def, cat, ARMY_POINTS_LIMIT, DP_BUDGET);
  const pack = toPack(def, cat, abilitiesIndex(def.faction_id));
  Object.assign(a, { kind: 'def', faction_id: def.faction_id, def, points: pack.points, problems, packJson: JSON.stringify(pack), updated_at: new Date().toISOString() });
}

function buildRosArmy(a, ros) {
  let slug;
  try {
    slug = w.faction_slug(ros);
  } catch (e) {
    throw new HttpError(400, `roster: ${e.message}`);
  }
  const abilities = read(path.join(data, 'enrichment', slug, 'abilities.json')) ?? '[]';
  const detachments = read(path.join(data, 'core', slug, 'detachments.json')) ?? '';
  let packJson;
  try {
    packJson = w.build_pack_with_detachments(ros, abilities, coreAbilities, detachments);
  } catch (e) {
    throw new HttpError(400, `pack: ${e.message}`);
  }
  const pack = JSON.parse(packJson);
  // The real server scrubs prose before storing; the mock keeps nothing on disk.
  Object.assign(a, { kind: 'ros', faction_id: slug, ros, points: pack.points, problems: JSON.parse(w.validate_pack(packJson, ARMY_POINTS_LIMIT)), packJson, updated_at: new Date().toISOString() });
}

function joinCode() {
  const alphabet = 'ABCDEFGHJKLMNPQRSTUVWXYZ23456789';
  for (;;) {
    let c = '';
    for (let i = 0; i < 6; i++) c += alphabet[crypto.randomInt(alphabet.length)];
    if (![...games.values()].some((g) => g.join_code === c)) return c;
  }
}

function gamePublic(g, userId) {
  const seats = g.seats.map((s, i) => {
    const army = armies.get(s.army_id);
    return { seat: i, username: users.get(s.user_id).username, army_name: army?.name ?? '?', army_points: army?.points ?? 0, army_id: s.army_id };
  });
  const my = g.seats.findIndex((s) => s.user_id === userId);
  return {
    id: g.id,
    join_code: my < 0 ? null : g.join_code,
    visibility: g.visibility,
    status: g.status,
    pattern: g.pattern,
    points_limit: g.points_limit,
    seats,
    my_seat: my < 0 ? null : my,
    winner: g.winner,
    host: users.get(g.seats[0].user_id).username,
    created_at: g.created_at,
  };
}

function seatOf(g, userId) {
  const i = g.seats.findIndex((s) => s.user_id === userId);
  return i < 0 ? null : i;
}

function stateMessage(g, seat) {
  return `{"type":"state","seat":${seat === null ? 'null' : seat},"state":${g.game.state()},"to_act":${g.game.player_to_act()}}`;
}

function broadcast(g, make) {
  for (const sock of g.sockets) {
    if (sock.readyState !== sock.OPEN) continue;
    sock.send(make(seatOf(g, sock.userId)));
  }
}

function joinGame(g, userId, armyId) {
  if (g.status !== 'waiting') throw new HttpError(409, 'that game has started');
  if (g.seats.some((s) => s.user_id === userId)) throw new HttpError(409, 'you are already in that game');
  if (g.seats.length >= 2) throw new HttpError(409, 'that game is full');
  const army = ownArmy(userId, armyId);
  g.seats.push({ user_id: userId, army_id: army.id });
  broadcast(g, (seat) => JSON.stringify({ type: 'lobby', game: gamePublic(g, sockUser(g, seat)) }));
  return g;
}

function sockUser(g, seat) {
  return seat === null ? null : g.seats[seat].user_id;
}

function ownArmy(userId, armyId) {
  const a = armies.get(Number(armyId));
  if (!a || a.owner !== userId) throw new HttpError(404, 'no such army');
  return a;
}

function startGame(g, userId) {
  if (g.seats[0]?.user_id !== userId) throw new HttpError(403, 'only the host can start');
  if (g.status !== 'waiting') throw new HttpError(409, 'already started');
  if (g.seats.length < 2) throw new HttpError(409, 'both seats must be filled');
  const packs = g.seats.map((s) => armies.get(s.army_id));
  const problems = packs.map((a) => [...a.problems]);
  const blocking = [];
  packs.forEach((a, i) => {
    if (a.points > g.points_limit) blocking.push(`${a.name} is ${a.points} pts, over the ${g.points_limit} pt limit`);
    if (!JSON.parse(a.packJson).detachment) blocking.push(`${a.name} has no detachment`);
    for (const b of blocking) if (!problems[i].includes(b)) problems[i].push(b);
  });
  if (blocking.length) {
    const e = new HttpError(409, blocking.join('; '));
    e.problems = problems;
    throw e;
  }
  let mission;
  try {
    mission = w.build_mission(patterns, g.pattern);
  } catch (e) {
    throw new HttpError(400, `mission: ${e.message}`);
  }
  const seed = crypto.randomBytes(8).readBigUInt64LE();
  try {
    g.game = new w.Game(`[${packs[0].packJson},${packs[1].packJson}]`, mission, seed);
  } catch (e) {
    throw new HttpError(400, `engine: ${e.message}`);
  }
  g.seed = seed.toString();
  g.actions = [];
  g.status = 'playing';
  broadcast(g, (seat) => stateMessage(g, seat));
  return problems;
}

// ------------------------------------------------------------ http

function parseCookies(req) {
  const out = {};
  for (const part of (req.headers.cookie ?? '').split(';')) {
    const [k, ...v] = part.trim().split('=');
    if (k) out[k] = decodeURIComponent(v.join('='));
  }
  return out;
}

function userOf(req) {
  const token = parseCookies(req)[COOKIE];
  const uid = token && sessions.get(token);
  return uid ? users.get(uid) : null;
}

function requireUser(req) {
  const u = userOf(req);
  if (!u) throw new HttpError(401, 'not signed in');
  return u;
}

function body(req) {
  return new Promise((resolve, reject) => {
    const chunks = [];
    req.on('data', (c) => chunks.push(c));
    req.on('end', () => {
      const text = Buffer.concat(chunks).toString('utf8');
      if (!text) return resolve({});
      try {
        resolve(JSON.parse(text));
      } catch {
        reject(new HttpError(400, 'bad JSON'));
      }
    });
    req.on('error', reject);
  });
}

function send(res, status, json, headers = {}) {
  if (json === undefined) {
    res.writeHead(status, headers);
    res.end();
    return;
  }
  const text = typeof json === 'string' ? json : JSON.stringify(json);
  res.writeHead(status, { 'content-type': 'application/json', ...headers });
  res.end(text);
}

async function handle(req, res) {
  const url = new URL(req.url, 'http://localhost');
  const parts = url.pathname.split('/').filter(Boolean); // ['api', ...]
  if (parts[0] !== 'api') throw new HttpError(404, 'not found');
  const m = req.method;
  const p = parts.slice(1);
  const route = (method, ...pattern) => m === method && p.length === pattern.length && pattern.every((x, i) => x === '*' || x === p[i]);

  // accounts
  if (route('POST', 'register')) {
    const { username, password } = await body(req);
    if (typeof username !== 'string' || !username.trim() || typeof password !== 'string' || !password) throw new HttpError(400, 'username and password are needed');
    if ([...users.values()].some((u) => u.username.toLowerCase() === username.trim().toLowerCase())) throw new HttpError(409, 'that username is taken');
    const user = { id: id(), username: username.trim(), password };
    users.set(user.id, user);
    const token = crypto.randomBytes(24).toString('hex');
    sessions.set(token, user.id);
    return send(res, 201, { id: user.id, username: user.username }, { 'set-cookie': `${COOKIE}=${token}; Path=/; HttpOnly; SameSite=Lax` });
  }
  if (route('POST', 'login')) {
    const { username, password } = await body(req);
    const user = [...users.values()].find((u) => u.username.toLowerCase() === String(username ?? '').trim().toLowerCase());
    if (!user || user.password !== password) throw new HttpError(401, 'wrong username or password');
    const token = crypto.randomBytes(24).toString('hex');
    sessions.set(token, user.id);
    return send(res, 200, { id: user.id, username: user.username }, { 'set-cookie': `${COOKIE}=${token}; Path=/; HttpOnly; SameSite=Lax` });
  }
  if (route('POST', 'logout')) {
    sessions.delete(parseCookies(req)[COOKIE]);
    return send(res, 204, undefined, { 'set-cookie': `${COOKIE}=; Path=/; Max-Age=0` });
  }
  if (route('GET', 'me')) {
    const u = requireUser(req);
    return send(res, 200, { id: u.id, username: u.username });
  }

  // catalogue
  if (route('GET', 'catalogue')) return send(res, 200, [...catalogues.values()].map((c) => ({ faction_id: c.faction_id, name: c.name })));
  if (route('GET', 'catalogue', '*')) {
    const c = catalogues.get(p[1]);
    if (!c) throw new HttpError(404, 'no such faction');
    return send(res, 200, c);
  }

  // armies
  const user = requireUser(req);
  if (route('GET', 'armies')) return send(res, 200, [...armies.values()].filter((a) => a.owner === user.id).map((a) => armyPublic(a)));
  if (route('POST', 'armies')) {
    const b = await body(req);
    if (b.kind !== 'def' || !b.def || typeof b.def !== 'object') throw new HttpError(400, 'kind must be "def" with a def');
    const a = { id: id(), owner: user.id, name: String(b.name ?? b.def.name ?? 'army') };
    buildDefArmy(a, { ...b.def, name: a.name });
    armies.set(a.id, a);
    return send(res, 201, armyPublic(a, true));
  }
  if (route('POST', 'armies', 'import')) {
    const b = await body(req);
    if (typeof b.ros !== 'string' || !b.ros.trim()) throw new HttpError(400, 'ros XML is needed');
    const a = { id: id(), owner: user.id, name: String(b.name ?? 'imported') };
    buildRosArmy(a, b.ros);
    if (!b.name) a.name = JSON.parse(a.packJson).name || a.name;
    armies.set(a.id, a);
    return send(res, 201, armyPublic(a, true));
  }
  if (route('GET', 'armies', '*')) return send(res, 200, armyPublic(ownArmy(user.id, p[1]), true));
  if (route('PUT', 'armies', '*')) {
    const a = ownArmy(user.id, p[1]);
    const b = await body(req);
    if (typeof b.name === 'string' && b.name.trim()) a.name = b.name.trim();
    if (b.def) {
      if (a.kind !== 'def') throw new HttpError(400, 'imported armies have no def');
      buildDefArmy(a, { ...b.def, name: a.name });
    } else if (a.kind === 'def') buildDefArmy(a, { ...a.def, name: a.name });
    a.updated_at = new Date().toISOString();
    return send(res, 200, armyPublic(a, true));
  }
  if (route('DELETE', 'armies', '*')) {
    const a = ownArmy(user.id, p[1]);
    if ([...games.values()].some((g) => g.status !== 'finished' && g.seats.some((s) => s.army_id === a.id))) throw new HttpError(409, 'that army is seated in an unfinished game');
    armies.delete(a.id);
    return send(res, 204);
  }
  if (route('GET', 'armies', '*', 'pack')) return send(res, 200, ownArmy(user.id, p[1]).packJson);

  // games
  if (route('POST', 'games')) {
    const b = await body(req);
    const army = ownArmy(user.id, b.army_id);
    const g = {
      id: id(),
      join_code: joinCode(),
      visibility: b.visibility === 'closed' ? 'closed' : 'open',
      status: 'waiting',
      pattern: typeof b.pattern === 'string' ? b.pattern : 'dawn-of-war',
      points_limit: Number(b.points_limit) || 2000,
      seats: [{ user_id: user.id, army_id: army.id }],
      winner: null,
      created_at: new Date().toISOString(),
      game: null,
      actions: [],
      sockets: new Set(),
    };
    games.set(g.id, g);
    return send(res, 201, gamePublic(g, user.id));
  }
  if (route('GET', 'games', 'open')) {
    return send(
      res,
      200,
      [...games.values()].filter((g) => g.visibility === 'open' && g.status === 'waiting' && g.seats.length < 2).map((g) => ({ id: g.id, host: users.get(g.seats[0].user_id).username, pattern: g.pattern, points_limit: g.points_limit, created_at: g.created_at })),
    );
  }
  if (route('GET', 'games', 'mine')) return send(res, 200, [...games.values()].filter((g) => g.seats.some((s) => s.user_id === user.id)).map((g) => gamePublic(g, user.id)));
  if (route('POST', 'games', 'join')) {
    const b = await body(req);
    const g = [...games.values()].find((x) => x.join_code === String(b.join_code ?? '').toUpperCase());
    if (!g) throw new HttpError(404, 'no game with that code');
    joinGame(g, user.id, b.army_id);
    return send(res, 200, gamePublic(g, user.id));
  }
  const g = p[0] === 'games' && p[1] ? games.get(Number(p[1])) : null;
  if (p[0] === 'games' && p[1] && !g) throw new HttpError(404, 'no such game');
  if (route('GET', 'games', '*')) return send(res, 200, gamePublic(g, user.id));
  if (route('POST', 'games', '*', 'join')) {
    const b = await body(req);
    if (g.visibility !== 'open') throw new HttpError(403, 'that game needs a join code');
    if (g.seats[0]?.user_id === user.id) throw new HttpError(409, 'you host that game');
    joinGame(g, user.id, b.army_id);
    return send(res, 200, gamePublic(g, user.id));
  }
  if (route('POST', 'games', '*', 'start')) {
    const problems = startGame(g, user.id);
    return send(res, 200, { ...gamePublic(g, user.id), problems });
  }
  throw new HttpError(404, 'not found');
}

const server = http.createServer((req, res) => {
  handle(req, res).catch((e) => {
    const status = e instanceof HttpError ? e.status : 500;
    if (status === 500) console.error(e);
    send(res, status, e.problems ? { error: e.message, problems: e.problems } : { error: e.message });
  });
});

// ------------------------------------------------------------ websocket

const wss = new WebSocketServer({ noServer: true });

server.on('upgrade', (req, socket, head) => {
  const url = new URL(req.url, 'http://localhost');
  const m = url.pathname.match(/^\/api\/games\/([^/]+)\/ws$/);
  const user = userOf(req);
  const g = m && games.get(Number(decodeURIComponent(m[1])));
  if (!m || !user || !g) {
    socket.write(`HTTP/1.1 ${!m || !g ? 404 : 401} \r\n\r\n`);
    socket.destroy();
    return;
  }
  wss.handleUpgrade(req, socket, head, (ws) => {
    ws.userId = user.id;
    g.sockets.add(ws);
    const seat = seatOf(g, user.id);
    ws.send(g.status === 'waiting' ? JSON.stringify({ type: 'lobby', game: gamePublic(g, user.id) }) : stateMessage(g, seat));
    ws.on('message', (raw) => {
      let msg;
      try {
        msg = JSON.parse(String(raw));
      } catch {
        return ws.send(JSON.stringify({ type: 'error', message: 'bad JSON' }));
      }
      if (msg.type === 'ping') return ws.send(JSON.stringify({ type: 'pong' }));
      if (msg.type !== 'action') return ws.send(JSON.stringify({ type: 'error', message: `unknown message type ${msg.type}` }));
      if (g.status !== 'playing' || !g.game) return ws.send(JSON.stringify({ type: 'error', message: 'the game has not started' }));
      const mySeat = seatOf(g, user.id);
      if (mySeat === null) return ws.send(JSON.stringify({ type: 'error', message: 'spectators cannot act' }));
      try {
        g.game.apply(mySeat, JSON.stringify(msg.action));
      } catch (e) {
        return ws.send(JSON.stringify({ type: 'error', message: e.message }));
      }
      g.actions.push({ seq: g.actions.length, seat: mySeat, action: msg.action });
      const st = JSON.parse(g.game.state());
      if (st.phase === 'Ended') {
        g.status = 'finished';
        g.winner = st.winner;
      }
      broadcast(g, (s) => stateMessage(g, s));
    });
    ws.on('close', () => g.sockets.delete(ws));
  });
});

server.listen(PORT, () => {
  console.log(`mock server on http://localhost:${PORT} with ${catalogues.size} catalogue${catalogues.size === 1 ? '' : 's'}: ${[...catalogues.keys()].join(', ')}`);
});
