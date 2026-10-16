// An online game: the lobby while waiting, then muster and battle over the
// socket. Owns the GameConnection for the screen's lifetime.

import { ApiError, api, errorText, GameConnection, startProblems } from './api';
import { opponentOf, patternLabel, statusLabel } from './home';
import type { App } from './main';
import { OnlineSession } from './session';
import type { GameRecord, ServerMessage } from './types';
import { button, h, toast } from './ui';

export function renderGame(app: App, gameId: string): { el: HTMLElement; teardown: () => void } {
  const el = h('div', { class: 'lobby' }, h('p', { class: 'loading' }, 'Connecting…'));
  let record: GameRecord | null = null;
  let session: OnlineSession | null = null;
  let status: 'connecting' | 'open' | 'closed' = 'connecting';
  let starting = false;
  let problems: string[] = [];

  const conn = new GameConnection(gameId, {
    onMessage(msg: ServerMessage) {
      if (msg.type === 'lobby') {
        record = msg.game;
        if (record.status === 'cancelled') {
          toast('the host closed this lobby', 'info');
          app.navigate('#/');
          return;
        }
        if (!session) renderLobby();
        return;
      }
      if (session) {
        session.receive(msg);
        return;
      }
      if (msg.type === 'state') {
        const opp = record ? opponentOf(record) : null;
        try {
          session = new OnlineSession(conn, msg, opp);
        } catch (e) {
          toast(`could not read the game state: ${errorText(e)}`);
          return;
        }
        app.showSession(session);
      } else if (msg.type === 'error') {
        toast(msg.message);
      }
    },
    onStatus(s) {
      status = s;
      if (session) session.connectionChanged();
      else renderLobby();
    },
  });

  void api
    .game(gameId)
    .then((g) => {
      record = g;
      if (!session) renderLobby();
    })
    .catch((e) => {
      el.replaceChildren(h('div', { class: 'setup' }, h('p', { style: 'color:var(--err)' }, errorText(e)), button('Back to home', () => app.navigate('#/'))));
    });

  function renderLobby(): void {
    if (!el.isConnected && el.childElementCount) return;
    const g = record;
    if (!g) {
      el.replaceChildren(h('p', { class: 'loading' }, status === 'closed' ? 'Disconnected; retrying…' : 'Connecting…'));
      return;
    }
    const seats = [0, 1].map((i) => g.seats.find((s) => s.seat === i) ?? null);
    const host = g.my_seat === 0;
    const full = seats.every((s) => s !== null);
    el.replaceChildren(
      h(
        'div',
        { class: 'row', style: 'justify-content:space-between' },
        h('h1', null, 'Lobby'),
        h('div', { class: 'row' }, h('span', { class: `badge ${status === 'open' ? 'ok' : 'warn'}` }, status), button(
            'Leave',
            () => {
              void (async () => {
                if (g.my_seat !== null) {
                  if (host && !confirm('Leaving as host closes this lobby for everyone. Continue?')) return;
                  try {
                    await api.leaveGame(g.id);
                  } catch (e) {
                    toast(errorText(e));
                  }
                }
                app.leaveGame(dummySession());
              })();
            },
            { class: 'small' },
          )),
      ),
      h(
        'div',
        { class: 'card joincode' },
        h('div', { class: 'muted small' }, 'Join code'),
        g.join_code ? h('div', { class: 'row' }, h('span', { class: 'code' }, g.join_code), button('Copy', () => void copy(g.join_code!), { class: 'small' })) : h('div', { class: 'muted' }, 'only shown to the players'),
        h('div', { class: 'muted small' }, `${g.visibility === 'open' ? 'Open lobby: listed for anyone to join' : 'Closed lobby: join by code only'} · ${patternLabel(g.pattern)} · ${g.points_limit} pts · ${statusLabel(g)}${g.host ? ` · host ${g.host}` : ''}`),
      ),
      h(
        'div',
        { class: 'seats' },
        seats.map((s, i) =>
          h(
            'div',
            { class: 'card' },
            h('h2', { class: `p${i}` }, `Seat ${i + 1}`, i === 0 ? h('span', { class: 'badge' }, 'host') : null, g.my_seat === i ? h('span', { class: 'badge ok' }, 'you') : null),
            s ? h('div', null, h('b', null, s.username), h('div', { class: 'muted' }, `${s.army_name} · ${s.army_points} pts`)) : h('div', { class: 'muted' }, 'Waiting for a player…'),
          ),
        ),
      ),
      h(
        'div',
        { class: 'row' },
        host
          ? button(
              starting ? 'Starting…' : 'Start battle',
              async () => {
                starting = true;
                renderLobby();
                const names = seats.map((s) => s?.username ?? null);
                try {
                  const r = await api.startGame(g.id);
                  problems = startProblems(r, names);
                  if (problems.length) toast(`started with ${problems.length} army problem${problems.length === 1 ? '' : 's'}; see the lobby list`, 'info');
                  // The state message follows on the socket.
                } catch (e) {
                  problems = e instanceof ApiError ? startProblems(e.body, names) : [];
                  toast(errorText(e));
                  starting = false;
                  renderLobby();
                }
              },
              { class: 'primary', disabled: !full || starting || g.status !== 'waiting' },
            )
          : h('span', { class: 'muted' }, g.status === 'waiting' ? (full ? 'Waiting for the host to start.' : 'Waiting for a second player.') : 'Waiting for the game state…'),
        g.my_seat === null ? h('span', { class: 'badge warn' }, 'spectating') : null,
      ),
      h('div', null, problems.length ? h('div', { class: 'problems' }, h('b', null, 'Army problems:'), h('ul', null, problems.map((p) => h('li', null, p)))) : null),
    );
  }

  async function copy(text: string): Promise<void> {
    try {
      await navigator.clipboard.writeText(text);
      toast('join code copied', 'info');
    } catch {
      toast('could not copy; select the code by hand');
    }
  }

  /** The lobby has no session yet; leaving just closes the socket. */
  function dummySession() {
    return {
      online: true,
      leave: () => conn.close(),
    } as unknown as OnlineSession;
  }

  return {
    el,
    teardown() {
      // The battle screen replaces this element but keeps the socket; close
      // it only when a different screen takes over.
      if (!session) conn.close();
    },
  };
}
