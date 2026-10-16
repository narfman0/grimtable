import { api, errorText } from './api';
import type { App } from './main';
import { button, h } from './ui';

export function renderLogin(app: App): HTMLElement {
  let mode: 'login' | 'register' = 'login';
  const username = h('input', { type: 'text', autocomplete: 'username', placeholder: 'username', maxlength: 32 });
  const password = h('input', { type: 'password', autocomplete: 'current-password', placeholder: 'password' });
  const error = h('div', { class: 'formerror', hidden: true });
  const submit = button('Sign in', () => void go(), { class: 'primary' });
  const toggle = button('Create an account instead', () => {
    mode = mode === 'login' ? 'register' : 'login';
    submit.textContent = mode === 'login' ? 'Sign in' : 'Register';
    toggle.textContent = mode === 'login' ? 'Create an account instead' : 'I already have an account';
    password.autocomplete = mode === 'login' ? 'current-password' : 'new-password';
    showError(null);
  });

  function showError(msg: string | null): void {
    error.hidden = msg === null;
    error.textContent = msg ?? '';
  }

  async function go(): Promise<void> {
    const u = username.value.trim();
    const p = password.value;
    if (!u || !p) {
      showError('username and password are both needed');
      return;
    }
    submit.disabled = true;
    try {
      app.user = mode === 'login' ? await api.login(u, p) : await api.register(u, p);
      if (mode === 'register') {
        // Registration may or may not start a session; make sure one exists.
        try {
          await api.me();
        } catch {
          app.user = await api.login(u, p);
        }
      }
      app.navigate('#/');
    } catch (e) {
      showError(errorText(e));
    } finally {
      submit.disabled = false;
    }
  }

  const form = h(
    'form',
    {
      class: 'card loginform',
      onSubmit: (e: Event) => {
        e.preventDefault();
        void go();
      },
    },
    h('label', null, 'Username', username),
    h('label', null, 'Password', password),
    error,
    h('div', { class: 'row' }, submit, toggle),
  );
  submit.type = 'submit';
  toggle.type = 'button';

  return h(
    'div',
    { class: 'setup login' },
    h('h1', null, 'grimtable'),
    h('p', { class: 'muted' }, 'Rules-enforced battles in the browser. Sign in to build armies and play online, or play a local hotseat game without an account.'),
    form,
    h('p', null, button('Local hotseat', () => app.navigate('#/hotseat'))),
  );
}
