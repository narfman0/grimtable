// Canvas board at true scale: inches to pixels with pan and zoom.

import { aliveModels, baseRadius, centroid, dist, placedModels } from './geom';
import type { Placement } from './placement';
import type { GameState, Unit, Vec2 } from './types';
import { PLAYER_COLOURS, PLAYER_COLOURS_SOFT } from './ui';

export interface Overlay {
  selected: number | null;
  targets: number[];
  placement: Placement | null;
  /** Draw engagement halos around the selected unit's models. */
  engagement: boolean;
  /** Extra range rings (centre, radius, colour), e.g. charge range. */
  rings: { c: Vec2; r: number; colour: string; dashed?: boolean }[];
}

export interface BoardCallbacks {
  onPlace(p: Vec2, shift: boolean): void;
  onSelectUnit(id: number): void;
  onGhostDrag(i: number, p: Vec2): void;
  onGhostDrop(i: number): void;
  onHover(p: Vec2 | null): void;
}

const OBJECTIVE_RADIUS = 40 / 25.4 / 2;

export class Board {
  private readonly ctx: CanvasRenderingContext2D;
  private state: GameState | null = null;
  overlay: Overlay = { selected: null, targets: [], placement: null, engagement: false, rings: [] };
  private scale = 10;
  private ox = 0;
  private oy = 0;
  private fitted = false;
  private pointer: { id: number; start: Vec2; last: Vec2; ghost: number | null; moved: boolean; unit: number | null } | null = null;
  private raf = 0;

  constructor(private readonly canvas: HTMLCanvasElement, private readonly cb: BoardCallbacks) {
    const ctx = canvas.getContext('2d');
    if (!ctx) throw new Error('no 2d context');
    this.ctx = ctx;
    new ResizeObserver(() => this.resize()).observe(canvas);
    canvas.addEventListener('pointerdown', (e) => this.down(e));
    canvas.addEventListener('pointermove', (e) => this.move(e));
    canvas.addEventListener('pointerup', (e) => this.up(e));
    canvas.addEventListener('pointercancel', (e) => this.up(e));
    canvas.addEventListener('pointerleave', () => this.cb.onHover(null));
    canvas.addEventListener('wheel', (e) => this.wheel(e), { passive: false });
    canvas.addEventListener('contextmenu', (e) => e.preventDefault());
    this.resize();
  }

  setState(s: GameState): void {
    this.state = s;
    if (!this.fitted) this.fit();
    this.requestDraw();
  }

  setOverlay(o: Partial<Overlay>): void {
    this.overlay = { ...this.overlay, ...o };
    this.requestDraw();
  }

  fit(): void {
    if (!this.state) return;
    const w = this.canvas.clientWidth;
    const h = this.canvas.clientHeight;
    if (w === 0 || h === 0) return;
    const { board_w, board_h } = this.state.params;
    this.scale = Math.min(w / board_w, h / board_h) * 0.96;
    this.ox = (w - board_w * this.scale) / 2;
    this.oy = (h - board_h * this.scale) / 2;
    this.fitted = true;
    this.requestDraw();
  }

  requestDraw(): void {
    if (this.raf) return;
    this.raf = requestAnimationFrame(() => {
      this.raf = 0;
      this.draw();
    });
  }

  // ------------------------------------------------------------ transforms

  private toPx(p: Vec2): Vec2 {
    return { x: this.ox + p.x * this.scale, y: this.oy + p.y * this.scale };
  }

  private toIn(px: Vec2): Vec2 {
    return { x: (px.x - this.ox) / this.scale, y: (px.y - this.oy) / this.scale };
  }

  private eventPx(e: PointerEvent | WheelEvent): Vec2 {
    const r = this.canvas.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  }

  private resize(): void {
    const dpr = window.devicePixelRatio || 1;
    const w = this.canvas.clientWidth;
    const h = this.canvas.clientHeight;
    this.canvas.width = Math.max(1, Math.floor(w * dpr));
    this.canvas.height = Math.max(1, Math.floor(h * dpr));
    this.ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    if (!this.fitted) this.fit();
    this.requestDraw();
  }

  // ------------------------------------------------------------ input

  private hitGhost(p: Vec2): number | null {
    const pl = this.overlay.placement;
    if (!pl) return null;
    const minR = 7 / this.scale;
    let best: number | null = null;
    let bestD = Infinity;
    pl.positions.forEach((q, i) => {
      if (!q) return;
      const d = dist(p, q);
      if (d <= Math.max(pl.models[i].r, minR) && d < bestD) {
        best = i;
        bestD = d;
      }
    });
    return best;
  }

  private hitUnit(p: Vec2): number | null {
    if (!this.state) return null;
    const minR = 6 / this.scale;
    let best: number | null = null;
    let bestD = Infinity;
    for (const u of this.state.units) {
      if (!onBoard(u)) continue;
      for (const m of placedModels(u)) {
        const d = dist(p, m.pos);
        if (d <= Math.max(baseRadius(m.base_mm), minR) && d < bestD) {
          best = u.id;
          bestD = d;
        }
      }
    }
    return best;
  }

  private down(e: PointerEvent): void {
    if (e.button !== 0 && e.button !== 1 && e.button !== 2) return;
    const px = this.eventPx(e);
    const p = this.toIn(px);
    const ghost = e.button === 0 ? this.hitGhost(p) : null;
    const unit = e.button === 0 && ghost === null ? this.hitUnit(p) : null;
    this.pointer = { id: e.pointerId, start: px, last: px, ghost, moved: false, unit };
    this.canvas.setPointerCapture(e.pointerId);
    e.preventDefault();
  }

  private move(e: PointerEvent): void {
    const px = this.eventPx(e);
    this.cb.onHover(this.toIn(px));
    const pt = this.pointer;
    if (!pt || pt.id !== e.pointerId) return;
    if (!pt.moved && dist(px, pt.start) > 4) pt.moved = true;
    if (pt.ghost !== null) {
      if (pt.moved) this.cb.onGhostDrag(pt.ghost, this.toIn(px));
    } else if (pt.moved) {
      this.ox += px.x - pt.last.x;
      this.oy += px.y - pt.last.y;
      this.requestDraw();
    }
    pt.last = px;
  }

  private up(e: PointerEvent): void {
    const pt = this.pointer;
    if (!pt || pt.id !== e.pointerId) return;
    this.pointer = null;
    try {
      this.canvas.releasePointerCapture(e.pointerId);
    } catch {
      // already released
    }
    const p = this.toIn(this.eventPx(e));
    if (pt.ghost !== null) {
      if (pt.moved) this.cb.onGhostDrop(pt.ghost);
      return;
    }
    if (pt.moved || e.button !== 0) return;
    if (this.overlay.placement) this.cb.onPlace(p, e.shiftKey);
    else if (pt.unit !== null) this.cb.onSelectUnit(pt.unit);
  }

  private wheel(e: WheelEvent): void {
    e.preventDefault();
    const px = this.eventPx(e);
    const before = this.toIn(px);
    const factor = e.deltaY < 0 ? 1.12 : 1 / 1.12;
    this.scale = Math.min(80, Math.max(3, this.scale * factor));
    const after = this.toPx(before);
    this.ox += px.x - after.x;
    this.oy += px.y - after.y;
    this.requestDraw();
  }

  // ------------------------------------------------------------ drawing

  private draw(): void {
    const ctx = this.ctx;
    const w = this.canvas.clientWidth;
    const h = this.canvas.clientHeight;
    ctx.clearRect(0, 0, w, h);
    ctx.fillStyle = '#0f1115';
    ctx.fillRect(0, 0, w, h);
    const s = this.state;
    if (!s) return;
    const { board_w, board_h } = s.params;
    const o = this.toPx({ x: 0, y: 0 });
    const sc = this.scale;

    // Board surface.
    ctx.fillStyle = '#1b1e25';
    ctx.fillRect(o.x, o.y, board_w * sc, board_h * sc);

    // Deployment zones.
    s.mission.deployment_zones.forEach((z, i) => {
      ctx.fillStyle = PLAYER_COLOURS_SOFT[i];
      this.polygon(z.points);
      ctx.fill();
      ctx.strokeStyle = PLAYER_COLOURS[i];
      ctx.lineWidth = 1;
      ctx.setLineDash([4, 4]);
      this.polygon(z.points);
      ctx.stroke();
      ctx.setLineDash([]);
    });

    // Grid every 6".
    ctx.strokeStyle = 'rgba(255,255,255,0.05)';
    ctx.lineWidth = 1;
    for (let x = 0; x <= board_w; x += 6) {
      ctx.beginPath();
      ctx.moveTo(o.x + x * sc, o.y);
      ctx.lineTo(o.x + x * sc, o.y + board_h * sc);
      ctx.stroke();
    }
    for (let y = 0; y <= board_h; y += 6) {
      ctx.beginPath();
      ctx.moveTo(o.x, o.y + y * sc);
      ctx.lineTo(o.x + board_w * sc, o.y + y * sc);
      ctx.stroke();
    }
    ctx.strokeStyle = '#3a3f4b';
    ctx.strokeRect(o.x, o.y, board_w * sc, board_h * sc);

    // Terrain.
    for (const t of s.mission.terrain) {
      ctx.fillStyle = 'rgba(120,124,135,0.55)';
      this.polygon(t.points);
      ctx.fill();
      ctx.strokeStyle = '#8a8f9c';
      ctx.lineWidth = 1;
      this.polygon(t.points);
      ctx.stroke();
    }

    // Objectives with their control range.
    s.objectives.forEach((ob, i) => {
      const c = this.toPx(ob.pos);
      const colour = ob.controller === null ? '#9aa0ad' : PLAYER_COLOURS[ob.controller];
      ctx.beginPath();
      ctx.arc(c.x, c.y, (OBJECTIVE_RADIUS + s.params.objective_range) * sc, 0, Math.PI * 2);
      ctx.strokeStyle = colour;
      ctx.globalAlpha = 0.35;
      ctx.setLineDash([3, 5]);
      ctx.lineWidth = 1;
      ctx.stroke();
      ctx.setLineDash([]);
      ctx.globalAlpha = 1;
      ctx.beginPath();
      ctx.arc(c.x, c.y, OBJECTIVE_RADIUS * sc, 0, Math.PI * 2);
      ctx.fillStyle = colour;
      ctx.globalAlpha = 0.5;
      ctx.fill();
      ctx.globalAlpha = 1;
      ctx.lineWidth = 2;
      ctx.stroke();
      ctx.fillStyle = '#fff';
      ctx.font = `${Math.max(9, sc * 0.9)}px system-ui`;
      ctx.textAlign = 'center';
      ctx.textBaseline = 'middle';
      const home = s.mission.home_objectives.indexOf(i);
      ctx.fillText(home >= 0 ? `H${home + 1}` : String(i + 1), c.x, c.y);
    });

    // Extra rings.
    for (const r of this.overlay.rings) {
      const c = this.toPx(r.c);
      ctx.beginPath();
      ctx.arc(c.x, c.y, r.r * sc, 0, Math.PI * 2);
      ctx.strokeStyle = r.colour;
      ctx.lineWidth = 1;
      if (r.dashed) ctx.setLineDash([4, 4]);
      ctx.stroke();
      ctx.setLineDash([]);
    }

    // Placement origins and limits.
    const pl = this.overlay.placement;
    if (pl) {
      pl.models.forEach((m, i) => {
        if (!m.origin) return;
        const c = this.toPx(m.origin);
        if (pl.limit !== null) {
          ctx.beginPath();
          ctx.arc(c.x, c.y, (pl.limit + m.r) * sc, 0, Math.PI * 2);
          ctx.strokeStyle = 'rgba(255,255,255,0.22)';
          ctx.lineWidth = 1;
          ctx.stroke();
        }
        if (pl.outerLimit !== null) {
          ctx.beginPath();
          ctx.arc(c.x, c.y, (pl.outerLimit + m.r) * sc, 0, Math.PI * 2);
          ctx.strokeStyle = 'rgba(255,255,255,0.18)';
          ctx.setLineDash([3, 4]);
          ctx.stroke();
          ctx.setLineDash([]);
        }
        const q = pl.positions[i];
        if (q) {
          const d = this.toPx(q);
          ctx.beginPath();
          ctx.moveTo(c.x, c.y);
          ctx.lineTo(d.x, d.y);
          ctx.strokeStyle = 'rgba(255,255,255,0.3)';
          ctx.lineWidth = 1;
          ctx.stroke();
        }
      });
    }

    // Models.
    for (const u of s.units) {
      if (!onBoard(u)) continue;
      const selected = this.overlay.selected === u.id;
      const target = this.overlay.targets.includes(u.id);
      const moving = pl !== null && pl.unitId === u.id;
      const colour = PLAYER_COLOURS[u.owner];
      for (const m of placedModels(u)) {
        const c = this.toPx(m.pos);
        const r = baseRadius(m.base_mm) * sc;
        if (selected && this.overlay.engagement) {
          ctx.beginPath();
          ctx.arc(c.x, c.y, r + s.params.engagement_range * sc, 0, Math.PI * 2);
          ctx.fillStyle = 'rgba(255,255,255,0.06)';
          ctx.fill();
        }
        ctx.beginPath();
        ctx.arc(c.x, c.y, r, 0, Math.PI * 2);
        ctx.fillStyle = colour;
        ctx.globalAlpha = moving ? 0.3 : u.battle_shocked ? 0.7 : 1;
        ctx.fill();
        ctx.globalAlpha = 1;
        ctx.lineWidth = selected ? 2.5 : 1;
        ctx.strokeStyle = selected ? '#ffffff' : target ? '#ff4d4d' : 'rgba(0,0,0,0.6)';
        if (target) ctx.lineWidth = 2.5;
        ctx.stroke();
        if (m.is_leader) {
          ctx.beginPath();
          ctx.arc(c.x, c.y, Math.max(1.5, r * 0.3), 0, Math.PI * 2);
          ctx.fillStyle = '#fff';
          ctx.fill();
        }
        if (m.wounds < m.max_wounds) {
          ctx.beginPath();
          ctx.arc(c.x, c.y, r * 0.75, -Math.PI / 2, -Math.PI / 2 + (Math.PI * 2 * (m.max_wounds - m.wounds)) / m.max_wounds);
          ctx.strokeStyle = '#ff3b3b';
          ctx.lineWidth = Math.max(1.5, r * 0.25);
          ctx.stroke();
        }
        if (u.battle_shocked) {
          ctx.fillStyle = '#ffcf5c';
          ctx.font = `bold ${Math.max(8, r * 1.4)}px system-ui`;
          ctx.textAlign = 'center';
          ctx.textBaseline = 'middle';
          ctx.fillText('!', c.x, c.y);
        }
      }
      // Unit label at the centroid.
      if (sc >= 7) {
        const pts = placedModels(u).map((m) => m.pos);
        if (pts.length) {
          const c = this.toPx(centroid(pts));
          const maxR = Math.max(...aliveModels(u).map((m) => baseRadius(m.base_mm))) * sc;
          const rows = Math.ceil(Math.sqrt(pts.length));
          const label = u.spec.name.length > 18 ? u.spec.name.slice(0, 17) + '…' : u.spec.name;
          ctx.font = `${selected ? 'bold ' : ''}10px system-ui`;
          ctx.textAlign = 'center';
          ctx.textBaseline = 'top';
          const tw = ctx.measureText(label).width;
          const ly = c.y + maxR * rows + 3;
          ctx.fillStyle = 'rgba(0,0,0,0.55)';
          ctx.fillRect(c.x - tw / 2 - 3, ly - 1, tw + 6, 12);
          ctx.fillStyle = selected ? '#fff' : target ? '#ff8a8a' : '#d8dbe3';
          ctx.fillText(label, c.x, ly);
        }
      }
    }

    // Ghosts (placement in progress).
    if (pl) {
      const colour = PLAYER_COLOURS[pl.owner];
      pl.positions.forEach((q, i) => {
        if (!q) return;
        const m = pl.models[i];
        const c = this.toPx(q);
        const r = m.r * sc;
        ctx.beginPath();
        ctx.arc(c.x, c.y, r, 0, Math.PI * 2);
        ctx.fillStyle = colour;
        ctx.globalAlpha = 0.75;
        ctx.fill();
        ctx.globalAlpha = 1;
        ctx.strokeStyle = '#fff';
        ctx.setLineDash([3, 2]);
        ctx.lineWidth = 1.5;
        ctx.stroke();
        ctx.setLineDash([]);
        ctx.fillStyle = '#fff';
        ctx.font = `bold ${Math.max(8, Math.min(r * 1.1, 14))}px system-ui`;
        ctx.textAlign = 'center';
        ctx.textBaseline = 'middle';
        ctx.fillText(String(i + 1), c.x, c.y);
      });
    }
  }

  private polygon(points: Vec2[]): void {
    const ctx = this.ctx;
    ctx.beginPath();
    points.forEach((p, i) => {
      const q = this.toPx(p);
      if (i === 0) ctx.moveTo(q.x, q.y);
      else ctx.lineTo(q.x, q.y);
    });
    ctx.closePath();
  }
}

export function onBoard(u: Unit): boolean {
  return u.merged_into === null && u.deployed && !u.in_reserve && u.embarked_in === null && aliveModels(u).length > 0;
}
