import { renderBattle } from './battle';
import { Game, initWasm } from './engine';
import { renderMuster } from './muster';
import { Session } from './session';
import { renderSetup } from './setup';
import { clear, h } from './ui';

export interface App {
  root: HTMLElement;
  showSetup(): void;
  startGame(game: Game): void;
}

async function main(): Promise<void> {
  const root = document.getElementById('app');
  if (!root) throw new Error('no #app');
  try {
    await initWasm();
  } catch (e) {
    clear(root);
    root.appendChild(
      h('div', { class: 'setup' }, h('h1', null, 'grimtable'), h('p', { class: 'muted' }, 'Could not load the engine. Build it with: wasm-pack build crates/wasm --target web --out-dir ../../client/src/wasm'), h('pre', null, String(e))),
    );
    return;
  }

  let session: Session | null = null;

  const app: App = {
    root,
    showSetup() {
      session = null;
      clear(root);
      root.appendChild(renderSetup(app));
    },
    startGame(game: Game) {
      session = new Session(game);
      session.persist();
      clear(root);
      if (session.state.phase === 'Muster') root.appendChild(renderMuster(app, session));
      else root.appendChild(renderBattle(app, session));
    },
  };
  app.showSetup();
}

void main();
