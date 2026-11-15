// Desktop notifications, like ps5upload: the engine flags log lines worth one
// (Bridge connects/disconnects, picture lost/back, tests, clips, agents driving,
// app updates); this decides whether to show them. In the desktop app they use the
// system notification centre (Tauri plugin); in a browser, the Notification API.
import { t } from './i18n.js';

const $ = (s) => document.querySelector(s);
const store = {
  get(k, d) { try { const v = localStorage.getItem(`ns2bench.notify.${k}`); return v === null ? d : v === 'true'; } catch { return d; } },
  set(k, v) { try { localStorage.setItem(`ns2bench.notify.${k}`, String(v)); } catch { /* private mode */ } },
};
export const KINDS = {
  bridge: ['Bridge connected', 'Bridge disconnected'],
  video: ['No picture', 'Picture is back'],
  tasks: ['Test passed', 'Test failed', 'Clip saved'],
  agent: ['An agent is driving'],
  updates: ['Update available'],
};
const kindOf = (title) => Object.keys(KINDS).find((k) => KINDS[k].includes(title));
const plugin = () => window.__TAURI__?.notification;

async function allowed(ask) {
  const p = plugin();
  if (p) {
    if (await p.isPermissionGranted()) return true;
    return ask ? (await p.requestPermission()) === 'granted' : false;
  }
  if (!('Notification' in window)) return false;
  if (Notification.permission === 'granted') return true;
  return ask && Notification.permission !== 'denied' ? (await Notification.requestPermission()) === 'granted' : false;
}

export async function notify(title, body, { force = false } = {}) {
  if (!force) {
    const kind = kindOf(title);
    if (!store.get('on', true) || (kind && !store.get(kind, kind !== 'agent'))) return;
    if (store.get('background', true) && document.hasFocus() && document.visibilityState === 'visible') return;
  }
  if (!(await allowed(false))) return;
  const shown = t(title);
  try {
    const p = plugin();
    if (p) p.sendNotification({ title: `NS2Bench · ${shown}`, body });
    else new Notification(`NS2Bench · ${shown}`, { body, icon: '/static/favicon.svg', tag: title });
  } catch { /* the system refused; nothing else to do */ }
}

/** Called for every live log entry. */
export function fromLog(e) {
  if (e.notify) notify(e.notify, e.hint ? `${e.text}\n${e.hint}` : e.text);
}

function status() {
  const el = $('#notify-status');
  if (!el) return;
  const p = plugin();
  const state = p ? null : !('Notification' in window) ? 'unsupported' : Notification.permission;
  Promise.resolve(p ? p.isPermissionGranted().then((g) => (g ? 'granted' : 'default')) : state).then((s) => {
    el.textContent = s === 'granted' ? t('Notifications are allowed on this computer.')
      : s === 'denied' ? t('Notifications are blocked for NS2Bench. Allow them in your system or browser settings.')
        : s === 'unsupported' ? t('This browser can’t show notifications.')
          : t('Click Send a test notification to allow them.');
  });
}

for (const [id, key, d] of [['#notify-on', 'on', true], ['#notify-background', 'background', true], ['#notify-bridge', 'bridge', true],
  ['#notify-video', 'video', true], ['#notify-tasks', 'tasks', true], ['#notify-agent', 'agent', false], ['#notify-updates', 'updates', true]]) {
  const box = $(id);
  if (!box) continue;
  box.checked = store.get(key, d);
  box.addEventListener('change', async () => { store.set(key, box.checked); if (box.checked) { await allowed(true); status(); } });
}
$('#notify-test')?.addEventListener('click', async () => {
  await allowed(true);
  await notify('Notifications are on', t('NS2Bench will tell you when the Bridge or the picture drops, when tests and clips finish, and when an update is ready.'), { force: true });
  status();
});
status();
