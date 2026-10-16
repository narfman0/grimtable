// Army editor for `kind: "def"` armies: a datasheet browser on the left, the
// list on the right. Points and detachment points are computed locally the
// same way crates/content/src/army_def.rs does; the server's validation
// problems are shown after every save.

import { api, errorText } from './api';
import type { App } from './main';
import type { ArmyDef, ArmyPack, Catalogue, CatalogueWeapon, Datasheet, Enhancement, Id, ModelRow, ModelRowDef, UnitDef } from './types';
import { button, h, select, toast } from './ui';

export type EditorTarget = { factionId: string } | { armyId: string };

const GROUPS = ['Epic Hero', 'Character', 'Battleline', 'Infantry / other', 'Vehicle / Monster', 'Dedicated Transport'] as const;
type Group = (typeof GROUPS)[number];

function hasKeyword(d: Datasheet, k: string): boolean {
  return d.keywords.some((x) => x.toLowerCase() === k.toLowerCase());
}

function groupOf(d: Datasheet): Group {
  if (d.is_epic_hero || d.role === 'epic-hero') return 'Epic Hero';
  if (d.is_dedicated_transport || d.role === 'dedicated-transport') return 'Dedicated Transport';
  if (hasKeyword(d, 'Character') || d.role === 'character') return 'Character';
  if (d.is_battleline || d.role === 'battleline') return 'Battleline';
  if (hasKeyword(d, 'Vehicle') || hasKeyword(d, 'Monster')) return 'Vehicle / Monster';
  return 'Infantry / other';
}

/** Points for a unit: the tier matching the model count, else the smallest tier at or above, else the last. */
export function unitPoints(sheet: Datasheet, modelCount: number): number {
  if (!sheet.points.length) return 0;
  const tier = sheet.points.find((p) => p.models === modelCount) ?? sheet.points.find((p) => p.models >= modelCount) ?? sheet.points[sheet.points.length - 1];
  return tier.cost;
}

export function modelCount(u: UnitDef): number {
  return u.models.reduce((n, m) => n + m.count, 0);
}

export function defaultUnit(sheet: Datasheet): UnitDef {
  return {
    datasheet_id: sheet.id,
    models: sheet.models.map((r): ModelRowDef => ({ row: r.name, count: r.min, weapon_ids: [...r.default_weapon_ids], variants: [] })),
    enhancement_id: null,
    warlord: false,
  };
}

/** Weapons a model row may carry: its defaults, its loadout variants, and wargear replacements for that row or unconstrained. */
export function rowWeaponIds(sheet: Datasheet, row: ModelRow): string[] {
  const out: string[] = [...row.default_weapon_ids];
  for (const v of row.loadout_variants) out.push(...v.weapon_ids);
  for (const o of sheet.wargear_options) {
    const name = o.model_constraint?.model_name;
    if (name && name !== row.name) continue;
    if (o.replacement) out.push(...o.replacement);
    if (o.replacement_choice) for (const c of o.replacement_choice) out.push(...c);
  }
  return [...new Set(out)];
}

export function weaponProfileText(w: CatalogueWeapon | undefined): string {
  if (!w) return '';
  return w.profiles
    .map((p) => {
      const parts = [p.range === null ? 'melee' : `${p.range}"`, `A${p.attacks}`, p.skill === null ? 'auto' : `${p.skill}+`, `S${p.strength}`, `AP-${p.ap}`, `D${p.damage}`];
      if (p.keywords.length) parts.push(`[${p.keywords.join(', ')}]`);
      return (w.profiles.length > 1 ? `${p.name}: ` : '') + parts.join(' ');
    })
    .join(' / ');
}

export function renderEditor(app: App, target: EditorTarget): HTMLElement {
  const root = h('div', { class: 'editor' }, h('p', { class: 'loading' }, 'Loading catalogue…'));
  let cat: Catalogue | null = null;
  let def: ArmyDef = { name: '', faction_id: '', detachment_ids: [], units: [] };
  let armyId: Id | null = 'armyId' in target ? target.armyId : null;
  let problems: string[] = [];
  let search = '';
  let pointsLimit = 2000;
  let expanded = new Set<number>();
  let saving = false;

  const dpBudget = () => (pointsLimit <= 1000 ? 2 : 3);
  const sheetOf = (id: string) => cat?.units.find((u) => u.id === id);
  const weaponOf = (id: string) => cat?.weapons.find((w) => w.id === id);
  const enhancementOf = (id: string | null) => (id === null ? undefined : cat?.enhancements.find((e) => e.id === id));
  const unitCost = (u: UnitDef) => {
    const sheet = sheetOf(u.datasheet_id);
    return (sheet ? unitPoints(sheet, modelCount(u)) : 0) + (enhancementOf(u.enhancement_id)?.cost ?? 0);
  };
  const totalPoints = () => def.units.reduce((n, u) => n + unitCost(u), 0);
  const totalDp = () => def.detachment_ids.reduce((n, id) => n + (cat?.detachments.find((d) => d.id === id)?.detachment_points ?? 0), 0);

  async function load(): Promise<void> {
    try {
      if ('armyId' in target) {
        const rec = await api.army(target.armyId);
        if (rec.kind !== 'def' || !rec.def) {
          toast('imported armies cannot be edited; only their pack can be previewed');
          app.navigate('#/');
          return;
        }
        def = rec.def;
        def.name = rec.name || def.name;
        problems = rec.problems ?? [];
        cat = await api.catalogue(rec.def.faction_id);
      } else {
        cat = await api.catalogue(target.factionId);
        def = { name: `New ${cat.name} army`, faction_id: cat.faction_id, detachment_ids: [], units: [] };
      }
    } catch (e) {
      root.replaceChildren(h('div', { class: 'setup' }, h('p', { style: 'color:var(--err)' }, errorText(e)), button('Back', () => app.navigate('#/'))));
      return;
    }
    render();
  }

  // ------------------------------------------------------------ layout

  const browser = h('div', { class: 'browser' });
  const list = h('div', { class: 'list' });

  function render(): void {
    if (!cat) return;
    root.replaceChildren(
      h(
        'div',
        { class: 'row topbar' },
        button('← Home', () => app.navigate('#/'), { class: 'small' }),
        h('h1', null, `${cat.name} army editor`),
        h('span', { class: 'muted small' }, `points: ${cat.points_source}`),
      ),
      h('div', { class: 'cols' }, browser, list),
    );
    renderBrowser();
    renderList();
  }

  // ------------------------------------------------------------ browser

  function renderBrowser(): void {
    if (!cat) return;
    const q = search.trim().toLowerCase();
    const sheets = cat.units.filter((d) => !q || d.name.toLowerCase().includes(q) || d.keywords.some((k) => k.toLowerCase().includes(q)));
    const groups = new Map<Group, Datasheet[]>();
    for (const g of GROUPS) groups.set(g, []);
    for (const d of sheets) groups.get(groupOf(d))!.push(d);
    const input = h('input', {
      type: 'text',
      placeholder: 'search datasheets',
      value: search,
      onInput: (e: Event) => {
        search = (e.target as HTMLInputElement).value;
        renderBrowser();
        const again = browser.querySelector<HTMLInputElement>('input[type=text]');
        again?.focus();
        again?.setSelectionRange(search.length, search.length);
      },
    });
    const sections: HTMLElement[] = [];
    for (const [g, ds] of groups) {
      if (!ds.length) continue;
      sections.push(
        h('h3', null, g),
        h(
          'ul',
          { class: 'sheets' },
          ds
            .sort((a, b) => a.name.localeCompare(b.name))
            .map((d) =>
              h(
                'li',
                {
                  title: `${d.keywords.join(' · ')}\nM${d.stats.movement}" T${d.stats.toughness} Sv${d.stats.save}+ W${d.stats.wounds} Ld${d.stats.leadership}+ OC${d.stats.objective_control}`,
                  onClick: () => addUnit(d),
                },
                h('span', { class: 'name' }, d.name),
                h('span', { class: 'pts' }, d.points.map((p) => `${p.cost} (${p.models})`).join(' · ') || '—'),
              ),
            ),
        ),
      );
    }
    browser.replaceChildren(input, h('div', { class: 'muted small' }, 'Click a datasheet to add it with its minimum models and default weapons.'), ...sections);
  }

  function addUnit(d: Datasheet): void {
    def.units.push(defaultUnit(d));
    expanded.add(def.units.length - 1);
    renderList();
  }

  // ------------------------------------------------------------ list

  function renderList(): void {
    if (!cat) return;
    const pts = totalPoints();
    const dp = totalDp();
    const nameInput = h('input', {
      type: 'text',
      value: def.name,
      placeholder: 'army name',
      style: 'flex:1;min-width:200px',
      onInput: (e: Event) => (def.name = (e.target as HTMLInputElement).value),
    });
    const limitSel = select(
      [
        { value: '1000', label: 'Incursion · 1000 pts · 2 DP' },
        { value: '2000', label: 'Strike Force · 2000 pts · 3 DP' },
      ],
      String(pointsLimit),
      (v) => {
        pointsLimit = Number(v);
        renderList();
      },
    );
    const detachments = h(
      'div',
      { class: 'detachments' },
      cat.detachments.map((d) =>
        h(
          'label',
          { class: def.detachment_ids.includes(d.id) ? 'on' : '' },
          h('input', {
            type: 'checkbox',
            checked: def.detachment_ids.includes(d.id),
            onChange: (e: Event) => {
              const on = (e.target as HTMLInputElement).checked;
              def.detachment_ids = on ? [...def.detachment_ids, d.id] : def.detachment_ids.filter((x) => x !== d.id);
              // Enhancements from a dropped detachment no longer apply.
              for (const u of def.units) {
                const enh = enhancementOf(u.enhancement_id);
                if (enh && !def.detachment_ids.includes(enh.detachment_id)) u.enhancement_id = null;
              }
              renderList();
            },
          }),
          ` ${d.name} `,
          h('span', { class: 'badge' }, `${d.detachment_points} DP`),
        ),
      ),
    );
    const saveBtn = button(armyId === null ? 'Save army' : 'Save changes', () => void save(), { class: 'primary', disabled: saving });
    const packBtn = button('Pack preview', () => void showPack(), { disabled: armyId === null, title: armyId === null ? 'save first' : 'Fetch the engine pack the server builds from this army' });
    list.replaceChildren(
      h(
        'div',
        { class: 'listbody' },
      h('div', { class: 'row' }, nameInput, limitSel),
      h('h3', null, 'Detachments ', h('span', { class: `badge ${dp > dpBudget() ? 'err' : dp === 0 ? 'warn' : 'ok'}` }, `${dp} / ${dpBudget()} DP`)),
      detachments,
      h('h3', null, 'Units ', h('span', { class: `badge ${pts > pointsLimit ? 'err' : 'ok'}` }, `${pts} / ${pointsLimit} pts`)),
      def.units.length ? h('div', { class: 'units' }, def.units.map((u, i) => renderUnit(u, i))) : h('p', { class: 'muted' }, 'No units. Pick datasheets on the left.'),
      h('div', { class: 'row totals' }, h('b', null, `${pts} / ${pointsLimit} pts`), h('span', { class: 'muted' }, `· ${def.units.length} units · ${dp} / ${dpBudget()} DP`), h('span', { style: 'flex:1' }), packBtn, saveBtn),
      problems.length
        ? h('div', { class: 'problems' }, h('b', null, `${problems.length} problem${problems.length === 1 ? '' : 's'} reported by the server:`), h('ul', null, problems.map((p) => h('li', null, p))))
        : armyId !== null
          ? h('div', { class: 'small', style: 'color:var(--ok)' }, 'The server reports no problems with the saved army.')
          : null,
      h('div', { id: 'packpreview' }),
      ),
    );
  }

  function renderUnit(u: UnitDef, i: number): HTMLElement {
    const sheet = sheetOf(u.datasheet_id);
    if (!sheet) return h('div', { class: 'unit card' }, h('b', null, u.datasheet_id), ' ', h('span', { class: 'badge err' }, 'not in the catalogue'), ' ', button('remove', () => removeUnit(i), { class: 'small danger' }));
    const open = expanded.has(i);
    const isCharacter = hasKeyword(sheet, 'Character');
    const enhancements = cat!.enhancements.filter((e) => def.detachment_ids.includes(e.detachment_id));
    const head = h(
      'div',
      { class: 'row head' },
      button(open ? '▾' : '▸', () => {
        if (open) expanded.delete(i);
        else expanded.add(i);
        renderList();
      }, { class: 'small', title: open ? 'collapse' : 'expand' }),
      h('b', null, sheet.name),
      h('span', { class: 'muted' }, `${modelCount(u)} model${modelCount(u) === 1 ? '' : 's'}`),
      u.enhancement_id ? h('span', { class: 'badge' }, enhancementOf(u.enhancement_id)?.name ?? u.enhancement_id) : null,
      u.warlord ? h('span', { class: 'badge ok' }, 'Warlord') : null,
      h('span', { style: 'flex:1' }),
      h('b', null, `${unitCost(u)} pts`),
      isCharacter
        ? h(
            'label',
            { class: 'small', title: 'One Warlord per army' },
            h('input', {
              type: 'radio',
              name: 'warlord',
              checked: u.warlord,
              onChange: () => {
                for (const x of def.units) x.warlord = false;
                u.warlord = true;
                renderList();
              },
            }),
            ' Warlord',
          )
        : null,
      button('duplicate', () => {
        def.units.splice(i + 1, 0, { ...structuredClone(u), warlord: false });
        renderList();
      }, { class: 'small' }),
      button('remove', () => removeUnit(i), { class: 'small danger' }),
    );
    if (!open) return h('div', { class: 'unit card' }, head);

    const rows = sheet.models.map((row) => {
      let m = u.models.find((x) => x.row === row.name);
      if (!m) {
        m = { row: row.name, count: row.min, weapon_ids: [...row.default_weapon_ids], variants: [] };
        u.models.push(m);
      }
      const md = m;
      const countInput = h('input', {
        type: 'number',
        min: row.min,
        max: row.max,
        value: String(md.count),
        style: 'width:64px',
        disabled: row.min === row.max,
        onChange: (e: Event) => {
          const el = e.target as HTMLInputElement;
          const n = Math.max(row.min, Math.min(row.max, Math.round(Number(el.value) || row.min)));
          el.value = String(n);
          md.count = n;
          renderList();
        },
      });
      const allowed = rowWeaponIds(sheet, row);
      const weapons = allowed.map((id) => {
        const w = weaponOf(id);
        const checked = md.weapon_ids.includes(id);
        return h(
          'label',
          { class: `weapon ${checked ? 'on' : ''}` },
          h('input', {
            type: 'checkbox',
            checked,
            onChange: (e: Event) => {
              const on = (e.target as HTMLInputElement).checked;
              md.weapon_ids = on ? [...md.weapon_ids, id] : md.weapon_ids.filter((x) => x !== id);
              renderList();
            },
          }),
          ' ',
          h('span', { class: 'wname' }, w?.name ?? id, row.default_weapon_ids.includes(id) ? h('span', { class: 'badge' }, 'default') : null),
          h('span', { class: 'wstat' }, weaponProfileText(w)),
        );
      });
      return h(
        'div',
        { class: 'modelrow' },
        h('div', { class: 'row' }, h('b', null, row.name), row.is_leader_model ? h('span', { class: 'badge' }, 'leader model') : null, h('label', null, 'models ', countInput), h('span', { class: 'muted small' }, row.min === row.max ? `fixed at ${row.min}` : `${row.min}–${row.max}`)),
        h('div', { class: 'weapons' }, weapons.length ? weapons : h('span', { class: 'muted small' }, 'no weapon data')),
      );
    });
    // Rows no longer on the datasheet (catalogue changed) are dropped silently on save.
    u.models = u.models.filter((m) => sheet.models.some((r) => r.name === m.row));

    const enhSel = isCharacter
      ? h(
          'div',
          { class: 'row' },
          h('label', null, 'Enhancement '),
          select(
            [{ value: '', label: sheet.is_epic_hero ? 'none (Epic Heroes cannot take one)' : def.detachment_ids.length ? '— none —' : 'choose a detachment first' }, ...enhancements.map((e: Enhancement) => ({ value: e.id, label: `${e.name} · ${e.cost} pts${e.keyword_restrictions.length ? ` (${e.keyword_restrictions.join(', ')})` : ''}` }))],
            u.enhancement_id ?? '',
            (v) => {
              u.enhancement_id = v || null;
              renderList();
            },
          ),
        )
      : null;

    return h(
      'div',
      { class: 'unit card open' },
      head,
      h('div', { class: 'muted small' }, sheet.keywords.join(' · '), ` · M${sheet.stats.movement}" T${sheet.stats.toughness} Sv${sheet.stats.save}+ W${sheet.stats.wounds} Ld${sheet.stats.leadership}+ OC${sheet.stats.objective_control}${sheet.stats.invuln ? ` Inv${sheet.stats.invuln}+` : ''}`),
      h('div', { class: 'muted small' }, 'Points: ', sheet.points.map((p) => `${p.models} models ${p.cost} pts`).join(' · ') || 'unknown'),
      rows,
      enhSel,
    );
  }

  function removeUnit(i: number): void {
    def.units.splice(i, 1);
    expanded = new Set([...expanded].filter((x) => x !== i).map((x) => (x > i ? x - 1 : x)));
    renderList();
  }

  // ------------------------------------------------------------ save / pack

  async function save(): Promise<void> {
    if (!def.name.trim()) {
      toast('give the army a name');
      return;
    }
    saving = true;
    renderList();
    try {
      const rec = armyId === null ? await api.createArmy(def.name.trim(), def) : await api.updateArmy(armyId, def.name.trim(), def);
      armyId = rec.id;
      problems = rec.problems ?? [];
      if (rec.def) def = rec.def;
      history.replaceState(null, '', `#/army/${encodeURIComponent(String(armyId))}`);
      toast(`saved ${rec.name}: ${rec.points} pts${problems.length ? ` · ${problems.length} problem${problems.length === 1 ? '' : 's'}` : ''}`, 'info');
    } catch (e) {
      toast(errorText(e));
    } finally {
      saving = false;
      renderList();
    }
  }

  async function showPack(): Promise<void> {
    if (armyId === null) return;
    let pack: ArmyPack;
    try {
      pack = await api.armyPack(armyId);
    } catch (e) {
      toast(errorText(e));
      return;
    }
    const models = pack.units.reduce((n, u) => n + u.models.reduce((k, m) => k + m.count, 0), 0);
    const box = root.querySelector('#packpreview');
    box?.replaceChildren(
      h(
        'div',
        { class: 'card packpreview' },
        h('div', { class: 'row', style: 'justify-content:space-between' }, h('b', null, `Pack: ${pack.name}`), button('close', () => box.replaceChildren(), { class: 'small' })),
        h('div', { class: 'muted' }, `${pack.faction}${pack.detachment ? ' · ' + pack.detachment : ''} · ${pack.points} pts · ${pack.units.length} units · ${models} models`),
        h('div', { class: 'small' }, 'Army rules: ', pack.army_rules.length ? pack.army_rules.map((r) => h('span', { class: `badge ${r.effect ? 'ok' : 'warn'}`, title: r.effect ? '' : 'no effect data; apply by hand' }, r.name)) : h('span', { class: 'muted' }, 'none')),
        h(
          'ul',
          { class: 'small' },
          pack.units.map((u) =>
            h('li', null, `${u.name} · ${u.points} pts · ${u.models.reduce((k, m) => k + m.count, 0)} models · ${u.weapons.length} weapon profiles`, u.abilities.some((a) => !a.effect) ? h('span', { class: 'badge warn' }, `${u.abilities.filter((a) => !a.effect).length} by hand`) : null),
          ),
        ),
      ),
    );
  }

  void load();
  return root;
}
