// Tiny DOM helpers; no framework.

type Child = Node | string | null | undefined | false;

export function h<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  attrs: Record<string, unknown> | null = null,
  ...children: (Child | Child[])[]
): HTMLElementTagNameMap[K] {
  const el = document.createElement(tag);
  if (attrs) {
    for (const [k, v] of Object.entries(attrs)) {
      if (v === null || v === undefined || v === false) continue;
      if (k === 'class') el.className = String(v);
      else if (k === 'style') el.setAttribute('style', String(v));
      else if (k.startsWith('on') && typeof v === 'function') el.addEventListener(k.slice(2).toLowerCase(), v as EventListener);
      else if (k === 'disabled' || k === 'checked' || k === 'selected' || k === 'hidden') (el as unknown as Record<string, unknown>)[k] = v;
      else if (k === 'value') (el as HTMLInputElement).value = String(v);
      else el.setAttribute(k, String(v));
    }
  }
  append(el, children);
  return el;
}

function append(el: Node, children: (Child | Child[])[]): void {
  for (const c of children) {
    if (Array.isArray(c)) append(el, c);
    else if (c === null || c === undefined || c === false) continue;
    else el.appendChild(typeof c === 'string' ? document.createTextNode(c) : c);
  }
}

export function clear(el: Element): void {
  while (el.firstChild) el.removeChild(el.firstChild);
}

export function button(label: string, onClick: () => void, opts: { class?: string; disabled?: boolean; title?: string } = {}): HTMLButtonElement {
  return h('button', { class: opts.class ?? '', disabled: opts.disabled, title: opts.title, onClick }, label);
}

export function select(options: { value: string; label: string }[], value: string, onChange: (v: string) => void): HTMLSelectElement {
  const s = h('select', { onChange: (e: Event) => onChange((e.target as HTMLSelectElement).value) });
  for (const o of options) s.appendChild(h('option', { value: o.value, selected: o.value === value }, o.label));
  s.value = value;
  return s;
}

let toastTimer: number | undefined;

export function toast(message: string, kind: 'error' | 'info' = 'error'): void {
  const el = document.getElementById('toast');
  if (!el) return;
  el.textContent = message;
  el.className = kind;
  el.hidden = false;
  if (toastTimer) window.clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => (el.hidden = true), kind === 'error' ? 6000 : 3000);
}

export const PLAYER_COLOURS = ['#4f8cff', '#ff7a45'];
export const PLAYER_COLOURS_SOFT = ['rgba(79,140,255,0.14)', 'rgba(255,122,69,0.14)'];
