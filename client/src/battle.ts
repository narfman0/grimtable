// The battle screen: canvas board on the left, phase-driven panel on the
// right. Every action the engine supports is reachable from here.

import { Board, onBoard } from './board';
import { unitBuffs, visibleEnemies } from './engine';
import { aliveModels, baseRadius, dist, fmt, placedModels, unitDistance } from './geom';
import type { App } from './main';
import { Placement, type PlacementKind } from './placement';
import type { Session } from './session';
import { clearSave } from './storage';
import type { Buffs, DiceExpr, EnumValue, MoveKind, Phase, Unit, Vec2 } from './types';
import { PLAYER_COLOURS, button, h, select, toast } from './ui';

interface ShootChoice {
  weapon: number | null;
  target: number | null;
}

interface UiState {
  selected: number | null;
  listOwner: number | null;
  placement: Placement | null;
  placeMode: 'unit' | 'model';
  moveKind: MoveKind;
  shootUnit: number | null;
  shoot: Map<string, ShootChoice>;
  chargeTargets: Set<number>;
  fightUnit: number | null;
  fight: { target: number | null; pileIn: Vec2[] | null; consolidate: Vec2[] | null };
  showDice: boolean;
  handoff: boolean;
  lastActor: number | null;
}

const PHASE_LABEL: Record<Phase, string> = {
  Muster: 'Muster',
  Deployment: 'Deployment',
  PreBattle: 'Pre-battle',
  Command: 'Command phase',
  Movement: 'Movement phase',
  Shooting: 'Shooting phase',
  Charge: 'Charge phase',
  Fight: 'Fight phase',
  Ended: 'Battle over',
};

export function renderBattle(app: App, session: Session): HTMLElement {
  const ui: UiState = {
    selected: null,
    listOwner: null,
    placement: null,
    placeMode: 'unit',
    moveKind: 'Normal',
    shootUnit: null,
    shoot: new Map(),
    chargeTargets: new Set(),
    fightUnit: null,
    fight: { target: null, pileIn: null, consolidate: null },
    showDice: false,
    handoff: false,
    lastActor: null,
  };

  const canvas = h('canvas');
  const boardStatus = h('div', { class: 'boardstatus' });
  const boardHelp = h('div', { class: 'boardhelp' });
  const boardWrap = h('div', { class: 'boardwrap' }, canvas, boardHelp, boardStatus);
  const header = h('div', { class: 'header' });
  const scroll = h('div', { class: 'scroll' });
  const logWrap = h('div', { class: 'logwrap' });
  const panel = h('div', { class: 'panel' }, header, scroll, logWrap);
  const root = h('div', { class: 'battle' }, boardWrap, panel);

  const board = new Board(canvas, {
    onPlace(p, shift) {
      const pl = ui.placement;
      if (!pl) return;
      if (ui.placeMode === 'model' && !shift && !pl.complete()) pl.placeNext(p);
      else pl.clickAt(p, shift);
      renderPlacementStatus();
    },
    onSelectUnit(id) {
      selectUnit(id);
      render();
    },
    onGhostDrag(i, p) {
      ui.placement?.drag(i, p);
    },
    onGhostDrop(i) {
      ui.placement?.drop(i);
      renderPlacementStatus();
    },
    onHover(p) {
      if (!p) {
        boardStatus.textContent = '';
        return;
      }
      let text = `${fmt(p.x)}", ${fmt(p.y)}"`;
      const u = selectedUnit();
      if (u) {
        const models = placedModels(u);
        if (models.length) {
          const d = Math.min(...models.map((m) => dist(m.pos, p) - baseRadius(m.base_mm)));
          text += ` · ${fmt(Math.max(0, d))}" from ${u.spec.name}`;
        }
      }
      boardStatus.textContent = text;
    },
  });

  document.addEventListener('keydown', onKey);
  function onKey(e: KeyboardEvent): void {
    if (!root.isConnected) {
      document.removeEventListener('keydown', onKey);
      return;
    }
    if (e.key === 'Escape' && ui.placement && ui.placement.kind !== 'charge') {
      ui.placement = null;
      render();
    }
  }

  // ------------------------------------------------------------ helpers

  const s = () => session.state;
  const game = session.game;

  function unit(id: number | null): Unit | null {
    return id === null ? null : (s().units[id] ?? null);
  }

  function selectedUnit(): Unit | null {
    return unit(ui.selected);
  }

  function selectUnit(id: number | null): void {
    if (ui.selected === id) return;
    ui.selected = id;
    ui.chargeTargets.clear();
    ui.fight = { target: null, pileIn: null, consolidate: null };
    ui.fightUnit = null;
    ui.shoot.clear();
    ui.shootUnit = null;
    if (ui.placement && ui.placement.kind !== 'charge' && ui.placement.unitId !== id) ui.placement = null;
  }

  function act(player: number, action: Record<string, unknown>): boolean {
    const ok = session.apply(player, action);
    if (ok) {
      ui.placement = null;
      ui.chargeTargets.clear();
      ui.fight = { target: null, pileIn: null, consolidate: null };
      ui.shoot.clear();
      ui.shootUnit = null;
      ui.fightUnit = null;
    }
    render();
    return ok;
  }

  function enemiesOf(player: number): Unit[] {
    return s().units.filter((u) => u.owner !== player && onBoard(u));
  }

  function engagedWith(u: Unit): Unit[] {
    const er = s().params.engagement_range;
    return enemiesOf(u.owner).filter((e) => unitDistance(u, e) <= er);
  }

  function hasRule(u: Unit, name: string): boolean {
    return u.spec.core_rules.includes(name);
  }

  function ruleParam(u: Unit, name: string): number | null {
    for (const r of u.spec.core_rules) {
      if (typeof r === 'object' && name in r) return Number((r as Record<string, unknown>)[name]);
    }
    return null;
  }

  function moveLimit(u: Unit): number {
    const b = unitBuffs(game, u.id);
    const statM = b.stat_add.filter(([k]) => k === 'M').reduce((n, [, v]) => n + v, 0);
    return Math.max(0, u.spec.stats.movement + b.move_add + statM);
  }

  function makePlacement(u: Unit, kind: PlacementKind, title: string, hint: string, limit: number | null, outerLimit: number | null, fromOrigins: boolean): void {
    const p = s().params;
    ui.placement = new Placement({
      unitId: u.id,
      owner: u.owner,
      kind,
      title,
      hint,
      models: aliveModels(u).map((m) => ({ id: m.id, name: m.name, r: baseRadius(m.base_mm), origin: m.pos, isLeader: m.is_leader })),
      limit,
      outerLimit,
      startFromOrigins: fromOrigins,
      boardW: p.board_w,
      boardH: p.board_h,
    });
    ui.placement.onChange = () => board.requestDraw();
    render();
  }

  function openChargePlacement(): void {
    const pending = s().pending;
    if (!pending) return;
    const u = unit(pending.ChargeMove.unit);
    if (!u) return;
    ui.selected = u.id;
    makePlacement(
      u,
      'charge',
      `Charge move: rolled ${pending.ChargeMove.roll}"`,
      `Each model may move up to ${pending.ChargeMove.roll}". End within 1" of every target and not within 1" of any other enemy unit.`,
      pending.ChargeMove.roll,
      null,
      true,
    );
  }

  function confirmPlacement(): void {
    const pl = ui.placement;
    if (!pl) return;
    if (!pl.complete()) {
      toast(`${pl.placedCount()} of ${pl.models.length} models placed`);
      return;
    }
    const positions = pl.json();
    const player = s().active;
    switch (pl.kind) {
      case 'deploy':
        act(player, { type: 'deploy', unit: pl.unitId, positions });
        break;
      case 'scout':
        act(player, { type: 'scout_move', unit: pl.unitId, positions });
        break;
      case 'move':
        act(player, { type: 'move', unit: pl.unitId, kind: ui.moveKind, positions });
        break;
      case 'arrive':
        act(player, { type: 'arrive_from_reserve', unit: pl.unitId, positions });
        break;
      case 'disembark':
        act(player, { type: 'disembark', unit: pl.unitId, positions });
        break;
      case 'charge':
        act(player, { type: 'charge_move', unit: pl.unitId, positions });
        break;
      case 'pile_in':
        ui.fight.pileIn = positions;
        ui.placement = null;
        render();
        break;
      case 'consolidate':
        ui.fight.consolidate = positions;
        ui.placement = null;
        render();
        break;
    }
  }

  // ------------------------------------------------------------ render

  function render(): void {
    const st = s();
    const actor = session.playerToAct();
    if (st.pending && !ui.placement) openChargePlacement();
    if (!ui.handoff) ui.lastActor = actor;

    board.setState(st);
    board.setOverlay({
      selected: ui.selected,
      targets: overlayTargets(),
      placement: ui.placement,
      engagement: st.phase === 'Charge' || st.phase === 'Fight' || st.phase === 'Movement',
    });
    boardHelp.textContent = ui.placement
      ? ui.placeMode === 'model' && !ui.placement.complete()
        ? 'Click to place the next model. Drag a placed model to adjust. Shift-click re-forms the unit.'
        : 'Click to place or move the whole unit. Drag single models to adjust. Shift-click re-forms the unit. Wheel zooms, drag empty space pans.'
      : 'Click a model to select its unit. Wheel zooms, drag pans.';

    renderHeader(actor);
    renderPanelBody(actor);
    renderLog();

    const old = root.querySelector('.overlay');
    old?.remove();
    if (ui.handoff && ui.lastActor !== null && ui.lastActor !== actor && st.phase !== 'Ended') {
      root.appendChild(
        h(
          'div',
          { class: 'overlay' },
          h(
            'div',
            { class: 'box' },
            h('div', { class: 'muted' }, 'Pass the device to'),
            h('div', { class: `who p${actor}` }, st.players[actor].name),
            button(
              'Continue',
              () => {
                ui.lastActor = actor;
                render();
              },
              { class: 'primary' },
            ),
          ),
        ),
      );
    }
  }

  function overlayTargets(): number[] {
    const st = s();
    if (st.pending) return unit(st.pending.ChargeMove.unit)?.charge_targets ?? [];
    switch (st.phase) {
      case 'Shooting':
        return [...new Set([...ui.shoot.values()].map((c) => c.target).filter((t): t is number => t !== null))];
      case 'Charge':
        return [...ui.chargeTargets];
      case 'Fight':
        return ui.fight.target === null ? [] : [ui.fight.target];
      default:
        return [];
    }
  }

  function renderHeader(actor: number): void {
    const st = s();
    const p = st.players;
    header.replaceChildren(
      h(
        'div',
        null,
      h(
        'div',
        { class: 'row', style: 'justify-content:space-between' },
        h('div', { class: 'phase' }, st.round > 0 ? `Round ${st.round} of ${st.params.battle_rounds} · ` : '', PHASE_LABEL[st.phase]),
        h(
          'div',
          { class: 'row' },
          button('Fit', () => board.fit(), { class: 'small', title: 'Fit the board to the window' }),
          button(
            'New game',
            () => {
              if (confirm('Leave this battle? The saved game will be discarded.')) {
                clearSave();
                app.showSetup();
              }
            },
            { class: 'small' },
          ),
        ),
      ),
      h('div', { class: `toact p${actor}` }, st.phase === 'Ended' ? 'Final result' : `${p[actor].name} to act`),
      st.phase === 'Fight' && actor !== st.active ? h('div', { class: 'muted small' }, `${p[st.active].name}'s turn; ${p[actor].name} picks the next unit to fight`) : null,
      h(
        'div',
        { class: 'scores' },
        p.map((pl, i) =>
          h(
            'div',
            { style: `border-left:3px solid ${PLAYER_COLOURS[i]}` },
            h('b', null, pl.name),
            h('div', { class: 'muted' }, `CP ${pl.cp} · VP ${pl.vp}${st.first_player === i ? ' · first turn' : ''}`),
          ),
        ),
      ),
      h(
        'div',
        { class: 'row small', style: 'margin-top:6px' },
        h(
          'label',
          null,
          h('input', {
            type: 'checkbox',
            checked: ui.handoff,
            onChange: (e: Event) => {
              ui.handoff = (e.target as HTMLInputElement).checked;
              ui.lastActor = actor;
            },
          }),
          ' pass-device screen',
        ),
        h(
          'label',
          null,
          h('input', {
            type: 'checkbox',
            checked: ui.showDice,
            onChange: (e: Event) => {
              ui.showDice = (e.target as HTMLInputElement).checked;
              renderLog();
            },
          }),
          ' show dice lines',
        ),
      ),
      ),
    );
  }

  function renderPanelBody(actor: number): void {
    const st = s();
    const parts: (HTMLElement | null)[] = [];
    if (st.phase === 'Ended') parts.push(renderEnded());
    if (ui.placement) parts.push(renderPlacement());
    else {
      parts.push(renderPhaseActions(actor));
      parts.push(renderEndPhase(actor));
    }
    parts.push(renderArmyBox(actor));
    parts.push(renderUnitList(actor));
    parts.push(renderUnitPanel());
    scroll.replaceChildren(...parts.filter((p): p is HTMLElement => p !== null));
  }

  function renderEnded(): HTMLElement {
    const st = s();
    const [a, b] = st.players;
    const w = st.winner;
    return h(
      'div',
      { class: 'winner' },
      h('div', { class: 'big' }, w === null ? 'A draw' : h('span', { class: `p${w}` }, `${st.players[w].name} wins`)),
      h('div', null, `${a.name} ${a.vp} VP · ${b.name} ${b.vp} VP`),
    );
  }

  // ------------------------------------------------------------ placement UI

  const placementStatus = h('div', { class: 'modellist' });

  function renderPlacementStatus(): void {
    const pl = ui.placement;
    if (!pl) return;
    placementStatus.replaceChildren(
      ...pl.models.map((m, i) => h('div', { class: pl.positions[i] ? 'placed' : '' }, `${i + 1}. ${m.name}${m.isLeader ? ' (leader)' : ''}${pl.positions[i] ? '' : ' — unplaced'}`)),
    );
    const btn = scroll.querySelector<HTMLButtonElement>('button.confirm');
    if (btn) btn.disabled = !pl.complete();
  }

  function renderPlacement(): HTMLElement {
    const pl = ui.placement!;
    const u = unit(pl.unitId)!;
    renderPlacementStatus();
    return h(
      'div',
      { class: 'actions' },
      h('div', { class: 'title' }, pl.title, ' — ', u.spec.name),
      h('div', { class: 'hint' }, pl.hint),
      h(
        'div',
        { class: 'row' },
        h('label', null, 'Formation '),
        select(
          [
            { value: 'blob', label: 'blob' },
            { value: 'line', label: 'line' },
            { value: 'column', label: 'column' },
          ],
          pl.formation,
          (v) => {
            pl.formation = v as typeof pl.formation;
          },
        ),
        h('label', null, 'Click places '),
        select(
          [
            { value: 'unit', label: 'whole unit' },
            { value: 'model', label: 'one model at a time' },
          ],
          ui.placeMode,
          (v) => {
            ui.placeMode = v as 'unit' | 'model';
            render();
          },
        ),
      ),
      placementStatus,
      h(
        'div',
        { class: 'row' },
        button('Confirm', confirmPlacement, { class: 'primary confirm', disabled: !pl.complete() }),
        button('Reset', () => {
          pl.reset();
          renderPlacementStatus();
        }),
        button('Clear', () => {
          pl.clearAll();
          renderPlacementStatus();
        }),
        pl.kind === 'charge'
          ? null
          : button('Cancel', () => {
              ui.placement = null;
              render();
            }),
      ),
    );
  }

  // ------------------------------------------------------------ phase actions

  function renderPhaseActions(actor: number): HTMLElement | null {
    const st = s();
    const u = selectedUnit();
    const mine = u !== null && u.owner === actor;
    switch (st.phase) {
      case 'Deployment':
        return renderDeployment(actor, u, mine);
      case 'PreBattle':
        return renderPreBattle(u, mine);
      case 'Command':
        return h(
          'div',
          { class: 'actions' },
          h('div', { class: 'title' }, 'Command phase'),
          h('div', { class: 'hint' }, 'Both players gained CP and battle-shock tests were resolved when the turn began (see the log). Choose army-rule options below, then end the phase.'),
        );
      case 'Movement':
        return renderMovement(u, mine);
      case 'Shooting':
        return renderShooting(u, mine);
      case 'Charge':
        return renderCharge(u, mine);
      case 'Fight':
        return renderFight(actor, u, mine);
      default:
        return null;
    }
  }

  function renderDeployment(actor: number, u: Unit | null, mine: boolean): HTMLElement {
    const st = s();
    const todo = st.units.filter((x) => x.owner === actor && !x.deployed && x.merged_into === null);
    const rows: (HTMLElement | null)[] = [
      h('div', { class: 'title' }, `${st.players[actor].name} deploys`),
      h('div', { class: 'hint' }, `${todo.length} unit${todo.length === 1 ? '' : 's'} left to set up. Pick one from the list, then place it wholly inside your zone, hold it in reserve, or embark it.`),
    ];
    if (u && mine && !u.deployed && u.merged_into === null) {
      const infiltrates = hasRule(u, 'Infiltrators');
      const transports = st.units.filter((t) => t.owner === actor && t.spec.transport_capacity !== null && t.id !== u.id && t.merged_into === null);
      const aboard = (t: Unit) => st.units.filter((x) => x.embarked_in === t.id).reduce((n, x) => n + aliveModels(x).length, 0);
      const fits = transports.filter((t) => aboard(t) + aliveModels(u).length <= (t.spec.transport_capacity ?? 0));
      const canEmbark = u.spec.keywords.some((k) => k.toLowerCase() === 'infantry') && fits.length > 0;
      let transportChoice = fits[0]?.id ?? null;
      rows.push(
        h(
          'div',
          { class: 'row' },
          button(
            'Place on board',
            () =>
              makePlacement(
                u,
                'deploy',
                'Deploy',
                infiltrates
                  ? 'Infiltrators: set up anywhere more than 9" from enemy models and outside the enemy deployment zone.'
                  : 'Every model must be wholly inside your deployment zone (tinted in your colour).',
                null,
                null,
                false,
              ),
            { class: 'primary' },
          ),
          button('Hold in reserve', () => act(actor, { type: 'reserve', unit: u.id })),
        ),
        canEmbark
          ? h(
              'div',
              { class: 'row' },
              h('label', null, 'Embark in '),
              select(
                fits.map((t) => ({ value: String(t.id), label: `${t.spec.name} (${aboard(t)}/${t.spec.transport_capacity})` })),
                String(transportChoice),
                (v) => (transportChoice = Number(v)),
              ),
              button('Embark', () => act(actor, { type: 'embark', unit: u.id, transport: transportChoice })),
            )
          : null,
      );
    } else if (u && mine) {
      rows.push(h('div', { class: 'hint' }, `${u.spec.name} is already set up.`));
    }
    return h('div', { class: 'actions' }, rows);
  }

  function renderPreBattle(u: Unit | null, mine: boolean): HTMLElement {
    const st = s();
    const rows: (HTMLElement | null)[] = [
      h('div', { class: 'title' }, 'Pre-battle: scout moves'),
      h('div', { class: 'hint' }, `${st.players[st.active].name} may make scout moves with units that have Scouts, then ends the phase. The other player follows.`),
    ];
    if (u && mine) {
      const d = ruleParam(u, 'Scouts');
      if (d === null) rows.push(h('div', { class: 'hint' }, `${u.spec.name} has no Scouts rule.`));
      else if (u.flags.moved) rows.push(h('div', { class: 'hint' }, `${u.spec.name} has made its scout move.`));
      else if (onBoard(u))
        rows.push(
          button(`Scout move (${d}")`, () => makePlacement(u, 'scout', `Scout move ${d}"`, `Each model may move up to ${d}". End more than 9" from enemy models.`, d, null, true), {
            class: 'primary',
          }),
        );
    }
    return h('div', { class: 'actions' }, rows);
  }

  function renderMovement(u: Unit | null, mine: boolean): HTMLElement {
    const st = s();
    const rows: (HTMLElement | null)[] = [h('div', { class: 'title' }, 'Movement phase')];
    if (!u || !mine) {
      rows.push(h('div', { class: 'hint' }, 'Select one of your units to move it, bring it on from reserve, or disembark it.'));
      return h('div', { class: 'actions' }, rows);
    }
    if (u.embarked_in !== null) {
      const t = unit(u.embarked_in);
      rows.push(
        h('div', { class: 'hint' }, `${u.spec.name} is embarked in ${t?.spec.name ?? 'a transport'}. Disembark within 3" of it (not after it advanced or fell back).`),
        button('Disembark', () => makePlacement(u, 'disembark', 'Disembark', `Set up every model within 3" of ${t?.spec.name ?? 'the transport'} and outside engagement range of enemies.`, null, null, false), {
          class: 'primary',
        }),
      );
      return h('div', { class: 'actions' }, rows);
    }
    if (u.in_reserve) {
      const b = unitBuffs(game, u.id);
      if (st.round < st.params.reserves_from_round) rows.push(h('div', { class: 'hint' }, `Reserves can arrive from battle round ${st.params.reserves_from_round}.`));
      else
        rows.push(
          h('div', { class: 'hint' }, b.deep_strike ? `Deep Strike: set up anywhere more than ${st.params.reserve_distance}" from enemy models.` : `Strategic reserves: set up wholly within 6" of a battlefield edge and more than ${st.params.reserve_distance}" from enemy models.`),
          button('Arrive from reserve', () => makePlacement(u, 'arrive', 'Arrive from reserve', b.deep_strike ? 'Deep Strike: more than 8" from enemy models.' : 'Within 6" of a board edge, more than 8" from enemy models.', null, null, false), {
            class: 'primary',
          }),
        );
      return h('div', { class: 'actions' }, rows);
    }
    if (!onBoard(u)) return h('div', { class: 'actions' }, rows, h('div', { class: 'hint' }, 'That unit is not on the battlefield.'));
    if (u.flags.moved) {
      rows.push(h('div', { class: 'hint' }, `${u.spec.name} already moved this phase (${u.flags.moved}).`));
      return h('div', { class: 'actions' }, rows);
    }
    const m = moveLimit(u);
    const engaged = engagedWith(u).length > 0;
    rows.push(
      h('div', { class: 'hint' }, `M ${m}"${engaged ? ' · in engagement range: it can only fall back or stay' : ''}${u.disembarked_after_move ? ' · disembarked after its transport moved' : ''}`),
      h(
        'div',
        { class: 'row' },
        button(
          'Normal move',
          () => {
            ui.moveKind = 'Normal';
            makePlacement(u, 'move', `Normal move ${m}"`, `Each model may move up to ${m}". Do not end within 1" of enemy models.`, m, null, true);
          },
          { class: 'primary', disabled: engaged },
        ),
        button(
          'Advance',
          () => {
            ui.moveKind = 'Advance';
            makePlacement(u, 'move', `Advance ${m}" + D6`, `The engine rolls the D6 when you confirm. Solid ring is ${m}", dashed ring ${m + 6}". If a model is too far the move is refused and the roll shown; the same roll applies when you retry. No shooting (except Assault) or charging afterwards.`, m, m + 6, true);
          },
          { disabled: engaged },
        ),
        button(
          'Fall back',
          () => {
            ui.moveKind = 'FallBack';
            makePlacement(u, 'move', `Fall back ${m}"`, `Each model may move up to ${m}". End outside engagement range of every enemy. No shooting or charging afterwards.`, m, null, true);
          },
          { disabled: !engaged },
        ),
        button('Remain stationary', () => act(st.active, { type: 'move', unit: u.id, kind: 'Stationary', positions: [] })),
      ),
    );
    return h('div', { class: 'actions' }, rows);
  }

  function renderShooting(u: Unit | null, mine: boolean): HTMLElement {
    const st = s();
    const rows: (HTMLElement | null)[] = [h('div', { class: 'title' }, 'Shooting phase')];
    if (!u || !mine) {
      rows.push(h('div', { class: 'hint' }, 'Select one of your units, pick a target for each ranged weapon, then shoot.'));
      return h('div', { class: 'actions' }, rows);
    }
    if (!onBoard(u)) return h('div', { class: 'actions' }, rows, h('div', { class: 'hint' }, 'That unit is not on the battlefield.'));
    if (u.flags.shot) return h('div', { class: 'actions' }, rows, h('div', { class: 'hint' }, `${u.spec.name} has already shot.`));
    if (u.flags.moved === 'FallBack') return h('div', { class: 'actions' }, rows, h('div', { class: 'hint' }, `${u.spec.name} fell back and cannot shoot.`));
    if (ui.shootUnit !== u.id) {
      ui.shoot.clear();
      ui.shootUnit = u.id;
    }
    const alive = aliveModels(u);
    const carried = new Map<number, number>();
    for (const m of alive) for (const w of m.weapons) carried.set(w, (carried.get(w) ?? 0) + 1);
    const ranged = [...carried.keys()].filter((w) => u.spec.weapons[w].kind === 'Ranged').sort((a, b) => a - b);
    if (ranged.length === 0) return h('div', { class: 'actions' }, rows, h('div', { class: 'hint' }, `${u.spec.name} has no ranged weapons.`));
    const visible = visibleEnemies(game, u.id).map((id) => st.units[id]);
    const engaged = engagedWith(u);
    const targetOptions = [{ value: '', label: '— no target —' }, ...visible.map((e) => ({ value: String(e.id), label: `${e.spec.name} (${fmt(unitDistance(u, e))}")` }))];
    const hints: string[] = [];
    if (u.flags.moved === 'Advance') hints.push('advanced: only Assault weapons may fire');
    if (engaged.length) hints.push('in engagement range: only Pistols, at the units it is engaged with');
    if (visible.length === 0) hints.push('no enemy unit is visible to this unit');
    rows.push(h('div', { class: 'hint' }, `${visible.length} visible enemy unit${visible.length === 1 ? '' : 's'}. ${hints.join('; ')}`));

    // Group alternative profiles.
    const groups = new Map<string, number[]>();
    for (const w of ranged) {
      const key = u.spec.weapons[w].profile_group ?? `w${w}`;
      groups.set(key, [...(groups.get(key) ?? []), w]);
    }
    const allTarget = select(targetOptions, '', (v) => {
      for (const [key, ws] of groups) ui.shoot.set(key, { weapon: ws[0], target: v === '' ? null : Number(v) });
      render();
    });
    rows.push(h('div', { class: 'row' }, h('label', null, 'Everything at '), allTarget));

    for (const [key, ws] of groups) {
      const choice = ui.shoot.get(key) ?? { weapon: ws[0], target: null };
      ui.shoot.set(key, choice);
      const nameEl = h('div', { class: 'wname' });
      if (ws.length === 1) {
        const w = u.spec.weapons[ws[0]];
          nameEl.append(h('span', null, `${w.profile.name} ×${carried.get(ws[0])}`), h('span', { class: 'wstat' }, weaponStat(ws[0], u)));
      } else {
        nameEl.append(h('span', { class: 'muted small' }, `${u.spec.weapons[ws[0]].profile_group} ×${carried.get(ws[0])} — one profile:`));
        for (const w of ws) {
          const radio = h('input', {
            type: 'radio',
            name: `grp-${u.id}-${key}`,
            checked: choice.weapon === w,
            onChange: () => {
              choice.weapon = w;
              render();
            },
          });
          nameEl.append(h('label', { style: 'color:var(--fg)' }, radio, ' ', u.spec.weapons[w].profile.name, ' ', h('span', { class: 'wstat' }, weaponStat(w, u))));
        }
      }
      rows.push(
        h(
          'div',
          { class: 'weapon' },
          nameEl,
          select(targetOptions, choice.target === null ? '' : String(choice.target), (v) => {
            choice.target = v === '' ? null : Number(v);
            render();
          }),
        ),
      );
    }
    const assignments = [...ui.shoot.values()].filter((c) => c.weapon !== null && c.target !== null).map((c) => [c.weapon, c.target]);
    rows.push(
      h(
        'div',
        { class: 'row' },
        button('Shoot', () => act(st.active, { type: 'shoot', unit: u.id, assignments }), { class: 'primary', disabled: assignments.length === 0 }),
        h('span', { class: 'muted small' }, `${assignments.length} weapon${assignments.length === 1 ? '' : 's'} assigned`),
      ),
    );
    return h('div', { class: 'actions' }, rows);
  }

  function renderCharge(u: Unit | null, mine: boolean): HTMLElement {
    const st = s();
    const rows: (HTMLElement | null)[] = [h('div', { class: 'title' }, 'Charge phase')];
    if (!u || !mine) {
      rows.push(h('div', { class: 'hint' }, `Select one of your units, tick targets within ${st.params.charge_range}", then declare. The engine rolls 2D6; on success you make the charge move.`));
      return h('div', { class: 'actions' }, rows);
    }
    if (!onBoard(u)) return h('div', { class: 'actions' }, rows, h('div', { class: 'hint' }, 'That unit is not on the battlefield.'));
    if (u.flags.charged) return h('div', { class: 'actions' }, rows, h('div', { class: 'hint' }, `${u.spec.name} has already charged.`));
    if (u.charge_targets.length && !st.pending) return h('div', { class: 'actions' }, rows, h('div', { class: 'hint' }, `${u.spec.name} failed its charge this turn.`));
    if (u.flags.moved === 'Advance' || u.flags.moved === 'FallBack') return h('div', { class: 'actions' }, rows, h('div', { class: 'hint' }, `${u.spec.name} advanced or fell back and cannot charge.`));
    if (u.disembarked_after_move) return h('div', { class: 'actions' }, rows, h('div', { class: 'hint' }, `${u.spec.name} disembarked after its transport moved and cannot charge.`));
    if (engagedWith(u).length) return h('div', { class: 'actions' }, rows, h('div', { class: 'hint' }, `${u.spec.name} is already in engagement range.`));
    const near = enemiesOf(u.owner)
      .map((e) => ({ e, d: unitDistance(u, e) }))
      .filter((x) => x.d <= st.params.charge_range)
      .sort((a, b) => a.d - b.d);
    if (near.length === 0) rows.push(h('div', { class: 'hint' }, `No enemy unit within ${st.params.charge_range}".`));
    for (const { e, d } of near) {
      rows.push(
        h(
          'div',
          { class: 'row' },
          h(
            'label',
            { style: 'color:var(--fg)' },
            h('input', {
              type: 'checkbox',
              checked: ui.chargeTargets.has(e.id),
              onChange: (ev: Event) => {
                if ((ev.target as HTMLInputElement).checked) ui.chargeTargets.add(e.id);
                else ui.chargeTargets.delete(e.id);
                render();
              },
            }),
            ` ${e.spec.name} `,
            h('span', { class: 'muted small' }, `${fmt(d)}" · ${aliveModels(e).length} models`),
          ),
        ),
      );
    }
    rows.push(
      h(
        'div',
        { class: 'row' },
        button('Declare charge', () => act(st.active, { type: 'declare_charge', unit: u.id, targets: [...ui.chargeTargets] }), { class: 'primary', disabled: ui.chargeTargets.size === 0 }),
        h('span', { class: 'muted small' }, 'The roll must reach every target; a failed charge ends the unit\'s charge phase.'),
      ),
    );
    return h('div', { class: 'actions' }, rows);
  }

  function renderFight(actor: number, u: Unit | null, mine: boolean): HTMLElement {
    const st = s();
    const next = session.nextFighter();
    const nextUnit = unit(next < 0 ? null : next);
    const rows: (HTMLElement | null)[] = [h('div', { class: 'title' }, 'Fight phase')];
    if (!nextUnit) {
      rows.push(h('div', { class: 'hint' }, 'No unit can fight. End the turn.'));
      return h('div', { class: 'actions' }, rows);
    }
    rows.push(
      h('div', { class: 'hint' }, `${st.players[actor].name} picks the next unit to fight. Next in order: `, h('b', null, nextUnit.spec.name), nextUnit.flags.charged ? ' (charged this turn)' : '', '. Chargers and Fights First units go before the rest; then the players alternate.'),
    );
    if (!u || !mine) {
      rows.push(h('div', { class: 'hint' }, 'Select one of your units that is in engagement range.'));
      return h('div', { class: 'actions' }, rows);
    }
    if (!onBoard(u)) return h('div', { class: 'actions' }, rows, h('div', { class: 'hint' }, 'That unit is not on the battlefield.'));
    if (u.flags.fought) return h('div', { class: 'actions' }, rows, h('div', { class: 'hint' }, `${u.spec.name} has already fought.`));
    if (ui.fightUnit !== u.id) {
      ui.fight = { target: null, pileIn: null, consolidate: null };
      ui.fightUnit = u.id;
    }
    const er = st.params.engagement_range;
    const reach = er + st.params.pile_in;
    const near = enemiesOf(u.owner)
      .map((e) => ({ e, d: unitDistance(u, e) }))
      .filter((x) => x.d <= reach)
      .sort((a, b) => a.d - b.d);
    if (near.every((x) => x.d > er)) rows.push(h('div', { class: 'hint' }, `${u.spec.name} is not within ${er}" of an enemy and cannot fight.`));
    const options = [{ value: '', label: '— choose a target —' }, ...near.map((x) => ({ value: String(x.e.id), label: `${x.e.spec.name} (${fmt(x.d)}"${x.d > er ? ', needs pile in' : ''})` }))];
    rows.push(
      h(
        'div',
        { class: 'row' },
        h('label', null, 'Target '),
        select(options, ui.fight.target === null ? '' : String(ui.fight.target), (v) => {
          ui.fight.target = v === '' ? null : Number(v);
          render();
        }),
      ),
      h(
        'div',
        { class: 'row' },
        button(ui.fight.pileIn ? 'Pile in: set ✓' : 'Pile in…', () => makePlacement(u, 'pile_in', `Pile in ${st.params.pile_in}"`, `Each model may move up to ${st.params.pile_in}" and must end as close or closer to the nearest enemy model.`, st.params.pile_in, null, true)),
        ui.fight.pileIn
          ? button(
              'clear',
              () => {
                ui.fight.pileIn = null;
                render();
              },
              { class: 'small' },
            )
          : null,
        button(ui.fight.consolidate ? 'Consolidate: set ✓' : 'Consolidate…', () => makePlacement(u, 'consolidate', `Consolidate ${st.params.consolidate}"`, `After the attacks: each model may move up to ${st.params.consolidate}" and must end as close or closer to the nearest enemy model. Planned from the current positions; if models die in the fight the move may be refused and you can fight again without it.`, st.params.consolidate, null, true)),
        ui.fight.consolidate
          ? button(
              'clear',
              () => {
                ui.fight.consolidate = null;
                render();
              },
              { class: 'small' },
            )
          : null,
      ),
      h(
        'div',
        { class: 'row' },
        button('Fight', () => act(u.owner, { type: 'fight', unit: u.id, target: ui.fight.target, pile_in: ui.fight.pileIn, consolidate: ui.fight.consolidate }), { class: 'primary', disabled: ui.fight.target === null }),
      ),
    );
    return h('div', { class: 'actions' }, rows);
  }

  function renderEndPhase(actor: number): HTMLElement | null {
    const st = s();
    if (st.pending || st.phase === 'Muster' || st.phase === 'Deployment' || st.phase === 'Ended') return null;
    if (actor !== st.active) return null;
    let label = 'End phase';
    let blocked: string | null = null;
    switch (st.phase) {
      case 'PreBattle':
        label = 'Done with scout moves';
        break;
      case 'Command':
        label = 'End Command phase → Movement';
        break;
      case 'Movement':
        label = 'End Movement → Shooting';
        break;
      case 'Shooting':
        label = 'End Shooting → Charge';
        break;
      case 'Charge':
        label = 'End Charge → Fight';
        break;
      case 'Fight': {
        label = st.active === 1 - st.first_player && st.round >= st.params.battle_rounds ? 'End turn → battle ends' : 'End turn';
        const n = session.nextFighter();
        if (n >= 0) blocked = `${st.units[n].spec.name} must still fight`;
        break;
      }
    }
    return h(
      'div',
      { class: 'endphase row' },
      button(label, () => act(st.active, { type: 'end_phase' }), { disabled: blocked !== null }),
      blocked ? h('span', { class: 'muted small' }, blocked) : null,
    );
  }

  // ------------------------------------------------------------ army box

  function renderArmyBox(actor: number): HTMLElement | null {
    const st = s();
    const p = st.players[actor];
    const own = st.units.filter((u) => u.owner === actor && u.merged_into === null && aliveModels(u).length > 0);
    const pending = new Map<string, string[]>();
    for (const u of own) {
      for (const [name, opts] of unitBuffs(game, u.id).choices_pending) if (!pending.has(name)) pending.set(name, opts);
      if (pending.size >= 4) break;
    }
    for (const [name] of pending) if (name in p.stances) pending.delete(name);
    const chosen = Object.entries(p.stances);
    if (p.rules.length === 0 && pending.size === 0 && chosen.length === 0) return null;
    const canChoose = actor === st.active && st.phase !== 'Ended';
    const choiceRows: HTMLElement[] = [];
    const renderChoice = (name: string, opts: string[], current: number | null) =>
      h(
        'div',
        { class: 'row small' },
        h('b', null, name, ': '),
        opts.map((o, i) =>
          h(
            'label',
            { style: 'color:var(--fg)' },
            h('input', {
              type: 'radio',
              name: `stance-${actor}-${name}`,
              checked: current === i,
              disabled: !canChoose,
              onChange: () => act(actor, { type: 'select_stance', name, option: i }),
            }),
            ' ',
            o,
          ),
        ),
        current === null ? h('span', { class: 'badge warn' }, 'choose') : null,
      );
    for (const [name, opts] of pending) choiceRows.push(renderChoice(name, opts, null));
    for (const [name, idx] of chosen) {
      let opts: string[] | null = null;
      for (const u of own) {
        const found = unitBuffs(game, u.id).choices_pending.find(([n]) => n === name);
        if (found) {
          opts = found[1];
          break;
        }
      }
      choiceRows.push(opts ? renderChoice(name, opts, idx) : h('div', { class: 'row small' }, h('b', null, name, ': '), `option ${idx + 1}`, canChoose ? button('change', () => act(actor, { type: 'select_stance', name, option: (idx + 1) % 2 }), { class: 'small' }) : null));
    }
    return h(
      'div',
      { class: 'actions' },
      h('div', { class: 'title' }, `${p.name} army rules`),
      p.rules.length ? h('div', { class: 'small' }, p.rules.map((r) => h('span', { class: `badge ${r.effect ? '' : 'warn'}`, title: r.effect ? '' : 'apply by hand' }, r.name))) : null,
      choiceRows,
      !canChoose && choiceRows.length ? h('div', { class: 'hint' }, 'Options can be changed on your own turn.') : null,
    );
  }

  // ------------------------------------------------------------ unit list

  function renderUnitList(actor: number): HTMLElement {
    const st = s();
    const owner = ui.listOwner ?? actor;
    const units = st.units.filter((u) => u.owner === owner && u.merged_into === null);
    const tabs = h(
      'div',
      { class: 'row', style: 'margin:8px 0 4px' },
      st.players.map((p, i) =>
        button(
          p.name,
          () => {
            ui.listOwner = i;
            render();
          },
          { class: `small ${owner === i ? 'primary' : ''}` },
        ),
      ),
    );
    const items = units.map((u) => {
      const alive = aliveModels(u).length;
      const gone = alive === 0;
      const badges: (HTMLElement | null)[] = [];
      if (gone) badges.push(h('span', { class: 'badge err' }, 'destroyed'));
      else if (u.embarked_in !== null) badges.push(h('span', { class: 'badge' }, `in ${unit(u.embarked_in)?.spec.name ?? 'transport'}`));
      else if (u.in_reserve) badges.push(h('span', { class: 'badge' }, 'reserve'));
      else if (!u.deployed) badges.push(h('span', { class: 'badge warn' }, 'to deploy'));
      if (u.attached_leaders.length) badges.push(h('span', { class: 'badge ok' }, `+${u.attached_leaders.join(', +')}`));
      const passengers = st.units.filter((x) => x.embarked_in === u.id);
      if (passengers.length) badges.push(h('span', { class: 'badge' }, `carrying ${passengers.map((x) => x.spec.name).join(', ')}`));
      if (u.battle_shocked) badges.push(h('span', { class: 'badge warn' }, 'shocked'));
      if (!gone && onBoard(u) && u.owner === st.active && st.round > 0) {
        if (u.flags.moved) badges.push(h('span', { class: 'badge' }, u.flags.moved === 'Normal' ? 'moved' : u.flags.moved === 'Advance' ? 'advanced' : u.flags.moved === 'FallBack' ? 'fell back' : 'stationary'));
        if (u.flags.shot) badges.push(h('span', { class: 'badge' }, 'shot'));
        if (u.flags.charged) badges.push(h('span', { class: 'badge' }, 'charged'));
        if (u.flags.fought) badges.push(h('span', { class: 'badge' }, 'fought'));
      }
      if (st.phase === 'Fight' && onBoard(u) && !u.flags.fought && engagedWith(u).length) badges.push(h('span', { class: 'badge warn' }, 'can fight'));
      return h(
        'li',
        {
          class: `${ui.selected === u.id ? 'sel' : ''} ${gone ? 'gone' : ''}`,
          onClick: () => {
            selectUnit(u.id);
            render();
          },
        },
        h('span', { class: 'name' }, u.spec.name),
        h('span', { class: 'count' }, `${alive}/${u.starting_models}`),
        badges,
      );
    });
    return h('div', null, h('h3', null, 'Units'), tabs, h('ul', { class: 'unitlist' }, items));
  }

  // ------------------------------------------------------------ unit panel

  function weaponStat(w: number, u: Unit): string {
    const spec = u.spec.weapons[w];
    const p = spec.profile;
    const parts = [spec.range !== null ? `${spec.range}"` : 'melee', `A${diceStr(p.attacks)}`, p.skill === null ? 'auto' : `${p.skill}+`, `S${p.strength}`, `AP${p.ap}`, `D${diceStr(p.damage)}`];
    if (p.abilities.length) parts.push(p.abilities.map(enumStr).join(', '));
    return parts.join(' · ');
  }

  function renderUnitPanel(): HTMLElement | null {
    const u = selectedUnit();
    if (!u) return null;
    const st = s();
    const stats = u.spec.stats;
    const alive = aliveModels(u);
    const carried = new Map<number, number>();
    for (const m of alive) for (const w of m.weapons) carried.set(w, (carried.get(w) ?? 0) + 1);
    const weaponRows = [...carried.entries()]
      .sort((a, b) => a[0] - b[0])
      .map(([w, n]) => {
        const spec = u.spec.weapons[w];
        const p = spec.profile;
        return h(
          'tr',
          null,
          h('td', null, p.name, n > 1 ? h('span', { class: 'muted' }, ` ×${n}`) : null, spec.profile_group ? h('span', { class: 'muted' }, ` [${spec.profile_group}]`) : null),
          h('td', null, spec.range !== null ? `${spec.range}"` : '—'),
          h('td', null, diceStr(p.attacks)),
          h('td', null, p.skill === null ? '—' : `${p.skill}+`),
          h('td', null, String(p.strength)),
          h('td', null, `-${p.ap}`),
          h('td', null, diceStr(p.damage)),
          h('td', { class: 'muted' }, p.abilities.map(enumStr).join(', ')),
        );
      });
    let buffs: Buffs | null = null;
    if (alive.length && u.merged_into === null) {
      try {
        buffs = unitBuffs(game, u.id);
      } catch {
        buffs = null;
      }
    }
    const modelLines = alive.map((m, i) => `${i + 1}. ${m.name}${m.is_leader ? ' (leader)' : ''} ${m.wounds}/${m.max_wounds}W${m.pos ? '' : ' (off board)'}`);
    return h(
      'div',
      { class: 'unitpanel' },
      h('h3', null, 'Unit'),
      h('div', null, h('b', { class: `p${u.owner}` }, u.spec.name), h('span', { class: 'muted' }, ` · ${u.spec.points} pts · ${st.players[u.owner].name}`)),
      u.attached_leaders.length ? h('div', { class: 'small' }, 'Leaders: ', u.attached_leaders.join(', ')) : null,
      h('div', { class: 'muted small' }, u.spec.keywords.join(' · ')),
      h(
        'div',
        { class: 'stats' },
        h('span', null, `M ${stats.movement}"`),
        h('span', null, `T ${stats.toughness}`),
        h('span', null, `Sv ${stats.save}+`),
        h('span', null, `W ${stats.wounds}`),
        h('span', null, `Ld ${stats.leadership}+`),
        h('span', null, `OC ${stats.objective_control}`),
        stats.invuln !== null ? h('span', null, `Inv ${stats.invuln}+`) : null,
        h('span', null, `base ${stats.base_mm}mm`),
      ),
      u.spec.core_rules.length ? h('div', { class: 'small' }, h('span', { class: 'muted' }, 'Core: '), u.spec.core_rules.map(coreRuleStr).join(', ')) : null,
      h('div', { class: 'small muted' }, modelLines.join(' · ')),
      weaponRows.length
        ? h(
            'table',
            null,
            h('thead', null, h('tr', null, h('th', null, 'Weapon'), h('th', null, 'Rng'), h('th', null, 'A'), h('th', null, 'Hit'), h('th', null, 'S'), h('th', null, 'AP'), h('th', null, 'D'), h('th', null, ''))),
            h('tbody', null, weaponRows),
          )
        : null,
      u.spec.abilities.length
        ? h(
            'div',
            { class: 'small' },
            h('span', { class: 'muted' }, 'Abilities: '),
            h(
              'ul',
              null,
              u.spec.abilities.map((a) => h('li', { class: a.effect ? '' : 'hand' }, a.name, a.effect ? '' : ' — apply by hand')),
            ),
          )
        : null,
      buffs ? renderBuffs(buffs) : null,
    );
  }

  function renderBuffs(b: Buffs): HTMLElement {
    const chips: string[] = [];
    if (b.feel_no_pain !== null) chips.push(`FNP ${b.feel_no_pain}+`);
    if (b.invuln !== null) chips.push(`Inv ${b.invuln}+`);
    if (b.hit_modifier) chips.push(`hit ${signed(b.hit_modifier)}`);
    if (b.wound_modifier) chips.push(`wound ${signed(b.wound_modifier)}`);
    if (b.reroll_hits !== 'None') chips.push(`re-roll hits: ${b.reroll_hits.toLowerCase()}`);
    if (b.reroll_wounds !== 'None') chips.push(`re-roll wounds: ${b.reroll_wounds.toLowerCase()}`);
    for (const [k, v] of b.stat_add) chips.push(`${k} ${signed(v)}`);
    if (b.move_add) chips.push(`M ${signed(b.move_add)}"`);
    if (b.oc_add) chips.push(`OC ${signed(b.oc_add)}`);
    if (b.oc_sticky) chips.push('sticky objectives');
    if (b.damage_reduction) chips.push(`damage -${b.damage_reduction}`);
    if (b.fights_first) chips.push('Fights First');
    if (b.deep_strike) chips.push('Deep Strike');
    if (b.stealth) chips.push('Stealth');
    if (b.lone_operative) chips.push('Lone Operative');
    for (const [k, t] of b.weapon_abilities) chips.push(`${k}${t ? ` (${t.toLowerCase()})` : ''}`);
    if (b.enemy_melee_hit_modifier) chips.push(`enemy melee hit ${signed(b.enemy_melee_hit_modifier)}`);
    if (b.critical_hit_on !== null) chips.push(`crit hit on ${b.critical_hit_on}+`);
    if (b.recovery) chips.push(`recovers ${b.recovery}`);
    for (const k of b.keywords) chips.push(`+${k}`);
    return h(
      'div',
      { class: 'small' },
      h('span', { class: 'muted' }, 'In effect now: '),
      chips.length ? h('div', { class: 'stats' }, chips.map((c) => h('span', null, c))) : h('span', { class: 'muted' }, 'nothing beyond the datasheet'),
      b.unsupported.length ? h('div', null, h('span', { class: 'muted' }, 'Apply by hand: '), h('ul', null, b.unsupported.map((n) => h('li', { class: 'hand' }, n)))) : null,
    );
  }

  // ------------------------------------------------------------ log

  function renderLog(): void {
    const st = s();
    const lines = st.log.map((ev) => {
      const dice = ev.text.startsWith('  ');
      const roundLine = ev.text.startsWith('Battle round') || ev.text.startsWith('Roll-off') || ev.text.startsWith('Battle over');
      return h(
        'div',
        { class: `line ${dice ? 'dice' : ''} ${dice && !ui.showDice ? 'collapsed' : ''} ${roundLine ? 'round' : ''}` },
        ev.player !== null && !dice ? h('span', { style: `color:${PLAYER_COLOURS[ev.player]}` }, '▸ ') : null,
        ev.text.trim(),
      );
    });
    logWrap.replaceChildren(h('div', { class: 'log' }, lines));
    logWrap.scrollTop = logWrap.scrollHeight;
  }

  render();
  return root;
}

// ------------------------------------------------------------ formatting

function diceStr(d: DiceExpr): string {
  if (d.count === 0) return String(d.add);
  const base = `${d.count > 1 ? d.count : ''}${d.sides}`;
  return d.add > 0 ? `${base}+${d.add}` : d.add < 0 ? `${base}${d.add}` : base;
}

function spaced(name: string): string {
  return name.replace(/([a-z])([A-Z])/g, '$1 $2');
}

function enumStr(v: EnumValue): string {
  if (typeof v === 'string') return spaced(v);
  const entries = Object.entries(v);
  if (entries.length === 0) return '?';
  const [k, val] = entries[0];
  if (k === 'Anti' && val && typeof val === 'object') {
    const a = val as { keyword: string; threshold: number };
    return `Anti-${a.keyword} ${a.threshold}+`;
  }
  if (val && typeof val === 'object') {
    if ('count' in (val as object)) return `${spaced(k)} ${diceStr(val as DiceExpr)}`;
    return `${spaced(k)} ${Object.values(val as object).join(' ')}`;
  }
  return `${spaced(k)} ${String(val)}`;
}

function coreRuleStr(v: EnumValue): string {
  if (typeof v === 'string') return spaced(v);
  const [k, val] = Object.entries(v)[0];
  switch (k) {
    case 'FeelNoPain':
      return `Feel No Pain ${val}+`;
    case 'Scouts':
      return `Scouts ${val}"`;
    case 'DeadlyDemise':
      return `Deadly Demise ${val}`;
    case 'FiringDeck':
      return `Firing Deck ${val}`;
    case 'Other':
      return String(val);
    default:
      return `${spaced(k)} ${String(val)}`;
  }
}

function signed(n: number): string {
  return n > 0 ? `+${n}` : String(n);
}
