import type { Model, Polygon, Unit, Vec2 } from './types';

export const MM_PER_INCH = 25.4;

export function baseRadius(mm: number): number {
  return mm / MM_PER_INCH / 2;
}

export function dist(a: Vec2, b: Vec2): number {
  return Math.hypot(a.x - b.x, a.y - b.y);
}

/** Edge-to-edge distance between two round bases, never negative. */
export function baseDistance(a: Vec2, ra: number, b: Vec2, rb: number): number {
  return Math.max(0, dist(a, b) - ra - rb);
}

/** Even-odd point-in-polygon, same algorithm as the engine. */
export function polygonContains(poly: Polygon, p: Vec2): boolean {
  const pts = poly.points;
  const n = pts.length;
  if (n < 3) return false;
  let inside = false;
  let j = n - 1;
  for (let i = 0; i < n; i++) {
    const a = pts[i];
    const b = pts[j];
    if (a.y > p.y !== b.y > p.y) {
      const x = a.x + ((p.y - a.y) / (b.y - a.y)) * (b.x - a.x);
      if (p.x < x) inside = !inside;
    }
    j = i;
  }
  return inside;
}

export function polygonBounds(poly: Polygon): { x0: number; y0: number; x1: number; y1: number } {
  let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
  for (const p of poly.points) {
    x0 = Math.min(x0, p.x);
    y0 = Math.min(y0, p.y);
    x1 = Math.max(x1, p.x);
    y1 = Math.max(y1, p.y);
  }
  return { x0, y0, x1, y1 };
}

export function aliveModels(u: Unit): Model[] {
  return u.models.filter((m) => m.wounds > 0);
}

export function placedModels(u: Unit): (Model & { pos: Vec2 })[] {
  return aliveModels(u).filter((m): m is Model & { pos: Vec2 } => m.pos !== null);
}

/** Closest edge-to-edge distance between two units, or Infinity. */
export function unitDistance(a: Unit, b: Unit): number {
  let best = Infinity;
  for (const ma of placedModels(a)) {
    for (const mb of placedModels(b)) {
      best = Math.min(best, baseDistance(ma.pos, baseRadius(ma.base_mm), mb.pos, baseRadius(mb.base_mm)));
    }
  }
  return best;
}

export function centroid(points: Vec2[]): Vec2 {
  if (points.length === 0) return { x: 0, y: 0 };
  let x = 0, y = 0;
  for (const p of points) {
    x += p.x;
    y += p.y;
  }
  return { x: x / points.length, y: y / points.length };
}

export type FormationKind = 'blob' | 'line' | 'column';

/**
 * Lay out models around a centre. Bases touch with a small gap so coherency
 * (2" edge to edge, two neighbours at 7+ models) holds for any base size up
 * to 60mm in a blob.
 */
export function formation(radii: number[], centre: Vec2, kind: FormationKind, gap = 0.15): Vec2[] {
  const n = radii.length;
  if (n === 0) return [];
  const maxR = Math.max(...radii);
  const pitch = maxR * 2 + gap;
  const cols = kind === 'line' ? n : kind === 'column' ? 1 : Math.ceil(Math.sqrt(n));
  const rows = Math.ceil(n / cols);
  const out: Vec2[] = [];
  for (let i = 0; i < n; i++) {
    const row = Math.floor(i / cols);
    const col = i % cols;
    const inRow = row === rows - 1 ? n - row * cols : cols;
    const x = centre.x + (col - (inRow - 1) / 2) * pitch + (row % 2 === 1 && kind === 'blob' ? pitch * 0.25 : 0);
    const y = centre.y + (row - (rows - 1) / 2) * pitch;
    out.push({ x, y });
  }
  return out;
}

export function clampToBoard(p: Vec2, r: number, w: number, h: number): Vec2 {
  return { x: Math.min(Math.max(p.x, r), w - r), y: Math.min(Math.max(p.y, r), h - r) };
}

/** Push model `i` out of any overlap with the other placed positions. */
export function resolveOverlap(positions: (Vec2 | null)[], radii: number[], i: number): Vec2 | null {
  let p = positions[i];
  if (!p) return null;
  for (let iter = 0; iter < 8; iter++) {
    let moved = false;
    for (let j = 0; j < positions.length; j++) {
      const q = positions[j];
      if (j === i || !q) continue;
      const min = radii[i] + radii[j] + 0.02;
      const d = dist(p, q);
      if (d < min) {
        const ux: number = d < 1e-6 ? 1 : (p.x - q.x) / d;
        const uy: number = d < 1e-6 ? 0 : (p.y - q.y) / d;
        p = { x: q.x + ux * min, y: q.y + uy * min };
        moved = true;
      }
    }
    if (!moved) break;
  }
  return p;
}

export function fmt(n: number, digits = 1): string {
  return n.toFixed(digits).replace(/\.0$/, '');
}
