// A running game as the screens see it: the parsed state, a wasm `Game` for
// read-only queries, and `apply`. Two implementations: hotseat (the wasm
// engine applies actions locally and the state is saved to localStorage) and
// online (actions go to the server, which answers with the next state; the
// local `Game` is a mirror restored from every state message).

import type { GameConnection } from './api';
import { Game, readState, tryApply } from './engine';
import { saveGame } from './storage';
import type { GameState, ServerMessage } from './types';
import { toast } from './ui';

export type ConnectionState = 'local' | 'connecting' | 'open' | 'closed';

export interface Session {
  readonly online: boolean;
  /** The local player's seat online; null in hotseat (either seat acts). */
  readonly seat: number | null;
  readonly state: GameState;
  readonly json: string;
  /** The engine (hotseat) or a read-only mirror of the server state (online). */
  readonly game: Game;
  readonly opponentName: string | null;
  readonly connection: ConnectionState;
  /** Called when the state or connection changes for a reason other than a local apply. */
  onChange: (() => void) | null;
  /** Apply an action for `player`; rule errors are toasted; resolves false on failure. */
  apply(player: number, action: Record<string, unknown>): Promise<boolean>;
  canAct(player: number): boolean;
  playerToAct(): number;
  nextFighter(): number;
  leave(): void;
}

export class LocalSession implements Session {
  readonly online = false;
  readonly seat = null;
  readonly opponentName = null;
  readonly connection = 'local';
  onChange: (() => void) | null = null;
  json = '';
  state!: GameState;

  constructor(readonly game: Game) {
    this.refresh(false);
  }

  refresh(persist = true): void {
    const r = readState(this.game);
    this.json = r.json;
    this.state = r.state;
    if (persist) this.persist();
  }

  persist(): void {
    const s = this.state;
    saveGame(this.json, { players: s.players.map((p) => p.name), phase: s.phase, round: s.round, savedAt: Date.now() });
  }

  apply(player: number, action: Record<string, unknown>): Promise<boolean> {
    const err = tryApply(this.game, player, action);
    if (err) {
      toast(err);
      return Promise.resolve(false);
    }
    this.refresh();
    return Promise.resolve(true);
  }

  canAct(): boolean {
    return true;
  }

  playerToAct(): number {
    return this.game.player_to_act();
  }

  nextFighter(): number {
    return this.game.next_fighter();
  }

  leave(): void {
    // Nothing to close; the save stays until the player discards it.
  }
}

const ACTION_TIMEOUT_MS = 15000;

export type StateMessage = Extract<ServerMessage, { type: 'state' }>;

export class OnlineSession implements Session {
  readonly online = true;
  onChange: (() => void) | null = null;
  seat: number | null;
  json: string;
  state: GameState;
  game: Game;
  toAct: number;
  private pending: ((ok: boolean) => void) | null = null;
  private pendingTimer: number | undefined;

  constructor(
    private readonly conn: GameConnection,
    first: StateMessage,
    readonly opponentName: string | null,
  ) {
    this.seat = first.seat;
    this.json = JSON.stringify(first.state);
    this.state = first.state;
    this.toAct = first.to_act;
    this.game = Game.restore(this.json);
  }

  get connection(): ConnectionState {
    return this.conn.status;
  }

  /** Feed every server message here; the game screen owns the socket. */
  receive(msg: ServerMessage): void {
    if (msg.type === 'state') {
      this.seat = msg.seat;
      this.json = JSON.stringify(msg.state);
      this.state = msg.state;
      this.toAct = msg.to_act;
      const old = this.game;
      try {
        this.game = Game.restore(this.json);
        old.free();
      } catch (e) {
        toast(`could not mirror the state: ${e instanceof Error ? e.message : String(e)}`);
      }
      if (this.pending) this.settle(true);
      else this.onChange?.();
    } else if (msg.type === 'error') {
      toast(msg.message);
      if (this.pending) this.settle(false);
    }
  }

  /** The socket went up or down; the screens show it. */
  connectionChanged(): void {
    this.onChange?.();
  }

  private settle(ok: boolean): void {
    const p = this.pending;
    this.pending = null;
    if (this.pendingTimer) window.clearTimeout(this.pendingTimer);
    this.pendingTimer = undefined;
    p?.(ok);
  }

  apply(player: number, action: Record<string, unknown>): Promise<boolean> {
    if (this.seat === null) {
      toast('you are spectating');
      return Promise.resolve(false);
    }
    if (player !== this.seat) {
      toast('that is not your seat');
      return Promise.resolve(false);
    }
    if (this.pending) {
      toast('waiting for the server');
      return Promise.resolve(false);
    }
    if (!this.conn.sendAction(action)) {
      toast('not connected');
      return Promise.resolve(false);
    }
    return new Promise<boolean>((resolve) => {
      this.pending = resolve;
      this.pendingTimer = window.setTimeout(() => {
        if (this.pending === resolve) {
          toast('no answer from the server');
          this.settle(false);
        }
      }, ACTION_TIMEOUT_MS);
    });
  }

  canAct(player: number): boolean {
    return this.seat !== null && player === this.seat;
  }

  playerToAct(): number {
    return this.toAct;
  }

  nextFighter(): number {
    return this.game.next_fighter();
  }

  leave(): void {
    this.settle(false);
    this.conn.close();
  }
}
