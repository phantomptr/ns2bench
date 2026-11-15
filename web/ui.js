// Small shared interface pieces, like ps5upload's: toasts for the result of an action, and a
// confirm dialog before anything that interrupts the console or removes something.
import { t } from './i18n.js';

const $ = (s) => document.querySelector(s);

// ---------------------------------------------------------------- toasts
// Bottom right, at most three, newest last. Errors stay until closed; the rest fade.
const TONES = { info: 5000, ok: 4000, warn: 8000, error: 0 };
export function toast(text, tone = 'info', action = null) {
  const box = $('#toasts');
  if (!box || !text) return;
  const el = document.createElement('div');
  el.className = `toast ${tone}`;
  el.setAttribute('role', tone === 'error' ? 'alert' : 'status');
  const msg = Object.assign(document.createElement('span'), { textContent: text });
  el.append(msg);
  if (action) {
    const b = Object.assign(document.createElement('button'), { type: 'button', textContent: action.label });
    b.onclick = () => { action.run(); el.remove(); };
    el.append(b);
  }
  const close = Object.assign(document.createElement('button'), { type: 'button', className: 'x', textContent: '×' });
  close.setAttribute('aria-label', t('Close'));
  close.onclick = () => el.remove();
  el.append(close);
  box.append(el);
  while (box.children.length > 3) box.firstElementChild.remove();
  const ms = TONES[tone] ?? 5000;
  if (ms) setTimeout(() => el.remove(), ms);
}

// ---------------------------------------------------------------- confirm
// ask({title, body, confirm, destructive}) -> Promise<boolean>. A destructive one starts on Cancel.
export function ask({ title, body = '', confirm = t('Continue'), destructive = false }) {
  const d = $('#confirm');
  if (!d) return Promise.resolve(true);
  $('#confirm-title').textContent = title;
  $('#confirm-body').textContent = body;
  const ok = $('#confirm-ok'), cancel = $('#confirm-cancel');
  ok.textContent = confirm;
  ok.className = destructive ? 'danger' : 'primary';
  return new Promise((resolve) => {
    const done = (v) => { d.close(); resolve(v); };
    ok.onclick = () => done(true);
    cancel.onclick = () => done(false);
    d.oncancel = (e) => { e.preventDefault(); done(false); };
    d.showModal();
    (destructive ? cancel : ok).focus();
  });
}
