// Home: my armies, my games, the lobby browser, create game, join by code.

import { api, errorText } from './api';
import type { App } from './main';
import { MISSIONS } from './setup';
import type { ArmyPack, ArmySummary, FactionSummary, GameRecord, Id, OpenGame, Visibility } from './types';
import { button, clear, h, select, toast } from './ui';

export const DEFAULT_POINTS_LIMIT = 2000;

export function patternLabel(id: string): string {
  return MISSIONS.find((m) => m.value === id)?.label ?? id;
}

export function statusLabel(g: GameRecord): string {
  if (g.status === 'finished') return g.winner === null ? 'finished · draw' : g.winner === g.my_seat ? 'finished · you won' : 'finished · you lost';
  if (g.status === 'playing') return 'playing';
  return g.seats.length < 2 ? 'waiting for an opponent' : 'ready to start';
}

export function opponentOf(g: GameRecord): string {
  const other = g.seats.find((s) => s.seat !== g.my_seat);
  return other ? other.username : '—';
}

function when(iso: string | undefined): string {
  if (!iso) return '';
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? iso : d.toLocaleString();
}

export function renderHome(app: App): HTMLElement {
  const user = app.user!;
  let armies: ArmySummary[] = [];
  let factions: FactionSummary[] = [];

  const armiesBox = h('div', null, h('p', { class: 'muted' }, 'Loading…'));
  const gamesBox = h('div', null, h('p', { class: 'muted' }, 'Loading…'));
  const openBox = h('div', null, h('p', { class: 'muted' }, 'Loading…'));
  const createBox = h('div');
  const joinBox = h('div');

  const root = h(
    'div',
    { class: 'home' },
    h(
      'div',
      { class: 'row topbar' },
      h('h1', null, 'grimtable'),
      h('span', { class: 'muted' }, `signed in as `, h('b', null, user.username)),
      h('span', { style: 'flex:1' }),
      button('Local hotseat', () => app.navigate('#/hotseat'), { title: 'Two players, one browser, no account needed' }),
      button('Log out', async () => {
        try {
          await api.logout();
        } catch {
          // The cookie may already be gone.
        }
        app.user = null;
        app.navigate('#/login');
      }),
    ),
    h('div', { class: 'cols' }, h('div', null, h('h2', null, 'My armies'), armiesBox, h('h2', null, 'My games'), gamesBox), h('div', null, h('h2', null, 'Create a game'), createBox, h('h2', null, 'Join by code'), joinBox, h('h2', null, 'Open lobbies'), openBox)),
  );

  // ------------------------------------------------------------ armies

  function armyOptions(): { value: string; label: string }[] {
    return armies.map((a) => ({ value: String(a.id), label: `${a.name} (${a.points} pts${a.problems?.length ? `, ${a.problems.length} problem${a.problems.length === 1 ? '' : 's'}` : ''})` }));
  }

  async function loadArmies(): Promise<void> {
    try {
      armies = await api.armies();
      // The list omits problems; fetch each record for the count.
      await Promise.all(
        armies.map(async (a) => {
          if (a.problems) return;
          try {
            a.problems = (await api.army(a.id)).problems ?? [];
          } catch {
            a.problems = undefined;
          }
        }),
      );
    } catch (e) {
      armiesBox.replaceChildren(h('p', { style: 'color:var(--err)' }, errorText(e)));
      return;
    }
    renderArmies();
    renderCreate();
    renderJoin();
    void loadOpen();
  }

  function renderArmies(): void {
    const fileInput = h('input', { type: 'file', accept: '.ros,.xml', hidden: true });
    fileInput.addEventListener('change', async () => {
      const f = fileInput.files?.[0];
      if (!f) return;
      const name = f.name.replace(/\.(ros|xml)$/i, '');
      try {
        const rec = await api.importArmy(name, await f.text());
        toast(`imported ${rec.name}: ${rec.points} pts${rec.problems.length ? ` · ${rec.problems.join('; ')}` : ''}`, rec.problems.length ? 'error' : 'info');
        await loadArmies();
      } catch (e) {
        toast(errorText(e));
      }
    });
    const factionSel = select([{ value: '', label: factions.length ? '— faction —' : 'loading factions…' }, ...factions.map((f) => ({ value: f.faction_id, label: f.name }))], '', () => undefined);
    const rows = armies.map((a) =>
      h(
        'tr',
        null,
        h('td', null, h('b', null, a.name)),
        h('td', { class: 'muted' }, factions.find((f) => f.faction_id === a.faction_id)?.name ?? a.faction_id),
        h('td', null, `${a.points} pts`),
        h('td', null, h('span', { class: 'badge' }, a.kind === 'def' ? 'editor' : 'imported')),
        h('td', null, a.problems === undefined ? h('span', { class: 'muted' }, '?') : a.problems.length ? h('span', { class: 'badge warn', title: a.problems.join('\n') }, `${a.problems.length} problem${a.problems.length === 1 ? '' : 's'}`) : h('span', { class: 'badge ok' }, 'ok')),
        h(
          'td',
          { class: 'row' },
          a.kind === 'def' ? button('Edit', () => app.navigate(`#/army/${encodeURIComponent(String(a.id))}`), { class: 'small' }) : null,
          button('Pack', () => void showPack(a), { class: 'small', title: 'Preview the engine pack the server builds' }),
          button(
            'Delete',
            async () => {
              if (!confirm(`Delete ${a.name}?`)) return;
              try {
                await api.deleteArmy(a.id);
                await loadArmies();
              } catch (e) {
                toast(errorText(e));
              }
            },
            { class: 'small danger' },
          ),
        ),
      ),
    );
    armiesBox.replaceChildren(
      armies.length ? h('table', { class: 'list' }, h('tbody', null, rows)) : h('p', { class: 'muted' }, 'No armies yet. Build one in the editor or import a New Recruit .ros export.'),
      h(
        'div',
        { class: 'row', style: 'margin-top:8px' },
        factionSel,
        button(
          'New army',
          () => {
            if (!factionSel.value) {
              toast('pick a faction first');
              return;
            }
            app.navigate(`#/army/new/${encodeURIComponent(factionSel.value)}`);
          },
          { class: 'primary' },
        ),
        button('Import .ros', () => fileInput.click()),
        fileInput,
      ),
    );
  }

  async function showPack(a: ArmySummary): Promise<void> {
    let pack: ArmyPack;
    try {
      pack = await api.armyPack(a.id);
    } catch (e) {
      toast(errorText(e));
      return;
    }
    const models = pack.units.reduce((n, u) => n + u.models.reduce((k, m) => k + m.count, 0), 0);
    const box = h(
      'div',
      { class: 'card packpreview' },
      h('div', { class: 'row', style: 'justify-content:space-between' }, h('b', null, pack.name), button('close', () => box.remove(), { class: 'small' })),
      h('div', { class: 'muted' }, `${pack.faction}${pack.detachment ? ' · ' + pack.detachment : ''} · ${pack.points} pts · ${pack.units.length} units · ${models} models`),
      h('div', { class: 'small' }, 'Army rules: ', pack.army_rules.length ? pack.army_rules.map((r) => h('span', { class: `badge ${r.effect ? 'ok' : 'warn'}` }, r.name)) : h('span', { class: 'muted' }, 'none')),
      h('ul', { class: 'small' }, pack.units.map((u) => h('li', null, `${u.name} · ${u.points} pts · ${u.models.reduce((k, m) => k + m.count, 0)} models`))),
    );
    armiesBox.prepend(box);
  }

  // ------------------------------------------------------------ games

  async function loadGames(): Promise<void> {
    let games: GameRecord[];
    try {
      games = await api.myGames();
    } catch (e) {
      gamesBox.replaceChildren(h('p', { style: 'color:var(--err)' }, errorText(e)));
      return;
    }
    if (!games.length) {
      gamesBox.replaceChildren(h('p', { class: 'muted' }, 'No games yet. Create one or join a lobby.'));
      return;
    }
    const order = { playing: 0, waiting: 1, finished: 2 };
    games.sort((a, b) => order[a.status] - order[b.status]);
    gamesBox.replaceChildren(
      h(
        'table',
        { class: 'list' },
        h(
          'tbody',
          null,
          games.map((g) =>
            h(
              'tr',
              null,
              h('td', null, h('b', null, `vs ${opponentOf(g)}`)),
              h('td', { class: 'muted' }, `${patternLabel(g.pattern)} · ${g.points_limit} pts`),
              h('td', null, h('span', { class: `badge ${g.status === 'playing' ? 'ok' : ''}` }, statusLabel(g))),
              h('td', { class: 'muted small' }, g.join_code ?? ''),
              h('td', null, button(g.status === 'waiting' ? 'Open' : 'Resume', () => app.navigate(`#/game/${encodeURIComponent(String(g.id))}`), { class: 'small primary' })),
            ),
          ),
        ),
      ),
    );
  }

  // ------------------------------------------------------------ create

  function renderCreate(): void {
    if (!armies.length) {
      createBox.replaceChildren(h('p', { class: 'muted' }, 'You need an army first.'));
      return;
    }
    let visibility: Visibility = 'open';
    let pattern = 'dawn-of-war';
    let armyId: Id = armies[0].id;
    const limit = h('input', { type: 'number', value: String(DEFAULT_POINTS_LIMIT), min: 500, max: 5000, step: 50, style: 'width:90px' });
    createBox.replaceChildren(
      h(
        'div',
        { class: 'card' },
        h(
          'div',
          { class: 'options' },
          h(
            'label',
            null,
            'Visibility',
            select(
              [
                { value: 'open', label: 'Open (listed in the lobby browser)' },
                { value: 'closed', label: 'Closed (join code only)' },
              ],
              visibility,
              (v) => (visibility = v as Visibility),
            ),
          ),
          h('label', null, 'Deployment', select(MISSIONS, pattern, (v) => (pattern = v))),
          h('label', null, 'Points limit', limit),
          h('label', null, 'Army', select(armyOptions(), String(armyId), (v) => (armyId = v))),
        ),
        button(
          'Create game',
          async () => {
            try {
              const g = await api.createGame(visibility, pattern, Number(limit.value) || DEFAULT_POINTS_LIMIT, armyId);
              app.navigate(`#/game/${encodeURIComponent(String(g.id))}`);
            } catch (e) {
              toast(errorText(e));
            }
          },
          { class: 'primary' },
        ),
      ),
    );
  }

  function renderJoin(): void {
    if (!armies.length) {
      clear(joinBox);
      return;
    }
    let armyId: Id = armies[0].id;
    const code = h('input', { type: 'text', placeholder: 'join code', maxlength: 6, style: 'width:110px;text-transform:uppercase;font-family:ui-monospace,monospace' });
    joinBox.replaceChildren(
      h(
        'div',
        { class: 'card row' },
        code,
        h('label', null, 'with ', select(armyOptions(), String(armyId), (v) => (armyId = v))),
        button(
          'Join',
          async () => {
            const c = code.value.trim().toUpperCase();
            if (c.length !== 6) {
              toast('join codes are 6 characters');
              return;
            }
            try {
              const g = await api.joinByCode(c, armyId);
              app.navigate(`#/game/${encodeURIComponent(String(g.id))}`);
            } catch (e) {
              toast(errorText(e));
            }
          },
          { class: 'primary' },
        ),
      ),
    );
  }

  // ------------------------------------------------------------ open lobbies

  async function loadOpen(): Promise<void> {
    let open: OpenGame[];
    try {
      open = await api.openGames();
    } catch (e) {
      openBox.replaceChildren(h('p', { style: 'color:var(--err)' }, errorText(e)));
      return;
    }
    const mine = open.filter((g) => g.host !== user.username);
    if (!mine.length) {
      openBox.replaceChildren(h('p', { class: 'muted' }, 'No open lobbies right now.'), button('Refresh', () => void loadOpen(), { class: 'small' }));
      return;
    }
    let armyId: Id | '' = armies[0]?.id ?? '';
    openBox.replaceChildren(
      armies.length ? h('div', { class: 'row small' }, h('label', null, 'Join with '), select(armyOptions(), String(armyId), (v) => (armyId = v))) : h('p', { class: 'muted' }, 'You need an army to join.'),
      h(
        'table',
        { class: 'list' },
        h(
          'tbody',
          null,
          mine.map((g) =>
            h(
              'tr',
              null,
              h('td', null, h('b', null, g.host)),
              h('td', { class: 'muted' }, `${patternLabel(g.pattern)} · ${g.points_limit} pts`),
              h('td', { class: 'muted small' }, when(g.created_at)),
              h(
                'td',
                null,
                button(
                  'Join',
                  async () => {
                    if (armyId === '') return;
                    try {
                      const rec = await api.joinOpen(g.id, armyId);
                      app.navigate(`#/game/${encodeURIComponent(String(rec.id))}`);
                    } catch (e) {
                      toast(errorText(e));
                    }
                  },
                  { class: 'small primary', disabled: !armies.length },
                ),
              ),
            ),
          ),
        ),
      ),
      button('Refresh', () => void loadOpen(), { class: 'small' }),
    );
  }

  void (async () => {
    try {
      factions = await api.factions();
    } catch (e) {
      toast(`catalogue: ${errorText(e)}`);
    }
    await loadArmies();
  })();
  void loadGames();

  return root;
}
