// JavaScript port of crates/content/src/army_def.rs for the mock server:
// validation and pack building for editor armies against a catalogue. The
// real server uses the Rust code; this exists so the mock can start a game
// from an editor army without a wasm export for it. Keep it in step.

export function unitPoints(sheet, modelCount) {
  if (!sheet.points.length) return 0;
  const tier = sheet.points.find((p) => p.models === modelCount) ?? sheet.points.find((p) => p.models >= modelCount) ?? sheet.points[sheet.points.length - 1];
  return tier.cost;
}

const modelCount = (u) => u.models.reduce((n, m) => n + m.count, 0);

export function totalPoints(def, cat) {
  return def.units.reduce((n, u) => {
    const sheet = cat.units.find((s) => s.id === u.datasheet_id);
    const base = sheet ? unitPoints(sheet, modelCount(u)) : 0;
    const enh = u.enhancement_id ? (cat.enhancements.find((e) => e.id === u.enhancement_id)?.cost ?? 0) : 0;
    return n + base + enh;
  }, 0);
}

export function detachmentPoints(def, cat) {
  return def.detachment_ids.reduce((n, d) => n + (cat.detachments.find((x) => x.id === d)?.detachment_points ?? 0), 0);
}

export function allowedWeapons(sheet, row) {
  const out = [...row.default_weapon_ids];
  for (const v of row.loadout_variants) out.push(...v.weapon_ids);
  for (const o of sheet.wargear_options) {
    const name = o.model_constraint?.model_name;
    if (name && name !== row.name) continue;
    if (Array.isArray(o.replacement)) out.push(...o.replacement);
    if (Array.isArray(o.replacement_choice)) for (const c of o.replacement_choice) out.push(...c);
  }
  out.push(...sheet.weapon_ids);
  return [...new Set(out)].sort();
}

const hasKw = (list, k) => list.some((x) => x.toLowerCase() === k.toLowerCase());

export function validate(def, cat, pointsLimit, dpBudget) {
  const problems = [];
  const points = totalPoints(def, cat);
  if (points > pointsLimit) problems.push(`${points} pts is over the ${pointsLimit} pt limit by ${points - pointsLimit}`);
  if (!def.detachment_ids.length) problems.push('no detachment selected');
  const dp = detachmentPoints(def, cat);
  if (dp > dpBudget) problems.push(`${dp} detachment points over the budget of ${dpBudget}`);
  for (const d of def.detachment_ids) if (!cat.detachments.some((x) => x.id === d)) problems.push(`unknown detachment ${d}`);
  if (!def.units.length) problems.push('no units');
  const warlords = def.units.filter((u) => u.warlord).length;
  if (warlords !== 1) problems.push(`exactly one Warlord is needed, found ${warlords}`);
  const counts = new Map();
  const used = [];
  for (const u of def.units) {
    const sheet = cat.units.find((s) => s.id === u.datasheet_id);
    if (!sheet) {
      problems.push(`unknown datasheet ${u.datasheet_id}`);
      continue;
    }
    counts.set(sheet.id, (counts.get(sheet.id) ?? 0) + 1);
    if (u.warlord && !hasKw(sheet.keywords, 'Character')) problems.push(`${sheet.name} cannot be the Warlord; it is not a Character`);
    for (const row of sheet.models) {
      const count = u.models.find((m) => m.row === row.name)?.count ?? 0;
      if (count < row.min || count > row.max) problems.push(`${sheet.name}: ${row.name} needs ${row.min}–${row.max} models, has ${count}`);
    }
    for (const m of u.models) {
      const row = sheet.models.find((r) => r.name === m.row);
      if (!row) {
        problems.push(`${sheet.name}: no model row named ${m.row}`);
        continue;
      }
      const allowed = allowedWeapons(sheet, row);
      for (const w of m.weapon_ids ?? []) if (!allowed.includes(w)) problems.push(`${sheet.name}: ${row.name} cannot carry ${w}`);
      for (const [v, n] of m.variants ?? []) {
        const variant = row.loadout_variants.find((x) => x.name === v);
        if (!variant) problems.push(`${sheet.name}: no loadout variant ${v}`);
        else if (variant.max_count !== null && n > variant.max_count) problems.push(`${sheet.name}: at most ${variant.max_count} × ${v}`);
      }
    }
    if (u.enhancement_id) {
      const enh = cat.enhancements.find((x) => x.id === u.enhancement_id);
      if (!enh) problems.push(`unknown enhancement ${u.enhancement_id}`);
      else {
        if (!def.detachment_ids.includes(enh.detachment_id)) problems.push(`${enh.name} belongs to a detachment not in this army`);
        if (sheet.is_epic_hero) problems.push(`${sheet.name} is an Epic Hero and cannot take ${enh.name}`);
        if (!hasKw(sheet.keywords, 'Character')) problems.push(`${sheet.name} is not a Character and cannot take ${enh.name}`);
        const kws = [...sheet.keywords, ...cat.faction_keywords].map((k) => k.toLowerCase());
        for (const req of enh.keyword_restrictions) {
          const ok = req.split(/\s+/).every((w) => kws.includes(w.toLowerCase())) || kws.includes(req.toLowerCase());
          if (!ok) problems.push(`${enh.name} requires the ${req} keyword; ${sheet.name} lacks it`);
        }
        if (used.includes(enh.id)) problems.push(`${enh.name} is taken more than once`);
        used.push(enh.id);
      }
    }
  }
  for (const [id, n] of counts) {
    const sheet = cat.units.find((s) => s.id === id);
    const limit = sheet.is_epic_hero ? 1 : sheet.is_battleline || sheet.is_dedicated_transport ? 6 : 3;
    if (n > limit) problems.push(`${sheet.name} appears ${n} times; the limit is ${limit}`);
  }
  return problems;
}

// ------------------------------------------------------------ pack building

/** engine::dice::DiceExpr::parse; null when the text is not a dice expression. */
export function parseDice(text) {
  const s = String(text).trim().toUpperCase().replace(/\s/g, '');
  if (/^-?\d+$/.test(s)) return { count: 0, sides: 'D6', add: Number(s) };
  let dice = s;
  let add = 0;
  const plus = s.indexOf('+');
  if (plus >= 0) {
    dice = s.slice(0, plus);
    add = Number(s.slice(plus + 1));
    if (!Number.isInteger(add)) return null;
  } else {
    const minus = s.lastIndexOf('-');
    if (minus > 0) {
      dice = s.slice(0, minus);
      add = -Number(s.slice(minus + 1));
      if (!Number.isInteger(add)) return null;
    }
  }
  let sides;
  let countText;
  if (dice.endsWith('D6')) {
    sides = 'D6';
    countText = dice.slice(0, -2);
  } else if (dice.endsWith('D3')) {
    sides = 'D3';
    countText = dice.slice(0, -2);
  } else return null;
  const count = countText === '' ? 1 : Number(countText);
  if (!Number.isInteger(count)) return null;
  return { count, sides, add };
}

const flat = (n) => ({ count: 0, sides: 'D6', add: n });

/** engine::attack::WeaponAbility::parse, as serde JSON; null when unknown. */
export function parseWeaponAbility(text) {
  const t = text.replace(/ /g, ' ').trim();
  const lower = t.toLowerCase();
  const simple = {
    assault: 'Assault',
    blast: 'Blast',
    'devastating wounds': 'DevastatingWounds',
    'extra attacks': 'ExtraAttacks',
    hazardous: 'Hazardous',
    heavy: 'Heavy',
    'ignores cover': 'IgnoresCover',
    'indirect fire': 'IndirectFire',
    lance: 'Lance',
    'lethal hits': 'LethalHits',
    pistol: 'Pistol',
    precision: 'Precision',
    psychic: 'Psychic',
    torrent: 'Torrent',
    'twin-linked': 'TwinLinked',
    'twin linked': 'TwinLinked',
  };
  if (simple[lower]) return simple[lower];
  const num = (prefix) => (lower.startsWith(prefix) ? lower.slice(prefix.length).trim() : null);
  let v;
  if ((v = num('melta')) !== null) return Number.isInteger(Number(v)) ? { Melta: Number(v) } : null;
  if ((v = num('rapid fire')) !== null) return Number.isInteger(Number(v)) ? { RapidFire: Number(v) } : null;
  if ((v = num('sustained hits')) !== null) {
    const d = parseDice(v);
    return d ? { SustainedHits: d } : null;
  }
  const rest = num('anti-') ?? num('anti ');
  if (rest !== null) {
    const sp = rest.lastIndexOf(' ');
    if (sp < 0) return null;
    const original = t.slice(t.length - rest.length);
    const keyword = original.slice(0, sp).trim();
    const threshold = Number(rest.slice(sp + 1).replace(/\+$/, ''));
    return Number.isInteger(threshold) ? { Anti: { keyword, threshold } } : null;
  }
  return null;
}

/** roster::CoreRule::parse, as engine::content::CoreRule serde JSON. */
export function parseCoreRule(name) {
  const s = name.replace(/ /g, ' ').trim().replace(/\*+$/, '').trim();
  const lower = s.toLowerCase();
  const simple = { 'deep strike': 'DeepStrike', infiltrators: 'Infiltrators', leader: 'Leader', 'lone operative': 'LoneOperative', stealth: 'Stealth', support: 'Support', 'fights first': 'FightsFirst' };
  if (simple[lower]) return simple[lower];
  const after = (prefix) => (lower.startsWith(prefix) ? s.slice(prefix.length).trim() : null);
  let r;
  if ((r = after('deadly demise ')) !== null) return { DeadlyDemise: r.toUpperCase() };
  if ((r = after('feel no pain ')) !== null && Number.isInteger(Number(r.replace(/\+$/, '')))) return { FeelNoPain: Number(r.replace(/\+$/, '')) };
  if ((r = after('firing deck ')) !== null && Number.isInteger(Number(r))) return { FiringDeck: Number(r) };
  if ((r = after('scouts ')) !== null && Number.isInteger(Number(r.replace(/["”″]$/, '')))) return { Scouts: Number(r.replace(/["”″]$/, '')) };
  return { Other: name.trim() };
}

function weaponSpec(w, p, grouped) {
  return {
    kind: w.kind === 'melee' ? 'Melee' : 'Ranged',
    range: p.range,
    profile: {
      name: p.name,
      attacks: parseDice(p.attacks) ?? flat(1),
      skill: p.skill,
      strength: p.strength,
      ap: p.ap,
      damage: parseDice(p.damage) ?? flat(1),
      abilities: p.keywords.map(parseWeaponAbility).filter((a) => a !== null),
    },
    profile_group: grouped ? w.name : null,
  };
}

/**
 * army_def::to_pack. `abilities` is the enrichment index: lower-cased name →
 * entry, built from the faction's and `_core` abilities.json.
 */
export function toPack(def, cat, abilities) {
  const byId = new Map();
  for (const a of abilities.values()) if (typeof a.ability_id === 'string') byId.set(a.ability_id, a);
  const units = [];
  for (const u of def.units) {
    const sheet = cat.units.find((s) => s.id === u.datasheet_id);
    if (!sheet) continue;
    const rowIds = (m) => {
      const row = sheet.models.find((r) => r.name === m.row);
      return m.weapon_ids?.length ? m.weapon_ids : (row?.default_weapon_ids ?? []);
    };
    const weaponIds = [...new Set(u.models.flatMap(rowIds))].sort();
    const weapons = [];
    const index = new Map();
    for (const id of weaponIds) {
      const w = cat.weapons.find((x) => x.id === id);
      if (!w) continue;
      const grouped = w.profiles.length > 1;
      for (const p of w.profiles) {
        if (!index.has(id)) index.set(id, []);
        index.get(id).push(weapons.length);
        weapons.push(weaponSpec(w, p, grouped));
      }
    }
    const models = u.models
      .filter((m) => m.count > 0)
      .map((m) => {
        const row = sheet.models.find((r) => r.name === m.row);
        const idx = [...new Set(rowIds(m).flatMap((id) => index.get(id) ?? []))].sort((a, b) => a - b);
        return { name: m.row, count: m.count, stats: row?.stats ?? null, weapons: idx };
      });
    const core_rules = [];
    const specs = [];
    for (const id of sheet.ability_ids) {
      const name = cat.ability_names[id] ?? id;
      const rule = parseCoreRule(name);
      if (typeof rule === 'object' && 'Other' in rule) {
        const effect = byId.get(id)?.effect ?? abilities.get(name.toLowerCase())?.effect ?? null;
        specs.push({ name, effect });
      } else core_rules.push(rule);
    }
    if (sheet.can_lead_ids.length && !core_rules.includes('Leader') && !core_rules.includes('Support')) core_rules.push('Leader');
    let points = unitPoints(sheet, modelCount(u));
    const enh = u.enhancement_id ? cat.enhancements.find((e) => e.id === u.enhancement_id) : null;
    if (enh) {
      points += enh.cost;
      specs.push({ name: enh.name, effect: (enh.ability_id && byId.get(enh.ability_id)?.effect) ?? null });
    }
    const keywords = [...sheet.keywords];
    for (const k of cat.faction_keywords) if (!hasKw(keywords, k)) keywords.push(k);
    keywords.push(`Faction: ${cat.name}`);
    if (u.warlord) keywords.push('Warlord');
    units.push({
      name: sheet.name,
      points,
      keywords,
      stats: sheet.stats,
      models,
      weapons,
      core_rules,
      abilities: specs,
      can_lead: sheet.can_lead_ids.map((id) => cat.units.find((s) => s.id === id)?.name).filter((n) => n !== undefined),
      transport_capacity: sheet.transport_capacity,
    });
  }
  let army_rules = [...abilities.values()].filter((a) => a.ability_type === 'faction' && typeof a.name === 'string').map((a) => ({ name: a.name, effect: a.effect ?? null }));
  for (const d of def.detachment_ids.map((id) => cat.detachments.find((x) => x.id === id)).filter(Boolean)) {
    const a = d.rule_ability_id ? byId.get(d.rule_ability_id) : null;
    if (a && typeof a.name === 'string') army_rules.push({ name: a.name, effect: a.effect ?? null });
  }
  army_rules.sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
  army_rules = army_rules.filter((r, i) => i === 0 || army_rules[i - 1].name !== r.name);
  const detachment = def.detachment_ids
    .map((id) => cat.detachments.find((x) => x.id === id)?.name)
    .filter(Boolean)
    .join(' + ');
  return { name: def.name, faction: cat.name, detachment: detachment || null, points: totalPoints(def, cat), units, army_rules };
}
