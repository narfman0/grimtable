import type { App } from './main';
import type { Session } from './session';
import type { Unit } from './types';
import { button, h, select } from './ui';

function hasRule(u: Unit, rule: string): boolean {
  return u.spec.core_rules.includes(rule);
}

export function isLeaderUnit(u: Unit): boolean {
  return hasRule(u, 'Leader') || hasRule(u, 'Support');
}

export function eligibleBodyguards(units: Unit[], leader: Unit): Unit[] {
  const names = leader.spec.can_lead.map((n) => n.toLowerCase());
  return units.filter((u) => u.owner === leader.owner && u.id !== leader.id && u.merged_into === null && names.includes(u.spec.name.toLowerCase()));
}

export function renderMuster(app: App, session: Session): HTMLElement {
  const root = h('div', { class: 'muster' });

  function render(): void {
    const s = session.state;
    if (s.phase !== 'Muster') {
      app.startGame(session.game);
      return;
    }
    root.replaceChildren(
      h('h1', null, 'Muster'),
      h('p', { class: 'muted' }, 'Attach Leaders and Support units to the units they can lead. Each player marks done when finished; the engine then rolls off for who deploys first.'),
      h(
        'div',
        { class: 'armies' },
        [0, 1].map((p) => renderArmy(p)),
      ),
    );
  }

  function renderArmy(p: number): HTMLElement {
    const s = session.state;
    const player = s.players[p];
    const done = s.fight_queue.includes(p);
    const units = s.units.filter((u) => u.owner === p && u.merged_into === null);
    const choices = new Map<number, number>();
    const items = units.map((u) => {
      const badges = [
        u.attached_leaders.length ? h('span', { class: 'badge ok' }, `+ ${u.attached_leaders.join(', ')}`) : null,
        isLeaderUnit(u) ? h('span', { class: 'badge' }, hasRule(u, 'Leader') ? 'Leader' : 'Support') : null,
      ];
      let attach: HTMLElement | null = null;
      if (isLeaderUnit(u) && !done) {
        const bodyguards = eligibleBodyguards(s.units, u);
        if (bodyguards.length) {
          choices.set(u.id, bodyguards[0].id);
          attach = h(
            'div',
            { class: 'attach' },
            select(
              bodyguards.map((b) => ({ value: String(b.id), label: `${b.spec.name} (${b.models.length})${b.attached_leaders.length ? ' + ' + b.attached_leaders.join(', ') : ''}` })),
              String(bodyguards[0].id),
              (v) => choices.set(u.id, Number(v)),
            ),
            button(
              'Attach',
              () => {
                if (session.apply(p, { type: 'attach', leader: u.id, bodyguard: choices.get(u.id) })) render();
              },
              { class: 'small' },
            ),
          );
        } else {
          attach = h('div', { class: 'muted small' }, u.spec.can_lead.length ? 'no eligible unit in this army' : 'leads nothing in this list');
        }
      }
      return h('li', null, h('div', null, u.spec.name, ' ', h('span', { class: 'muted small' }, `${u.models.length} models · ${u.spec.points} pts`), badges), attach);
    });
    return h(
      'div',
      { class: 'card' },
      h('h2', { class: `p${p}` }, player.name),
      h('p', { class: 'muted' }, `${player.faction}${player.detachment ? ' · ' + player.detachment : ''}`),
      h('ul', null, items),
      h(
        'div',
        { style: 'margin-top:12px' },
        done
          ? h('span', { class: 'badge ok' }, 'done, waiting for the other player')
          : button(
              `${player.name} done`,
              () => {
                if (session.apply(p, { type: 'finish_muster' })) render();
              },
              { class: 'primary' },
            ),
      ),
    );
  }

  render();
  return root;
}
