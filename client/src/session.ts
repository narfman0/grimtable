import { Game, readState, tryApply } from './engine';
import { saveGame } from './storage';
import type { GameState } from './types';
import { toast } from './ui';

/** A running game plus its last parsed state. Persists after every action. */
export class Session {
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

  /** Apply an action; shows the rule error as a toast and returns false on failure. */
  apply(player: number, action: Record<string, unknown>): boolean {
    const err = tryApply(this.game, player, action);
    if (err) {
      toast(err);
      return false;
    }
    this.refresh();
    return true;
  }

  playerToAct(): number {
    return this.game.player_to_act();
  }

  nextFighter(): number {
    return this.game.next_fighter();
  }
}
