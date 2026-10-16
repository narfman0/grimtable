// Drives the built bundle's screens in jsdom against the mock server:
// register, home, the army editor (add units, detachment, Warlord, save),
// create a game, lobby, start, muster, battle. Run: npm run build && npm run ui-smoke.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { JSDOM } from 'jsdom';
import WsClient from 'ws';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const dist = path.join(root, 'dist');
const PORT = Number(process.env.PORT ?? 8091);
const base = `http://localhost:${PORT}`;

const bundle = fs.readdirSync(path.join(dist, 'assets')).find((f) => f.startsWith('index-') && f.endsWith('.js'));
assert.ok(bundle, 'no built bundle; run npm run build first');

// ------------------------------------------------------------ mock server

const server = spawn(process.execPath, [path.join(root, 'scripts', 'mock-server.mjs')], { env: { ...process.env, PORT: String(PORT) }, stdio: ['ignore', 'pipe', 'inherit'] });
server.stdout.on('data', (d) => process.stdout.write(`[mock] ${d}`));
const stop = () => {
  if (!server.killed) server.kill();
};
process.on('exit', stop);
for (let i = 0; ; i++) {
  try {
    if ((await fetch(`${base}/api/catalogue`)).ok) break;
  } catch {
    // not up yet
  }
  if (i > 100) throw new Error('mock server did not start');
  await new Promise((r) => setTimeout(r, 100));
}

// ------------------------------------------------------------ a browser

const dom = new JSDOM('<!doctype html><html><body><div id="app"></div><div id="toast" hidden></div></body></html>', { url: `${base}/`, pretendToBeVisual: true });
const { window } = dom;
const nodeFetch = globalThis.fetch;
let cookie = '';

// Relative and /api URLs go to the mock with the cookie jar; file: URLs
// (the wasm next to the bundle) and /data come from disk.
globalThis.fetch = async (input, init = {}) => {
  const url = typeof input === 'string' ? input : input.url ?? String(input);
  if (url.startsWith('file:')) {
    // Vite emits the wasm as "/assets/<hash>.wasm": relative to dist when run from disk.
    let p = fileURLToPath(url);
    if (!fs.existsSync(p)) p = path.join(dist, new URL(url).pathname);
    return new Response(fs.readFileSync(p), { headers: { 'content-type': p.endsWith('.wasm') ? 'application/wasm' : 'application/octet-stream' } });
  }
  const abs = new URL(url, base);
  if (abs.pathname.startsWith('/data/')) {
    const p = path.join(root, 'public', abs.pathname);
    return fs.existsSync(p) ? new Response(fs.readFileSync(p)) : new Response('not found', { status: 404 });
  }
  const headers = new Headers(init.headers ?? {});
  if (cookie) headers.set('cookie', cookie);
  const r = await nodeFetch(abs, { ...init, headers, credentials: undefined });
  const set = r.headers.get('set-cookie');
  if (set) cookie = set.split(';')[0];
  return r;
};

class CookieWebSocket extends WsClient {
  constructor(url) {
    super(url, { headers: { cookie } });
  }
}
for (const k of ['document', 'HTMLElement', 'HTMLInputElement', 'HTMLSelectElement', 'HTMLButtonElement', 'HTMLCanvasElement', 'Node', 'Element', 'Event', 'KeyboardEvent', 'MouseEvent', 'CustomEvent', 'MessageEvent', 'MutationObserver', 'ResizeObserver', 'requestAnimationFrame', 'cancelAnimationFrame']) {
  if (k === 'ResizeObserver') globalThis.ResizeObserver = class { observe() {} disconnect() {} };
  else if (k === 'requestAnimationFrame') globalThis.requestAnimationFrame = (cb) => setTimeout(() => cb(Date.now()), 16);
  else if (k === 'cancelAnimationFrame') globalThis.cancelAnimationFrame = (id) => clearTimeout(id);
  else globalThis[k] = window[k];
}
globalThis.window = window;
globalThis.location = window.location;
globalThis.history = window.history;
Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
globalThis.localStorage = window.localStorage;
globalThis.WebSocket = CookieWebSocket;
window.WebSocket = CookieWebSocket;
window.confirm = () => true;
window.requestAnimationFrame = globalThis.requestAnimationFrame;
window.ResizeObserver = globalThis.ResizeObserver;
globalThis.confirm = window.confirm;
// A 2D context that accepts every call and returns nothing: the board draws into it.
const noop = new Proxy({}, { get: (_, k) => (k === 'measureText' ? () => ({ width: 10 }) : () => noop), set: () => true });
window.HTMLCanvasElement.prototype.getContext = () => noop;
window.HTMLCanvasElement.prototype.getBoundingClientRect = () => ({ left: 0, top: 0, width: 900, height: 600, right: 900, bottom: 600 });

const app = () => window.document.getElementById('app');
const text = () => app().textContent ?? '';
async function until(what, pred, ms = 10000) {
  const t0 = Date.now();
  while (Date.now() - t0 < ms) {
    if (pred()) return;
    await new Promise((r) => setTimeout(r, 25));
  }
  throw new Error(`timed out waiting for ${what}; screen: ${text().slice(0, 400)}`);
}
const q = (sel) => app().querySelector(sel);
const qa = (sel) => [...app().querySelectorAll(sel)];
const buttonNamed = (label) => qa('button').find((b) => b.textContent.trim() === label) ?? assert.fail(`no button "${label}"; buttons: ${qa('button').map((b) => b.textContent.trim()).join(' | ')}`);
const click = (el) => el.dispatchEvent(new window.MouseEvent('click', { bubbles: true }));
function setInput(el, value) {
  el.value = value;
  el.dispatchEvent(new window.Event('input', { bubbles: true }));
  el.dispatchEvent(new window.Event('change', { bubbles: true }));
}
function setSelect(el, value) {
  el.value = value;
  el.dispatchEvent(new window.Event('change', { bubbles: true }));
}

// ------------------------------------------------------------ run

await import(pathToFileURL(path.join(dist, 'assets', bundle)).href);

// Login screen, register.
await until('the login form', () => q('form.loginform'));
const suffix = Date.now().toString(36);
click(buttonNamed('Create an account instead'));
setInput(q('input[type=text]'), `ui-${suffix}`);
setInput(q('input[type=password]'), 'pw');
q('form.loginform').dispatchEvent(new window.Event('submit', { bubbles: true, cancelable: true }));
await until('the home screen', () => text().includes('My armies') && q('.home'));
console.log('home: signed in');

// Home: no armies, new army for Mechanicus.
await until('faction list', () => qa('.home select option').some((o) => o.value === 'adeptus-mechanicus'));
setSelect(qa('.home select').find((s) => [...s.options].some((o) => o.value === 'adeptus-mechanicus')), 'adeptus-mechanicus');
click(buttonNamed('New army'));
await until('the editor', () => q('.editor .sheets'));
console.log('editor: catalogue loaded');

// Editor: add a Dominus and Rangers, pick a detachment, Warlord, rename, save.
const sheetItem = (name) => qa('.editor .sheets li').find((li) => li.querySelector('.name')?.textContent === name) ?? assert.fail(`no datasheet ${name}`);
click(sheetItem('Tech-Priest Dominus'));
click(sheetItem('Skitarii Rangers'));
await until('two units', () => qa('.editor .unit').length === 2);
const detachmentBox = qa('.editor .detachments input')[0];
detachmentBox.checked = true;
detachmentBox.dispatchEvent(new window.Event('change', { bubbles: true }));
await until('detachment points', () => /[1-3] \/ 3 DP/.test(text()));
const warlord = q('.editor input[name=warlord]');
warlord.checked = true;
warlord.dispatchEvent(new window.Event('change', { bubbles: true }));
await until('warlord badge', () => text().includes('Warlord'));
// Rangers: tick a non-default weapon on the Alpha row.
const rangers = qa('.editor .unit')[1];
const extra = [...rangers.querySelectorAll('.weapon input')].find((i) => !i.checked);
if (extra) {
  extra.checked = true;
  extra.dispatchEvent(new window.Event('change', { bubbles: true }));
}
setInput(q('.editor .list input[type=text]'), `UI army ${suffix}`);
assert.ok(/\d+ \/ 2000 pts/.test(text()), 'running points total');
click(buttonNamed('Save army'));
await until('the save to land', () => /no problems|problem/.test(text()) && location.hash.startsWith('#/army/'));
assert.ok(text().includes('The server reports no problems'), `expected a clean army, got: ${q('.problems')?.textContent}`);
click(buttonNamed('Pack preview'));
await until('the pack preview', () => q('.packpreview'));
console.log(`editor: saved ${location.hash}, pack preview: ${q('.packpreview .muted').textContent}`);

// Home again: the army is listed with the editor badge.
click(buttonNamed('← Home'));
await until('home with the army', () => q('.home table.list') && text().includes(`UI army ${suffix}`));
await until('the problems badge', () => text().includes('ok'));
assert.ok(text().includes('editor'));

// Create a closed game.
await until('the create form', () => qa('.home select').length >= 3);
setSelect(qa('.home select').find((s) => [...s.options].some((o) => o.value === 'closed')), 'closed');
click(buttonNamed('Create game'));
await until('the lobby', () => q('.lobby .code'));
const code = q('.lobby .code').textContent.trim();
assert.equal(code.length, 6);
assert.ok(buttonNamed('Start battle').disabled, 'start is disabled with one seat');
console.log(`lobby: code ${code}`);

// A second player joins through the API with the Custodes fixture.
let cookieB = '';
async function callB(method, p, body) {
  const r = await nodeFetch(base + p, { method, headers: { 'content-type': 'application/json', ...(cookieB ? { cookie: cookieB } : {}) }, body: body === undefined ? undefined : JSON.stringify(body) });
  const set = r.headers.get('set-cookie');
  if (set) cookieB = set.split(';')[0];
  const j = await r.json().catch(() => null);
  if (!r.ok) throw new Error(`B ${p}: ${r.status} ${j?.error}`);
  return j;
}
await callB('POST', '/api/register', { username: `ui-b-${suffix}`, password: 'pw' });
const ros = fs.readFileSync(path.join(root, 'public', 'data', 'fixtures', 'custodes.ros'), 'utf8');
const armyB = await callB('POST', '/api/armies/import', { name: 'Custodes', ros });
const gameB = await callB('POST', '/api/games/join', { join_code: code, army_id: armyB.id });
await until('the second seat', () => text().includes(`ui-b-${suffix}`) && !buttonNamed('Start battle').disabled);
const wsB = new WsClient(`${base.replace('http', 'ws')}/api/games/${gameB.id}/ws`, { headers: { cookie: cookieB } });
const msgsB = [];
wsB.on('message', (m) => msgsB.push(JSON.parse(String(m))));
await new Promise((r) => wsB.once('open', r));

// Start: muster screen for the host; B finishes over the socket.
click(buttonNamed('Start battle'));
await until('the muster screen', () => q('.muster') && text().includes('Muster'));
assert.ok(text().includes('you'), 'muster marks the local seat');
const doneBtn = qa('.muster button').find((b) => b.textContent.endsWith(' done'));
assert.ok(doneBtn, 'the host has a done button');
assert.equal(qa('.muster button').filter((b) => b.textContent.endsWith(' done')).length, 1, 'only the local seat can finish');
click(doneBtn);
await until('host done', () => text().includes('done, waiting'));
wsB.send(JSON.stringify({ type: 'action', action: { type: 'finish_muster' } }));
await until('the battle screen', () => q('.battle') && text().includes('Deployment'));
assert.ok(text().includes('connected'), 'connection status shown');
assert.ok(text().includes(`ui-b-${suffix}`) || text().includes('opponent'), 'opponent shown');
console.log(`battle: ${q('.panel .toact').textContent}`);
// Concede through the header button (confirm stubbed), then Leave: back
// home with the game listed as finished.
window.confirm = () => true;
click(buttonNamed('Concede'));
await until('the battle ended', () => text().includes('Final result'));
click(buttonNamed('Leave'));
await until('home after leaving', () => q('.home'));
await until('the game listed as finished', () => text().includes('finished'));
console.log('home: game listed as finished after conceding');

// Hotseat still works: fixtures for both seats, start, muster, battle.
click(buttonNamed('Local hotseat'));
await until('the hotseat setup', () => q('.setup .slots'));
const slots = qa('.setup .slots select');
setSelect(slots[0], 'fixtures/mechanicus.ros');
setSelect(slots[1], 'fixtures/custodes.ros');
await until('both packs built', () => text().includes('Mechanicus') && text().includes('Custodes') && qa('.setup .summary b').length === 2);
click(buttonNamed('Start battle'));
await until('the hotseat muster', () => q('.muster') && location.hash === '#/hotseat/game');
assert.equal(qa('.muster button').filter((b) => b.textContent.endsWith(' done')).length, 2, 'hotseat: both seats act');
for (const b of qa('.muster button').filter((b) => b.textContent.endsWith(' done'))) click(b);
await until('the hotseat battle', () => q('.battle') && text().includes('Deployment'));
assert.ok(!text().includes('pass-device'), 'no pass-device overlay');
assert.ok(window.localStorage.getItem('grimtable.save'), 'hotseat games are saved locally');
console.log(`hotseat: ${q('.panel .toact').textContent}`);

wsB.close();
stop();
console.log('ui smoke ok');
process.exit(0);
