// HTTP and WebSocket access to the server (docs/multiplayer.md). Every call
// sends the session cookie; JSON error bodies become ApiError messages.

import type { ArmyDef, ArmyPack, ArmyRecord, ArmySummary, Catalogue, FactionSummary, GameRecord, Id, OpenGame, ServerMessage, User, Visibility } from './types';

export class ApiError extends Error {
  constructor(
    readonly status: number,
    message: string,
    /** The parsed JSON error body, when there was one (e.g. `problems` on a refused start). */
    readonly body: Record<string, unknown> | null = null,
  ) {
    super(message);
  }
}

async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  let r: Response;
  try {
    r = await fetch(path, {
      method,
      credentials: 'include',
      headers: body === undefined ? {} : { 'content-type': 'application/json' },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
  } catch (e) {
    throw new ApiError(0, `network error: ${e instanceof Error ? e.message : String(e)}`);
  }
  const text = await r.text();
  let json: unknown = null;
  if (text) {
    try {
      json = JSON.parse(text);
    } catch {
      json = null;
    }
  }
  if (!r.ok) {
    const o = (json ?? {}) as Record<string, unknown>;
    const msg = typeof o.error === 'string' ? o.error : typeof o.message === 'string' ? o.message : text.trim() || `${r.status} ${r.statusText}`;
    throw new ApiError(r.status, msg, json && typeof json === 'object' ? (json as Record<string, unknown>) : null);
  }
  return json as T;
}

const enc = (id: Id) => encodeURIComponent(String(id));

export const api = {
  // accounts
  register: (username: string, password: string) => request<User>('POST', '/api/register', { username, password }),
  login: (username: string, password: string) => request<User>('POST', '/api/login', { username, password }),
  logout: () => request<void>('POST', '/api/logout'),
  me: () => request<User>('GET', '/api/me'),

  // armies
  armies: () => request<ArmySummary[]>('GET', '/api/armies'),
  army: (id: Id) => request<ArmyRecord>('GET', `/api/armies/${enc(id)}`),
  createArmy: (name: string, def: ArmyDef) => request<ArmyRecord>('POST', '/api/armies', { name, kind: 'def', def }),
  updateArmy: (id: Id, name: string, def: ArmyDef) => request<ArmyRecord>('PUT', `/api/armies/${enc(id)}`, { name, def }),
  importArmy: (name: string, ros: string) => request<ArmyRecord>('POST', '/api/armies/import', { name, ros }),
  deleteArmy: (id: Id) => request<void>('DELETE', `/api/armies/${enc(id)}`),
  armyPack: (id: Id) => request<ArmyPack>('GET', `/api/armies/${enc(id)}/pack`),
  factions: () => request<FactionSummary[]>('GET', '/api/catalogue'),
  catalogue: (factionId: string) => request<Catalogue>('GET', `/api/catalogue/${encodeURIComponent(factionId)}`),

  // games
  createGame: (visibility: Visibility, pattern: string, points_limit: number, army_id: Id) => request<GameRecord>('POST', '/api/games', { visibility, pattern, points_limit, army_id }),
  openGames: () => request<OpenGame[]>('GET', '/api/games/open'),
  myGames: () => request<GameRecord[]>('GET', '/api/games/mine'),
  game: (id: Id) => request<GameRecord>('GET', `/api/games/${enc(id)}`),
  joinByCode: (join_code: string, army_id: Id) => request<GameRecord>('POST', '/api/games/join', { join_code, army_id }),
  joinOpen: (id: Id, army_id: Id) => request<GameRecord>('POST', `/api/games/${enc(id)}/join`, { army_id }),
  startGame: (id: Id) => request<GameRecord | void>('POST', `/api/games/${enc(id)}/start`),
};

/** Per-seat problems a start response or refusal carries, flattened with seat labels. */
export function startProblems(x: unknown, seatNames: (string | null)[]): string[] {
  const o = x as { problems?: unknown } | null;
  if (!o || !Array.isArray(o.problems)) return [];
  const out: string[] = [];
  (o.problems as unknown[]).forEach((list, i) => {
    if (!Array.isArray(list)) return;
    for (const p of list) out.push(`${seatNames[i] ?? `seat ${i + 1}`}: ${String(p)}`);
  });
  return out;
};

export function errorText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

// ------------------------------------------------------------ WebSocket

export type ConnectionStatus = 'connecting' | 'open' | 'closed';

export interface ConnectionHandlers {
  onMessage(msg: ServerMessage): void;
  onStatus(status: ConnectionStatus): void;
}

/**
 * The game socket. Reconnects with backoff until `close()` is called; the
 * server resends the current lobby or state on every connect.
 */
export class GameConnection {
  status: ConnectionStatus = 'connecting';
  private ws: WebSocket | null = null;
  private closed = false;
  private attempts = 0;
  private pingTimer: number | undefined;
  private reconnectTimer: number | undefined;

  constructor(
    readonly gameId: Id,
    private readonly handlers: ConnectionHandlers,
  ) {
    this.connect();
  }

  private url(): string {
    const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
    return `${proto}//${location.host}/api/games/${enc(this.gameId)}/ws`;
  }

  private setStatus(s: ConnectionStatus): void {
    if (this.status === s) return;
    this.status = s;
    this.handlers.onStatus(s);
  }

  private connect(): void {
    if (this.closed) return;
    this.setStatus('connecting');
    let ws: WebSocket;
    try {
      ws = new WebSocket(this.url());
    } catch {
      this.scheduleReconnect();
      return;
    }
    this.ws = ws;
    ws.onopen = () => {
      this.attempts = 0;
      this.setStatus('open');
      this.pingTimer = window.setInterval(() => this.sendRaw({ type: 'ping' }), 25000);
    };
    ws.onmessage = (ev: MessageEvent) => {
      let msg: ServerMessage;
      try {
        msg = JSON.parse(String(ev.data)) as ServerMessage;
      } catch {
        return;
      }
      if (msg && typeof msg === 'object' && msg.type !== 'pong') this.handlers.onMessage(msg);
    };
    ws.onclose = () => {
      if (this.pingTimer) window.clearInterval(this.pingTimer);
      this.pingTimer = undefined;
      this.ws = null;
      this.setStatus('closed');
      this.scheduleReconnect();
    };
    ws.onerror = () => {
      // onclose follows and handles the retry.
    };
  }

  private scheduleReconnect(): void {
    if (this.closed) return;
    const delay = Math.min(10000, 500 * 2 ** Math.min(this.attempts, 5));
    this.attempts++;
    this.reconnectTimer = window.setTimeout(() => this.connect(), delay);
  }

  private sendRaw(obj: unknown): boolean {
    if (!this.ws || this.ws.readyState !== WebSocket.OPEN) return false;
    this.ws.send(JSON.stringify(obj));
    return true;
  }

  /** Send an engine action; false when the socket is not open. */
  sendAction(action: Record<string, unknown>): boolean {
    return this.sendRaw({ type: 'action', action });
  }

  close(): void {
    this.closed = true;
    if (this.reconnectTimer) window.clearTimeout(this.reconnectTimer);
    if (this.pingTimer) window.clearInterval(this.pingTimer);
    const ws = this.ws;
    this.ws = null;
    if (ws) {
      ws.onclose = null;
      ws.close();
    }
    this.status = 'closed';
  }
}
