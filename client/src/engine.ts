import init, { Game, build_mission, build_pack_with_detachments, faction_slug, validate_pack } from './wasm/grimtable_wasm.js';
import type { ArmyPack, Buffs, GameState } from './types';

export { Game };

export async function initWasm(): Promise<void> {
  await init({ module_or_path: new URL('./wasm/grimtable_wasm_bg.wasm', import.meta.url) });
}

export async function fetchText(url: string): Promise<string | null> {
  try {
    const r = await fetch(url);
    if (!r.ok) return null;
    return await r.text();
  } catch {
    return null;
  }
}

export interface LoadedPack {
  pack: ArmyPack;
  json: string;
  slug: string;
  /** Army-construction problems from validate_pack; warnings only. */
  problems: string[];
}

export const POINTS_LIMIT = 2000;

/** Parse a .ros export and build its ArmyPack, fetching the matching abilities. */
export async function loadPack(rosXml: string): Promise<LoadedPack> {
  const slug = faction_slug(rosXml);
  const [abilities, core, detachments] = await Promise.all([
    fetchText(`/data/enrichment/${encodeURIComponent(slug)}/abilities.json`),
    fetchText('/data/enrichment/_core/abilities.json'),
    fetchText(`/data/core/${encodeURIComponent(slug)}/detachments.json`),
  ]);
  const json = build_pack_with_detachments(rosXml, abilities ?? '[]', core ?? '[]', detachments ?? '');
  let problems: string[] = [];
  try {
    problems = JSON.parse(validate_pack(json, POINTS_LIMIT)) as string[];
  } catch {
    problems = [];
  }
  return { pack: JSON.parse(json) as ArmyPack, json, slug, problems };
}

export async function loadMission(patternId: string): Promise<string> {
  const patterns = await fetchText('/data/deployment-patterns.json');
  if (!patterns) throw new Error('could not fetch /data/deployment-patterns.json; run scripts/sync-data.sh');
  return build_mission(patterns, patternId);
}

export function readState(game: Game): { json: string; state: GameState } {
  const json = game.state();
  return { json, state: JSON.parse(json) as GameState };
}

/** Apply an action; returns the rule violation text, or null on success. */
export function tryApply(game: Game, player: number, action: Record<string, unknown>): string | null {
  try {
    game.apply(player, JSON.stringify(action));
    return null;
  } catch (e) {
    return e instanceof Error ? e.message : String(e);
  }
}

export function unitBuffs(game: Game, unitId: number): Buffs {
  return JSON.parse(game.unit_buffs(unitId)) as Buffs;
}

export function visibleEnemies(game: Game, unitId: number): number[] {
  return JSON.parse(game.visible_enemies(unitId)) as number[];
}

export function errorMessage(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}
