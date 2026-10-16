// The placement tool: builds a set of model positions for one unit by
// clicking (formation), translating and dragging single models. The engine
// still validates the result; this only guides toward legal placements.

import { centroid, clampToBoard, dist, formation, resolveOverlap, type FormationKind } from './geom';
import type { Vec2 } from './types';

export type PlacementKind = 'deploy' | 'scout' | 'move' | 'arrive' | 'disembark' | 'charge' | 'pile_in' | 'consolidate';

export interface PlacementModel {
  id: number;
  name: string;
  r: number;
  origin: Vec2 | null;
  isLeader: boolean;
}

export interface PlacementOptions {
  unitId: number;
  owner: number;
  kind: PlacementKind;
  title: string;
  hint: string;
  models: PlacementModel[];
  /** Hard distance limit from each model's origin, if known. */
  limit: number | null;
  /** Dashed outer ring (Advance: M + 6). Models may go this far. */
  outerLimit: number | null;
  startFromOrigins: boolean;
  boardW: number;
  boardH: number;
}

export class Placement {
  readonly unitId: number;
  readonly owner: number;
  readonly kind: PlacementKind;
  readonly title: string;
  readonly hint: string;
  readonly models: PlacementModel[];
  readonly limit: number | null;
  readonly outerLimit: number | null;
  readonly startFromOrigins: boolean;
  positions: (Vec2 | null)[];
  formation: FormationKind = 'blob';
  onChange: (() => void) | null = null;
  private readonly boardW: number;
  private readonly boardH: number;

  constructor(o: PlacementOptions) {
    this.unitId = o.unitId;
    this.owner = o.owner;
    this.kind = o.kind;
    this.title = o.title;
    this.hint = o.hint;
    this.models = o.models;
    this.limit = o.limit;
    this.outerLimit = o.outerLimit;
    this.startFromOrigins = o.startFromOrigins;
    this.boardW = o.boardW;
    this.boardH = o.boardH;
    this.positions = [];
    this.reset();
  }

  get radii(): number[] {
    return this.models.map((m) => m.r);
  }

  reset(): void {
    this.positions = this.models.map((m) => (this.startFromOrigins ? m.origin : null));
    this.changed();
  }

  clearAll(): void {
    this.positions = this.models.map(() => null);
    this.changed();
  }

  complete(): boolean {
    return this.positions.every((p) => p !== null);
  }

  placedCount(): number {
    return this.positions.filter((p) => p !== null).length;
  }

  json(): Vec2[] {
    return this.positions.map((p) => ({ x: round2(p!.x), y: round2(p!.y) }));
  }

  /** The hard radius a model may stray from its origin. */
  private maxFromOrigin(): number | null {
    return this.outerLimit ?? this.limit;
  }

  /**
   * Click on the board: lays a fresh formation when any model is unplaced
   * (or on shift-click); otherwise translates the whole unit so its centre
   * lands on the click.
   */
  clickAt(p: Vec2, shift: boolean): void {
    if (shift || !this.complete()) {
      const pts = formation(this.radii, p, this.formation);
      this.positions = pts;
    } else {
      const c = centroid(this.positions as Vec2[]);
      const dx = p.x - c.x;
      const dy = p.y - c.y;
      this.positions = this.positions.map((q) => ({ x: q!.x + dx, y: q!.y + dy }));
    }
    this.constrainAll();
    this.changed();
  }

  /** Place one model (the first unplaced) at a point. */
  placeNext(p: Vec2): boolean {
    const i = this.positions.findIndex((q) => q === null);
    if (i < 0) return false;
    this.positions[i] = p;
    this.constrain(i);
    this.positions[i] = resolveOverlap(this.positions, this.radii, i);
    this.changed();
    return true;
  }

  drag(i: number, p: Vec2): void {
    this.positions[i] = p;
    this.constrain(i);
    this.changed();
  }

  drop(i: number): void {
    this.positions[i] = resolveOverlap(this.positions, this.radii, i);
    this.constrain(i);
    this.changed();
  }

  private constrainAll(): void {
    for (let i = 0; i < this.positions.length; i++) this.constrain(i);
    for (let i = 0; i < this.positions.length; i++) this.positions[i] = resolveOverlap(this.positions, this.radii, i);
  }

  private constrain(i: number): void {
    let p = this.positions[i];
    if (!p) return;
    const m = this.models[i];
    const max = this.maxFromOrigin();
    if (max !== null && m.origin) {
      const d = dist(m.origin, p);
      if (d > max) {
        const k = (max - 0.01) / d;
        p = { x: m.origin.x + (p.x - m.origin.x) * k, y: m.origin.y + (p.y - m.origin.y) * k };
      }
    }
    this.positions[i] = clampToBoard(p, m.r, this.boardW, this.boardH);
  }

  private changed(): void {
    this.onChange?.();
  }
}

function round2(n: number): number {
  return Math.round(n * 100) / 100;
}
