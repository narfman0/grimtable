// The saved game is the raw state JSON string from the engine. It is never
// re-serialised from a parsed object: the RNG state is a u64 and JSON.parse
// would round it.

const KEY = 'grimtable.save';
const META = 'grimtable.save.meta';

export interface SaveMeta {
  players: string[];
  phase: string;
  round: number;
  savedAt: number;
}

export function saveGame(stateJson: string, meta: SaveMeta): void {
  try {
    localStorage.setItem(KEY, stateJson);
    localStorage.setItem(META, JSON.stringify(meta));
  } catch {
    // Storage may be unavailable; the game still plays.
  }
}

export function loadGame(): { json: string; meta: SaveMeta } | null {
  try {
    const json = localStorage.getItem(KEY);
    const meta = localStorage.getItem(META);
    if (!json || !meta) return null;
    return { json, meta: JSON.parse(meta) as SaveMeta };
  } catch {
    return null;
  }
}

export function clearSave(): void {
  try {
    localStorage.removeItem(KEY);
    localStorage.removeItem(META);
  } catch {
    // ignore
  }
}
