import { Game, errorMessage, loadMission, loadPack, type LoadedPack } from './engine';
import type { App } from './main';
import { clearSave, loadGame } from './storage';
import { button, clear, h, select, toast } from './ui';

const FIXTURES: { value: string; label: string }[] = [
  { value: '', label: '— choose an army —' },
  { value: 'fixtures/mechanicus.ros', label: 'Mechanicus' },
  { value: 'fixtures/custodes.ros', label: 'Custodes' },
  { value: 'fixtures/necrons.ros', label: 'Necrons' },
  { value: 'upload', label: 'Upload a .ros file…' },
];

const MISSIONS: { value: string; label: string }[] = [
  { value: 'dawn-of-war', label: 'Dawn of War' },
  { value: 'hammer-and-anvil', label: 'Hammer and Anvil' },
  { value: 'tipping-point', label: 'Tipping Point' },
  { value: 'sweeping-engagement', label: 'Sweeping Engagement' },
  { value: 'crucible-of-battle', label: 'Crucible of Battle' },
  { value: 'search-and-destroy', label: 'Search and Destroy' },
];

function randomSeed(): string {
  return String(Math.floor(Math.random() * 2 ** 48));
}

export function renderSetup(app: App): HTMLElement {
  const packs: (LoadedPack | null)[] = [null, null];
  let mission = 'dawn-of-war';
  const seedInput = h('input', { type: 'text', value: randomSeed(), size: 18 });

  const slotEls = [0, 1].map((i) => renderSlot(i, (p) => (packs[i] = p)));

  const startBtn = button(
    'Start battle',
    async () => {
      if (!packs[0] || !packs[1]) {
        toast('pick two armies first');
        return;
      }
      let seed: bigint;
      try {
        seed = BigInt(seedInput.value.trim() || randomSeed());
      } catch {
        toast('seed must be a whole number');
        return;
      }
      try {
        const missionJson = await loadMission(mission);
        const game = new Game(`[${packs[0].json},${packs[1].json}]`, missionJson, seed);
        clearSave();
        app.startGame(game);
      } catch (e) {
        toast(errorMessage(e));
      }
    },
    { class: 'primary' },
  );

  const demoBtn = button('Demo armies', () => {
    let seed: bigint;
    try {
      seed = BigInt(seedInput.value.trim() || randomSeed());
    } catch {
      seed = BigInt(randomSeed());
    }
    clearSave();
    app.startGame(Game.demo(seed));
  });

  const saved = loadGame();
  const resume = saved
    ? h(
        'div',
        { class: 'card', style: 'margin-bottom:16px' },
        h('b', null, 'Saved battle: '),
        `${saved.meta.players.join(' vs ')} · ${saved.meta.phase}${saved.meta.round ? ` · round ${saved.meta.round}` : ''} · ${new Date(saved.meta.savedAt).toLocaleString()} `,
        button(
          'Resume',
          () => {
            try {
              app.startGame(Game.restore(saved.json));
            } catch (e) {
              toast('could not restore: ' + errorMessage(e));
            }
          },
          { class: 'primary' },
        ),
        ' ',
        button('Discard', () => {
          clearSave();
          app.showSetup();
        }),
      )
    : null;

  return h(
    'div',
    { class: 'setup' },
    h('h1', null, 'grimtable'),
    h('p', { class: 'muted' }, 'Hotseat, rules-enforced battle. Both players share this browser and take turns.'),
    resume,
    h('div', { class: 'slots' }, slotEls),
    h(
      'div',
      { class: 'options' },
      h('label', null, 'Deployment', select(MISSIONS, mission, (v) => (mission = v))),
      h('label', null, 'Seed', seedInput),
      startBtn,
      demoBtn,
    ),
    h('p', { class: 'muted small' }, 'Army lists come from New Recruit .ros exports. Numbers and effect data only; powered by 40kdc-data and BSData.'),
  );
}

function renderSlot(i: number, onPack: (p: LoadedPack | null) => void): HTMLElement {
  const summary = h('div', { class: 'summary' }, h('span', { class: 'muted' }, 'No army loaded.'));
  const fileInput = h('input', { type: 'file', accept: '.ros,.xml', hidden: true });

  async function load(text: string, label: string): Promise<void> {
    clear(summary);
    summary.appendChild(h('span', { class: 'muted' }, `Building ${label}…`));
    try {
      const p = await loadPack(text);
      onPack(p);
      clear(summary);
      const models = p.pack.units.reduce((n, u) => n + u.models.reduce((k, m) => k + m.count, 0), 0);
      summary.replaceChildren(
        h('div', null, h('b', null, p.pack.name), ` · ${p.pack.faction}`),
        h('div', null, p.pack.detachment ? `${p.pack.detachment} · ` : '', `${p.pack.points} pts · ${p.pack.units.length} units · ${models} models`),
        h('div', { class: 'small' }, `abilities: ${p.slug}`, p.pack.army_rules.length ? ` · army rules: ${p.pack.army_rules.map((r) => r.name).join(', ')}` : ''),
        ...(p.problems.length ? [h('div', { class: 'small', style: 'color:var(--warn)' }, 'Warnings: ', p.problems.join('; '))] : []),
      );
    } catch (e) {
      onPack(null);
      clear(summary);
      summary.appendChild(h('span', { style: 'color:var(--err)' }, errorMessage(e)));
    }
  }

  fileInput.addEventListener('change', async () => {
    const f = fileInput.files?.[0];
    if (!f) return;
    await load(await f.text(), f.name);
  });

  const sel = select(FIXTURES, '', async (v) => {
    if (v === 'upload') {
      fileInput.hidden = false;
      fileInput.click();
      return;
    }
    fileInput.hidden = true;
    if (!v) {
      onPack(null);
      clear(summary);
      summary.appendChild(h('span', { class: 'muted' }, 'No army loaded.'));
      return;
    }
    const r = await fetch(`/data/${v}`);
    if (!r.ok) {
      toast(`could not fetch /data/${v}; run scripts/sync-data.sh`);
      return;
    }
    await load(await r.text(), v);
  });

  return h('div', { class: 'card' }, h('h2', { class: `p${i}` }, `Player ${i + 1}`), h('div', { class: 'row' }, sel, fileInput), summary);
}
