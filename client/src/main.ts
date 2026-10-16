// Entry point and hash router.
//
//   #/login           sign in or register
//   #/                home: armies, games, lobby browser
//   #/army/new/:fid   army editor for a faction catalogue
//   #/army/:id        army editor for a saved army
//   #/game/:id        lobby, then the muster and battle screens over the socket
//   #/hotseat         the local two-players-one-browser mode
//   #/hotseat/game    the running hotseat game (in memory; reload offers Resume)

import { api } from './api';
import { renderBattle } from './battle';
import { renderEditor } from './editor';
import { Game, initWasm } from './engine';
import { renderGame } from './game';
import { renderHome } from './home';
import { renderLogin } from './login';
import { renderMuster } from './muster';
import { LocalSession, type Session } from './session';
import { renderSetup } from './setup';
import type { User } from './types';
import { clear, h } from './ui';

export interface App {
  root: HTMLElement;
  user: User | null;
  navigate(hash: string): void;
  /** Hotseat setup screen. */
  showSetup(): void;
  /** Start (or resume) a hotseat game from a wasm Game. */
  startGame(game: Game): void;
  /** Show the right screen for a session's phase (muster or battle). */
  showSession(session: Session): void;
  /** Leave the current game: hotseat goes back to setup, online to home. */
  leaveGame(session: Session): void;
}

/** A screen may return a teardown; it runs before the next screen renders. */
type Teardown = () => void;

async function main(): Promise<void> {
  const found = document.getElementById('app');
  if (!found) throw new Error('no #app');
  const root: HTMLElement = found;
  try {
    await initWasm();
  } catch (e) {
    clear(root);
    root.appendChild(
      h('div', { class: 'setup' }, h('h1', null, 'grimtable'), h('p', { class: 'muted' }, 'Could not load the engine. Build it with: wasm-pack build crates/wasm --target web --out-dir ../../client/src/wasm'), h('pre', null, String(e))),
    );
    return;
  }

  let local: LocalSession | null = null;
  let teardown: Teardown | null = null;
  let active: Session | null = null;

  function show(el: HTMLElement, down: Teardown | null = null): void {
    teardown?.();
    teardown = down;
    clear(root);
    root.appendChild(el);
  }

  const app: App = {
    root,
    user: null,
    navigate(hash: string) {
      if (location.hash === hash) void route();
      else location.hash = hash;
    },
    showSetup() {
      local = null;
      app.navigate('#/hotseat');
    },
    startGame(game: Game) {
      local = new LocalSession(game);
      local.persist();
      if (location.hash !== '#/hotseat/game') {
        location.hash = '#/hotseat/game';
        // The hashchange handler renders the session.
        return;
      }
      app.showSession(local);
    },
    showSession(session: Session) {
      active = session;
      if (session.state.phase === 'Muster') show(renderMuster(app, session));
      else show(renderBattle(app, session));
    },
    leaveGame(session: Session) {
      session.leave();
      if (session.online) app.navigate('#/');
      else app.showSetup();
    },
  };

  try {
    app.user = await api.me();
  } catch {
    app.user = null;
  }

  async function route(): Promise<void> {
    // Any route change ends the current online session (sockets included).
    if (active?.online) active.leave();
    active = null;
    const hash = location.hash || '#/';
    const parts = hash.replace(/^#\/?/, '').split('/').map(decodeURIComponent);
    const head = parts[0] ?? '';
    if (head === 'hotseat') {
      if (parts[1] === 'game') {
        if (local) {
          app.showSession(local);
          return;
        }
        location.hash = '#/hotseat';
        return;
      }
      local = null;
      show(renderSetup(app));
      return;
    }
    if (head === 'login') {
      show(renderLogin(app));
      return;
    }
    if (!app.user) {
      location.hash = '#/login';
      return;
    }
    if (head === 'army') {
      if (parts[1] === 'new') show(renderEditor(app, { factionId: parts[2] ?? '' }));
      else show(renderEditor(app, { armyId: parts[1] ?? '' }));
      return;
    }
    if (head === 'game' && parts[1]) {
      const screen = renderGame(app, parts[1]);
      show(screen.el, screen.teardown);
      return;
    }
    show(renderHome(app));
  }

  window.addEventListener('hashchange', () => void route());
  await route();
}

void main();
