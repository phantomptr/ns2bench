import { apiFetch, authURL, setToken } from './client.js';
import { t } from './i18n.js';
import { inboxAdd, notify } from './notify.js';
import { ask, toast } from './ui.js';
import { prefsReady, savePrefsNow } from './prefs.js';
import { cardChanged } from './consoles.js';
const $ = (s) => document.querySelector(s);
const dialog = $('#settings');
let networkRevision = 0, inputRevision = 0;
$('#network-settings').addEventListener('change', () => { networkRevision++; });
$('#input-settings').addEventListener('input', () => { inputRevision++; });
$('#input-settings').addEventListener('change', () => { inputRevision++; });
// Results of an action show as a toast next to where people are looking, not in a line at the top.
const message = (s, tone = 'info') => { if (s) toast(t(s), tone); };
async function api(path, body) {
  const r = await apiFetch(path, body === undefined ? {} : { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) });
  if (!r.ok) throw new Error(await r.text());
  return r.json();
}
function run(fn) { return async (event) => { event?.preventDefault(); try { await fn(event); } catch (e) { message(e.message || String(e), 'error'); } }; }
$('#btn-settings').onclick = run(async () => { document.activeElement?.blur(); dialog.showModal(); await refresh(); });
$('#settings-close').onclick = () => dialog.close();
dialog.addEventListener('keydown', (e) => e.stopPropagation());
async function showTab(tab) {
  dialog.querySelectorAll('[data-tab]').forEach((t) => t.setAttribute('aria-pressed', String(t.dataset.tab === tab)));
  dialog.querySelectorAll('[data-pane]').forEach((p) => { p.hidden = p.dataset.pane !== tab; });
  if (tab === 'network') await network();
  if (tab === 'consoles') await consolesPane();
  // Opening Updates looks again (at most every 10 minutes), instead of showing an old answer.
  if (tab === 'updates' && invoke && Date.now() - lastAttempt > 600000) updateAction('check').then(automaticUpdates).catch(() => {});
  if (tab === 'devices') serialPorts();
}
for (const b of dialog.querySelectorAll('[data-tab]')) b.onclick = run(() => showTab(b.dataset.tab));
// Other parts of the page (status pills, the getting-started card) open a tab directly.
document.addEventListener('ns2bench:settings', run(async (e) => {
  if (!dialog.open) { document.activeElement?.blur(); dialog.showModal(); }
  await showTab(e.detail || 'devices'); await refresh();
}));
async function about() {
  const s = await api('/api/status');
  $('#about-version').textContent = s.version ? `v${s.version}` : '';
}
async function refresh() {
  const results = await Promise.allSettled([input(), network(), integrations(), about(), power()]);
  const errors = results.filter((r) => r.status === 'rejected').map((r) => r.reason.message);
  message(errors.join(' · '), 'error');
}
function inputFields() {
  const kind = $('#input-kind').value;
  $('#input-network').hidden = kind !== 'hidd';
  $('#input-serial').hidden = !['serial', 'ch9329'].includes(kind);
  $('#input-help').textContent = ({ hidd: 'Any compatible Linux USB gadget device running NS2Bench Bridge. Use its management IP or hostname.',
    serial: 'Pico: UART0 GP0/GP1 + GND to a 3.3 V USB-UART adapter; 921600 baud. Console uses the Pico USB port.',
    ch9329: 'Keyboard and mouse only; controller buttons and sticks are unavailable. Match the module’s configured baud (usually 9600).', none: 'Capture and other tools remain available without input.' })[kind];
}
$('#input-kind').onchange = () => { $('#input-baud').value = $('#input-kind').value === 'serial' ? '921600' : '9600'; inputFields(); };
async function input() {
  const revision = inputRevision;
  const { settings: s } = await api('/api/input');
  if (revision !== inputRevision) return;
  $('#input-kind').value = s.backend; $('#input-host').value = s.host || ''; $('#input-port').value = s.port || 7631;
  $('#input-path').value = s.serial_port || ''; $('#input-baud').value = String(s.baud || 921600); $('#input-token-file').value = s.token_file || '';
  inputFields();
}
$('#input-settings').onsubmit = run(async () => {
  const kind = $('#input-kind').value;
  const s = { backend: kind };
  if (kind === 'hidd') {
    // A pasted "192.168.79.2:7631" or "http://host:7631/" goes into the two fields.
    const m = $('#input-host').value.trim().replace(/^[a-z]+:\/\//i, '').replace(/\/.*$/, '').match(/^([^:\s]+):(\d{1,5})$/);
    if (m) { $('#input-host').value = m[1]; $('#input-port').value = m[2]; }
    Object.assign(s, { host: $('#input-host').value.trim(), port: Number($('#input-port').value), token_file: $('#input-token-file').value.trim() || null });
  }
  if (['serial', 'ch9329'].includes(kind)) Object.assign(s, { serial_port: $('#input-path').value.trim(), baud: Number($('#input-baud').value) });
  await api('/api/input', s);
  try { localStorage.setItem('ns2bench.viewOnly', JSON.stringify(kind === 'none')); } catch { /* private mode */ }
  await checkInput(kind);
});
function networkFields() {
  const topology = $('#network-topology').value, direct = topology === 'direct', shared = topology === 'shared';
  for (const key of ['dhcp', 'dns', 'connectivity']) {
    const el = $(`#network-${key}`);
    // shared: DNS and the test page only; handing out addresses would fight the home router
    const on = direct || (shared && key !== 'dhcp');
    el.disabled = !on; if (!on) el.checked = false;
  }
  $('#shared-setup').hidden = !shared;
  if (shared) { $('#network-help').textContent = t('Consoles stay on your home network with manual settings (gateway: their own address, DNS: this computer). NS2Bench answers only their connection test and refuses every other name, including all of Nintendo. Set it up below.'); return; }
  $('#network-help').textContent = direct
    ? 'Requires the local ns2bench-netd service on macOS or Linux and a dedicated console interface. Interface/address changes require configuring that interface and restarting netd.'
    : $('#network-topology').value === 'router'
      ? 'Use a router/VLAN/SSID with internet access blocked for the console, including IPv6. The computer may share that LAN. Router DHCP supplies addresses; NS2Bench cannot enforce an external router’s firewall.'
      : 'No local network services. USB input and HDMI capture still work. NS2Bench cannot verify an unmanaged console network is offline.';
}
$('#network-topology').onchange = networkFields;
async function network(loadForm = true) {
  const revision = networkRevision;
  const n = await api('/api/network');
  if (loadForm && revision === networkRevision && !n.error) {
    $('#network-topology').value = n.controls.topology || 'direct';
    for (const key of ['dhcp', 'dns', 'connectivity']) $(`#network-${key}`).checked = n.controls[key] === true;
    networkFields();
  }
  const actual = n.daemon;
  const where = n.helper ? `${t('Network helper')} ${n.helper}` : t('Local service');
  const onOff = (v) => (v ? t('on') : t('off'));
  $('#network-status').textContent = n.error ? `${n.error}. ${t('Check that the helper is running and that network.helper.token_file matches its control token.')}` : n.online
    ? `${where}: ${actual.interface} · ${actual.address} · ${actual.interface_ready && actual.forwarding_off ? t('interface ready, forwarding off') : t('waiting for the interface (isolation check not ready)')}. DHCP ${onOff(actual.dhcp)}, DNS ${onOff(actual.dns)}, ${t('connectivity page')} ${onOff(actual.connectivity)}.`
    : n.helper ? `${where}: ${t('the helper answered, but its service is not running yet.')}` : t('The offline console network is optional and not running on this computer. Capture and USB input work without it. To give the console a LAN without internet, set up ns2bench-netd (see the setup guide).');
  // Plain status colours, and controls only when a service can apply them.
  const running = !n.error && n.online;
  $('#network-status').className = `status ${n.error ? 'bad' : running ? 'ok' : 'info'}`;
  // Controls stay editable: saved settings apply as soon as the service starts.
  $('#network-leases').textContent = Object.entries(n.leases).map(([mac, lease]) => `${mac}  ${lease.ip}  ${lease.hostname || ''}`).join('\n') || 'No DHCP leases recorded';
  renderShared(n, loadForm && revision === networkRevision);
  if (loadForm) loadInterfaces(n.shared?.interface).catch((e) => message(e.message));
}

// ---------------------------------------------------------------- shared home network
// docs/design/shared-network.md. Every value from the engine goes in as text, never as HTML.
let sharedEdited = false, ifacePicked = false, adapters = [];
$('#shared-consoles').addEventListener('input', () => { sharedEdited = true; autoAdapter(); sharedReady(); });
for (const c of document.querySelectorAll('.shared-check')) c.addEventListener('change', sharedReady);
$('#shared-iface').addEventListener('change', () => { ifacePicked = true; sharedReady(); });
const sharedList = () => $('#shared-consoles').value.split(/[\s,;]+/).map((s) => s.trim()).filter(Boolean);
// IPv4 text -> number (null if not an address); is `ip` on adapter `a`'s network?
const ipNum = (s) => { const p = s.split('.'); return p.length === 4 && p.every((x) => /^\d{1,3}$/.test(x) && +x < 256) ? ((+p[0] << 24) | (+p[1] << 16) | (+p[2] << 8) | +p[3]) >>> 0 : null; };
const onAdapter = (ip, a) => { const n = ipNum(ip), m = a.prefix ? (0xffffffff << (32 - a.prefix)) >>> 0 : 0; return n !== null && ((n & m) >>> 0) === ((ipNum(a.address) & m) >>> 0); };
const adapterLabel = (a) => `${a.name}${a.label ? ` · ${a.label}` : ''} · ${a.address}/${a.prefix}`;
// Until the person picks one, the adapter follows the consoles: the one on their network.
function autoAdapter() {
  if (ifacePicked) return;
  const first = sharedList()[0], match = first && adapters.find((a) => onAdapter(first, a));
  if (match) $('#shared-iface').value = match.name;
}
function sharedReady() {
  const checked = [...document.querySelectorAll('.shared-check')].every((c) => c.checked);
  const a = adapters.find((x) => x.name === $('#shared-iface').value);
  const off = a ? sharedList().filter((ip) => !onAdapter(ip, a)) : [];
  const right = off[0] && adapters.find((x) => onAdapter(off[0], x));
  const box = $('#shared-mismatch');
  box.hidden = !off.length;
  if (off.length) {
    box.textContent = `${off.join(', ')}: ${t('not on the network of the adapter you picked')} (${a.name}, ${a.address}/${a.prefix}). `
      + (right ? `${t('Pick this adapter instead:')} ${adapterLabel(right)}` : t('None of this computer’s adapters is on that network. Connect it to the consoles’ network first.'));
  }
  $('#shared-save').disabled = !checked || !$('#shared-iface').value || !sharedList().length || off.length > 0;
}
async function loadInterfaces(saved) {
  const r = await api('/api/network/interfaces');
  adapters = r.interfaces || [];
  const sel = $('#shared-iface');
  // the person's own pick, else the saved adapter, else the one on the consoles' network
  const keep = (ifacePicked && sel.value) || saved || '';
  sel.replaceChildren();
  if (!r.supported) { sel.append(new Option(t('The network helper runs on macOS and Linux'), '')); sel.disabled = true; return sharedReady(); }
  if (!r.interfaces.length) sel.append(new Option(t('No network adapter with a private address'), ''));
  for (const i of r.interfaces) {
    sel.append(new Option(adapterLabel(i), i.name));
  }
  sel.value = r.interfaces.some((i) => i.name === keep) ? keep : (r.interfaces[0]?.name || '');
  if (!keep) autoAdapter();
  sharedReady();
}
const ago = (t0) => {
  if (!t0) return t('never');
  const s = Math.max(0, Date.now() / 1000 - t0);
  return s < 90 ? `${Math.round(s)} s` : s < 5400 ? `${Math.round(s / 60)} min` : `${Math.round(s / 3600)} h`;
};
function renderShared(n, loadForm) {
  const s = n.shared || {};
  if (loadForm && !sharedEdited && s.consoles?.length) $('#shared-consoles').value = s.consoles.join(', ');
  const shared = n.controls?.topology === 'shared' && s.consoles?.length;
  $('#shared-settings-box').hidden = !shared;
  if (!shared) return;
  $('#shared-settings').replaceChildren(...s.settings.map((row) => {
    const tr = document.createElement('tr');
    for (const k of ['console', 'ip', 'mask', 'gateway', 'dns']) { const td = document.createElement('td'); td.textContent = row[k]; tr.append(td); }
    return tr;
  }));
  const rows = s.watch?.consoles;
  const items = rows ? rows.map((c) => {
    const li = document.createElement('li');
    const seen = `${t('last asked NS2Bench')} ${ago(c.last_dns)} · ${t('connection test')} ${ago(c.last_ctest)} · ${c.refused || 0} ${t('names refused')}`;
    const text = {
      protected: [`${c.console}: ${t('protected')} · ${seen}`, 'ok'],
      warning: [`${c.console}: ${t('on your network but not using NS2Bench. It may be back on automatic settings and online: check its gateway and DNS.')}`, 'bad'],
      waiting: [`${c.console}: ${t('on your network, waiting for its first lookup')}`, ''],
      away: [`${c.console}: ${t('not on the network right now')} · ${seen}`, ''],
      'helper-down': [`${c.console}: ${t('on your network, but the network helper isn’t running, so NS2Bench can’t protect or check it. Start the helper (Settings → Install).')}`, 'bad'],
      unknown: [`${c.console}: ${t('can’t tell whether it’s on the network (this computer couldn’t check)')} · ${seen}`, ''],
    }[c.state] || [c.console, ''];
    li.textContent = text[0]; if (text[1]) li.className = text[1];
    return li;
  }) : [Object.assign(document.createElement('li'), { textContent: t('The leak watch starts when the network helper runs in shared mode (it looks every 30 seconds).') })];
  $('#shared-watch').replaceChildren(...items);
}
$('#shared-save').onclick = run(async () => {
  const r = await api('/api/network/shared', { interface: $('#shared-iface').value, consoles: sharedList() });
  sharedEdited = false;
  const box = $('#shared-result');
  box.hidden = false; box.className = 'status ok';
  box.textContent = invoke
    ? `${t('Saved. Restart the network helper so it answers on')} ${r.interface} (${r.address}). ${t('Then enter the settings below on each console and run its connection test.')}`
    : `${t('Saved. Restart the network helper so it answers on')} ${r.interface} (${r.address}): launchctl kickstart -k gui/$(id -u)/com.phantomptr.ns2bench.netd (macOS) · sudo systemctl restart ns2bench-netd (Linux). ${t('Then enter the settings below on each console and run its connection test.')}`;
  $('#shared-restart').hidden = !invoke;
  await network(false);
  $('#network-topology').value = 'shared'; networkFields();
});
$('#shared-restart').onclick = run(async () => {
  const r = await component('netd', 'restart');
  const box = $('#shared-result'); box.hidden = false; box.className = 'status ok'; box.textContent = r.message;
  setTimeout(() => network(false).catch((e) => message(e.message)), 1500);
});
$('#network-settings').onsubmit = run(async () => {
  const s = { topology: $('#network-topology').value };
  for (const key of ['dhcp', 'dns', 'connectivity']) s[key] = $(`#network-${key}`).checked;
  await api('/api/network', s); message('Network controls saved. The network service applies them within one second. Internet blocking stays enforced.');
  setTimeout(() => network(false).catch((e) => message(e.message)), 1500);
});
async function integrations() {
  const i = await api('/api/integrations');
  $('#api-info').textContent = `Engine: ${i.api_url} · ${i.token_required ? 'Bearer token required' : 'No API token configured; keep the listener on loopback.'}`;
  $('#api-example').textContent = `curl ${i.token_required ? '-H "Authorization: Bearer <token>" ' : ''}${JSON.stringify(i.api_url + '/api/status')}\n\ncurl ${i.token_required ? '-H "Authorization: Bearer <token>" ' : ''}-X POST -H 'Content-Type: application/json' -d '{"buttons":["A"],"hold_ms":80}' ${JSON.stringify(i.api_url + '/api/press')}`;
  $('#mcp-config').textContent = JSON.stringify(i.mcp, null, 2);
  $('#mcp-count').textContent = `${i.tools.length} ${t('tools an agent can use. The same list is in the API and the command line.')}`;
  const url = i.mcp_url || `${i.api_url}/mcp`;
  $('#mcp-url').textContent = url;
  $('#mcp-token-note').hidden = !i.token_required;
  const commands = {
    'claude-code': `claude mcp add --scope user --transport http ns2bench ${url}`,
    codex: `codex mcp add ns2bench --url ${url}`,
    opencode: `opencode mcp add ns2bench --url ${url}`,
  };
  // Wrapped between words only (never inside "--scope" or the address); copying gives the same text.
  const words = (el, text) => el.replaceChildren(...text.split(' ').flatMap((w, i) => [...(i ? [' '] : []), Object.assign(document.createElement('span'), { className: 'w', textContent: w })]));
  for (const card of dialog.querySelectorAll('.agent')) words(card.querySelector('.agent-cmd'), commands[card.dataset.agent]);
  agents();
  $('#mcp-tools').replaceChildren(...i.tools.map((t) => { const p = document.createElement('p'); const b = document.createElement('b'); b.textContent = t.name; p.append(b, document.createTextNode(` — ${t.description || ''}`)); return p; }));
}
$('#api-token-apply').onclick = () => setToken($('#api-token').value);
// ---------------------------------------------------------------- agents (MCP)
// The desktop app asks each agent's own CLI whether it has NS2Bench and can add or remove it;
// in a browser the cards show the commands to run.
function showAgent(st) {
  const card = dialog.querySelector(`.agent[data-agent="${st.agent}"]`);
  if (!card) return;
  const badge = card.querySelector('.agent-badge'), add = card.querySelector('.agent-add'), rm = card.querySelector('.agent-remove'), note = card.querySelector('.agent-note');
  const here = st.configured && st.saved_url === st.expected_url;
  const state = !st.found ? 'missing' : st.read_failed ? 'unknown' : here ? 'added' : st.configured ? 'moved' : 'none';
  badge.dataset.state = state;
  badge.textContent = { missing: t('Not installed'), unknown: t('Couldn’t check'), added: t('Added'), moved: t('Other address'), none: t('Not added') }[state];
  add.hidden = !st.found || here;
  add.textContent = st.configured ? t('Update') : t('Add');
  rm.hidden = !(st.configured && st.can_remove);
  note.textContent = !st.found ? t('This agent isn’t installed on this computer, or NS2Bench couldn’t find it. You can still run the command below where it is installed.')
    : state === 'moved' ? t('This agent has an NS2Bench entry with a different address. Update points it at this engine.')
      : st.configured && !st.can_remove ? t('To remove it, delete the ns2bench entry under "mcp" in OpenCode’s config file.') : '';
}
let agentsBusy = false;
async function agents() {
  const cards = [...dialog.querySelectorAll('.agent')];
  if (!invoke) {
    for (const c of cards) {
      c.querySelector('.agent-badge').hidden = true;
      c.querySelector('.agent-note').textContent = t('Run the command below in a terminal. The NS2Bench desktop app can add it for you with one click.');
    }
    $('#agents-check').hidden = true;
    return;
  }
  if (agentsBusy) return;
  agentsBusy = true; $('#agents-check').disabled = true;
  for (const c of cards) { const b = c.querySelector('.agent-badge'); b.dataset.state = 'unknown'; b.textContent = t('Checking…'); }
  try { (await invoke('agent_action', { action: 'status' })).forEach(showAgent); }
  catch (e) { message(String(e)); }
  finally { agentsBusy = false; $('#agents-check').disabled = false; }
}
$('#agents-check').onclick = run(agents);
const AGENT_ERRORS = { cli_not_found: 'This agent isn’t installed on this computer.', timed_out: 'The agent took too long to answer. Try again.',
  token_required: 'This engine asks for an API token, which one-click setup can’t add. Add the address in your agent with an Authorization: Bearer header (see the API tab).' };
for (const card of dialog.querySelectorAll('.agent')) {
  for (const [sel, action] of [['.agent-add', 'add'], ['.agent-remove', 'remove']]) {
    card.querySelector(sel).onclick = run(async (e) => {
      const b = e.currentTarget;
      if (action === 'remove' && !(await ask({ title: t('Remove NS2Bench from this agent?'), body: t('The agent can no longer see or drive the console. You can add it again any time.'), confirm: t('Remove'), destructive: true }))) return;
      b.disabled = true;
      try { showAgent(await invoke('agent_action', { action, agent: card.dataset.agent })); message(action === 'add' ? t('Added. Start a new session in your agent to load it.') : t('Removed.')); }
      catch (err) { message(t(AGENT_ERRORS[String(err)] || String(err))); }
      finally { b.disabled = false; }
    });
  }
}
for (const b of dialog.querySelectorAll('.copyblock .copy')) {
  b.onclick = run(async () => {
    await navigator.clipboard.writeText(b.parentElement.querySelector('code').textContent);
    b.classList.add('done'); setTimeout(() => b.classList.remove('done'), 1200);
  });
}
for (const b of dialog.querySelectorAll('[data-sub]')) {
  b.onclick = () => {
    dialog.querySelectorAll('[data-sub]').forEach((x) => x.setAttribute('aria-pressed', String(x === b)));
    dialog.querySelectorAll('[data-subpane]').forEach((p) => { p.hidden = p.dataset.subpane !== b.dataset.sub; });
  };
}
for (const [button, content] of [['#api-copy', '#api-example'], ['#mcp-copy', '#mcp-config']]) $(button).onclick = run(async () => { await navigator.clipboard.writeText($(content).textContent); message('Copied.'); });
// Storage can be blocked (private mode, strict settings): never let that stop the UI.
const ls = {
  get(k) { try { return localStorage.getItem(k); } catch { return null; } },
  set(k, v) { try { localStorage.setItem(k, v); } catch { /* blocked */ } },
};
const pref = (key) => ls.get(`ns2bench.update.${key}`) === 'true';
let update = null, busy = false, lastActivity = Date.now(), failedAutoInstall = false, lastAttempt = 0, failedDownload = false;
for (const event of ['keydown', 'pointerdown', 'pointermove']) addEventListener(event, () => { lastActivity = Date.now(); }, { passive: true });
const invoke = window.__TAURI__?.core?.invoke;
// Defaults: find, download in the background, install when NS2Bench closes.
// (A choice saved before these existed keeps its value; "download" was off by default then.)
const quitInstall = () => invoke?.('update_action', { action: $('#update-auto-quit').checked ? 'quit_install_on' : 'quit_install_off' }).catch(() => {});
for (const [id, key, fallback] of [['#update-auto-check', 'check', true], ['#update-auto-download', 'download2', true], ['#update-auto-quit', 'quit', true], ['#update-auto-install', 'install', false]]) {
  const el = $(id); el.checked = ls.get(`ns2bench.update.${key}`) === null ? fallback : pref(key);
  el.onchange = () => { ls.set(`ns2bench.update.${key}`, String(el.checked)); failedAutoInstall = false; if (id === '#update-auto-quit') quitInstall(); };
}
prefsReady.then(() => {
  for (const [id, key, fallback] of [['#update-auto-check', 'check', true], ['#update-auto-download', 'download2', true], ['#update-auto-quit', 'quit', true], ['#update-auto-install', 'install', false]]) $(id).checked = ls.get(`ns2bench.update.${key}`) === null ? fallback : pref(key);
  quitInstall();
});
function renderUpdate() {
  const ready = update?.downloaded && $('#update-auto-quit').checked ? ` · ${t('installs when you quit NS2Bench')}` : '';
  $('#update-status').textContent = update ? `${t('Installed')} ${update.current_version}${update.latest_version ? ` · ${t('Latest')} ${update.latest_version}` : ''} · ${t(update.message)}${ready}` : 'Updates are available in the NS2Bench desktop app.';
  renderNotes($('#update-notes'), update?.notes || '');
  $('#update-check').disabled = busy || !invoke;
  $('#update-download').disabled = busy || !update?.available || update.downloaded || update.manual;
  $('#update-install').disabled = busy || !update?.downloaded;
  for (const id of ['#update-auto-check', '#update-auto-download', '#update-auto-quit', '#update-auto-install']) $(id).disabled = !invoke;
}
// quiet: a background check or download (at start, every 6 h). Its error stays in the Updates
// pane instead of a toast that sticks (an offline start, a .deb install).
async function updateAction(action, quiet = false) {
  if (busy || !invoke) return;
  if (action === 'check') { lastAttempt = Date.now(); failedDownload = false; }
  busy = true; renderUpdate(); $('#update-status').textContent = `${action === 'download' ? 'Downloading and verifying update…' : action === 'install' ? 'Preparing installation…' : 'Checking update…'}`;
  // While downloading, show how far it got (the download itself holds the updater, so it is asked separately).
  const MB = (n) => (n / 1048576).toFixed(1);
  let poll = 0;
  if (action === 'download') {
    $('#update-progress').hidden = false; $('#update-bar').removeAttribute('value'); $('#update-bytes').textContent = '';
    poll = setInterval(async () => {
      const p = await invoke('update_action', { action: 'progress' }).catch(() => null);
      if (!p?.received) return;
      if (p.total) { $('#update-bar').max = p.total; $('#update-bar').value = p.received; }
      $('#update-bytes').textContent = p.total ? `${MB(p.received)} / ${MB(p.total)} MB · ${Math.floor((p.received / p.total) * 100)}%` : `${MB(p.received)} MB`;
    }, 400);
  }
  try { update = await invoke('update_action', { action }); if (action === 'check') ls.set('ns2bench.update.lastCheck', String(Date.now())); }
  catch (e) { if (action === 'download') failedDownload = true; if (!quiet) message(String(e), 'error'); if (update) update.message = String(e); }
  finally {
    clearInterval(poll);
    $('#update-progress').hidden = true;
    busy = false; renderUpdate();
    if (action === 'download' && update?.downloaded) {
      toast($('#update-auto-quit').checked ? `NS2Bench ${update.latest_version} ${t('is ready. It installs when you quit NS2Bench.')}` : `NS2Bench ${update.latest_version} ${t('is ready to install.')}`,
        'ok', { label: t('Restart now'), run: () => $('#update-install').click() });
    }
  }
}
$('#update-check').onclick = run(() => updateAction('check'));
$('#update-download').onclick = run(() => updateAction('download'));
$('#update-install').onclick = run(async () => (await ask({ title: t('Install the update now?'), body: t('NS2Bench closes, installs the new version and opens again. Held buttons are released and agents lose the connection until it is back.'), confirm: t('Install and restart') })) && updateAction('install'));
async function automaticUpdates() {
  if (!invoke || busy) return;
  await prefsReady;
  // Once at start, then every 6 hours (a result is not kept across restarts, so start always asks).
  if ($('#update-auto-check').checked && (!lastAttempt || Date.now() - lastAttempt > 6 * 3600000)) await updateAction('check', true);
  $('#btn-settings').classList.toggle('badge', !!update?.available);
  $('#btn-update').classList.toggle('badge', !!update?.available);
  $('#btn-update').title = update?.available ? t('Update available') : t('Updates');
  if (update?.available && update.latest_version && ls.get('ns2bench.update.notified') !== update.latest_version) {
    ls.set('ns2bench.update.notified', update.latest_version);
    inboxAdd('Update available', `NS2Bench ${update.latest_version} ${t('is ready to install from Settings → Updates.')}`);
    notify('Update available', `NS2Bench ${update.latest_version} ${t('is ready to install from Settings → Updates.')}`);
  }
  $('#btn-settings').title = update?.available ? 'Update available' : 'Connections, appearance, API, MCP and updates';
  // (a system package, like a Linux .deb, isn't downloaded: it shows where to get the new one)
  if (update?.available && !update.manual && !update.downloaded && !failedDownload && $('#update-auto-download').checked) await updateAction('download', true);
  if (update?.downloaded && $('#update-auto-install').checked && !failedAutoInstall && Date.now() - lastActivity > 60000 && !dialog.open) {
    const s = await api('/api/status');
    const gpActive = [...(navigator.getGamepads?.() || [])].some((p) => p && (p.buttons.some((b) => b.pressed) || p.axes.some((a) => Math.abs(a) > 0.12)));
    if (!gpActive && !s.active_operations && !s.input.agent_busy && s.input.pad?.[0] === 0 && s.input.pad?.[1] === 8 && s.input.pad.slice(2).every((v) => v === 128) && s.input.kbd?.every((v) => v === 0)) { failedAutoInstall = true; await updateAction('install'); }
  }
}
renderUpdate();
if (invoke) updateAction('status').then(automaticUpdates).catch((e) => message(String(e)));
setInterval(() => automaticUpdates().catch((e) => message(e.message)), 60000);
// (one message when the engine goes away, not one every 5 s)
let networkFailing = false;
setInterval(() => { if (dialog.open && !$('[data-pane="network"]').hidden) network(false).then(() => { networkFailing = false; }).catch((e) => { if (!networkFailing) message(e.message); networkFailing = true; }); }, 5000);

// Command line tool: put the ns2bench CLI that ships in the app on the PATH.
let cli = null;
function renderCli(note) {
  const st = $('#cli-status'), inst = $('#cli-install'), rm = $('#cli-uninstall');
  inst.disabled = !invoke || !cli?.available || !!cli?.system_package; rm.hidden = !cli?.ours || !!cli?.system_package; rm.disabled = !invoke;
  inst.textContent = cli?.ours ? t('Reinstall') : t('Install command line tool');
  if (note) { st.textContent = note; return; }
  if (!invoke) { st.textContent = t('The command line tool installs from the NS2Bench desktop app.'); return; }
  if (!cli) return;
  const dir = (cli.target || '').replace(/[\\/]ns2bench(\.exe)?$/, '');
  if (!cli.available) st.textContent = t('This copy of NS2Bench has no command line tool. Download it from the GitHub release.');
  else if (cli.system_package) st.textContent = `${t('Installed by the package:')} ${cli.path || '/usr/bin/ns2bench'}`;
  else if (cli.ours) st.textContent = `${t('Installed:')} ${cli.path} · ${cli.dir_on_path ? t('Open a new terminal and run ns2bench --help.') : `${t('Add this folder to your PATH, or sign out and back in:')} ${dir}`}`;
  else if (cli.installed) st.textContent = `${t('Another ns2bench is on your PATH:')} ${cli.path}`;
  else st.textContent = t('Not installed.');
}
async function cliAction(action) {
  if (!invoke) return;
  renderCli(t(action === 'install' ? 'Installing…' : action === 'uninstall' ? 'Removing…' : 'Checking…'));
  try { cli = await invoke('cli_action', { action }); renderCli(); }
  catch (e) { const m = String(e); renderCli(m === 'Cancelled' ? undefined : m); }
}
$('#cli-install').onclick = run(() => cliAction('install'));
$('#cli-uninstall').onclick = run(async () => (await ask({ title: t('Remove the ns2bench command?'), body: t('Scripts and terminals that use ns2bench stop finding it. The app keeps working.'), confirm: t('Remove'), destructive: true })) && cliAction('uninstall'));
renderCli();
if (invoke) cliAction('status');

// Install: Pico firmware, Bridge board over SSH, network helper (desktop app only).
const component = (name, action, extra = {}) => invoke('component_action', { component: name, action, ...extra });
const say = (id, text) => { $(id).textContent = text; };
let installBusy = false;
async function installStep(buttons, id, busyText, fn) {
  if (!invoke || installBusy) return;
  installBusy = true; for (const b of buttons) $(b).disabled = true; say(id, busyText);
  try { await fn(); } catch (e) { say(id, String(e)); } finally { installBusy = false; renderInstall(); }
}
let netd = null;
function renderInstall() {
  const off = !invoke || installBusy;
  $('#pico-flash').disabled = off; $('#board-probe').disabled = off || !$('#board-host').value.trim(); $('#board-install').disabled = $('#board-probe').disabled;
  $('#netd-install').disabled = off || netd?.supported === false; $('#netd-uninstall').hidden = !netd?.installed; $('#netd-uninstall').disabled = off;
  if (!invoke) for (const id of ['#pico-status', '#board-status', '#netd-status']) say(id, t('Available in the NS2Bench desktop app.'));
}
function renderNetd() {
  if (!netd) return;
  if (!netd.supported) { say('#netd-status', t('The network helper runs on macOS and Linux. On Windows, run it on a Linux board or PC (see the setup guide).')); return; }
  $('#netd-install').textContent = netd.installed ? t('Update helper') : t('Install helper');
  if (!netd.installed) { say('#netd-status', t('Not installed.')); return; }
  const v = netd.installed_version && netd.installed_version !== 'older' ? netd.installed_version : t('an older version');
  say('#netd-status', `${netd.running ? t('Running') : t('Installed, not running')} · ${v}${netd.config ? ` · ${netd.config}` : ''}${netd.app_config && netd.config && netd.config !== netd.app_config ? ` · ${t('The app uses a different settings file:')} ${netd.app_config}` : ''}`);
}
$('#board-host').value = ls.get('ns2bench.board.host') || '';
$('#board-host').oninput = () => { ls.set('ns2bench.board.host', $('#board-host').value.trim()); renderInstall(); };
$('#pico-flash').onclick = run(async () => (await ask({ title: t('Flash the Pico?'), body: t('The Pico in BOOTSEL mode gets the NS2Bench Bridge firmware. Anything else on it is replaced.'), confirm: t('Flash Pico') })) && installStep(['#pico-flash'], '#pico-status', t('Looking for a Pico in BOOTSEL mode… Hold BOOTSEL while you plug it in.'), async () => {
  say('#pico-status', (await component('pico', 'flash')).message);
}));
$('#board-probe').onclick = run(() => installStep(['#board-probe', '#board-install'], '#board-status', t('Connecting…'), async () => {
  const b = await component('board', 'probe', { host: $('#board-host').value.trim() });
  say('#board-status', `${b.arch} · ${b.installed ? `${t('Installed:')} ${b.installed}` : t('NS2Bench Bridge is not installed.')}${b.supported ? '' : ` · ${t('This board is not supported.')}`}`);
}));
$('#board-install').onclick = run(async () => (await ask({ title: t('Install on the board?'), body: t('NS2Bench uploads the Bridge to the board and restarts it. The console sees the controller disconnect for a few seconds, and held buttons are released. The current files are kept as a backup on the board.'), confirm: t('Install') })) && installStep(['#board-probe', '#board-install'], '#board-status', t('Installing… The Bridge restarts, so held buttons are released.'), async () => {
  const r = await component('board', 'install', { host: $('#board-host').value.trim() });
  say('#board-status', r.message); $('#board-log').textContent = r.log || ''; $('#board-log').hidden = !r.log;
}));
$('#netd-install').onclick = run(() => installStep(['#netd-install', '#netd-uninstall'], '#netd-status', t('Installing…'), async () => {
  const r = await component('netd', 'install'); netd = r.status; renderNetd(); $('#netd-status').textContent += ` · ${r.message}`;
}));
$('#netd-uninstall').onclick = run(async () => (await ask({ title: t('Remove the network helper?'), body: t('The console’s offline network stops: no more local addresses or connectivity test. Capture and USB input keep working.'), confirm: t('Remove'), destructive: true })) && installStep(['#netd-install', '#netd-uninstall'], '#netd-status', t('Removing…'), async () => {
  const r = await component('netd', 'uninstall'); netd = r.status; renderNetd();
}));
renderInstall();
if (invoke) {
  component('netd', 'status').then((v) => { netd = v; renderNetd(); renderInstall(); }).catch((e) => say('#netd-status', String(e)));
  component('pico', 'status').then((v) => { if (v.drive) say('#pico-status', `${t('Pico ready:')} ${v.drive}`); }).catch(() => {});
}

// Help: bug report bundle, diagnostics, and a prefilled GitHub issue.
const helpNote = (t) => { $('#help-note').textContent = t; };
// Check everything: the engine's doctor, shown as a checklist with what to fix.
const CHECK_ICON = { ok: '✓', warn: '!', fail: '✕', skip: '–' };
$('#help-doctor').onclick = run(async () => {
  const btn = $('#help-doctor'), list = $('#help-checks');
  btn.disabled = true; btn.textContent = t('Checking…'); list.replaceChildren();
  try {
    const r = await api('/api/doctor', { probe: false });
    list.replaceChildren(...(r.checks || []).map((c) => {
      const li = document.createElement('li');
      li.className = `check ${c.status}`;
      li.innerHTML = '<span class="ci" aria-hidden="true"></span><span class="cb"><b></b><span></span></span>';
      li.querySelector('.ci').textContent = CHECK_ICON[c.status] || '·';
      li.querySelector('b').textContent = c.name;
      li.querySelector('.cb > span').textContent = c.detail || '';
      if (c.hint && c.status !== 'ok') { const h = document.createElement('small'); h.textContent = c.hint; li.querySelector('.cb').append(h); }
      return li;
    }));
  } catch (e) { list.replaceChildren(Object.assign(document.createElement('li'), { textContent: String(e) })); }
  finally { btn.disabled = false; btn.textContent = t('Run the check'); }
});
$('#help-bundle').onclick = () => { location.href = authURL('/api/bundle'); helpNote('Bug report saved to your Downloads folder.'); };
// The desktop app can open the folder with the logs and crash reports.
$('#help-logs').hidden = !invoke;
$('#help-logs').onclick = run(async () => { await invoke('open_logs'); });
$('#help-diag').onclick = run(async () => {
  const r = await apiFetch('/api/diagnostics');
  await navigator.clipboard.writeText(await r.text());
  helpNote('Diagnostics copied. Paste them into the issue.');
});
$('#help-issue').onclick = run(async () => {
  const s = await api('/api/status').catch(() => ({}));
  const ua = navigator.userAgent;
  const os = /Mac/.test(ua) ? 'macOS' : /Windows/.test(ua) ? 'Windows' : /Linux/.test(ua) ? 'Linux' : 'unknown OS';
  const q = new URLSearchParams({ template: 'bug_report.yml', versions: `${s.version || '?'}, ${os}` });
  window.open(`https://github.com/phantomptr/ns2bench/issues/new?${q}`, '_blank', 'noopener');
});

// ---------------------------------------------------------------- Devices: check the connection, suggest serial ports
// After Apply, watch the status for a few seconds and say right here whether it worked.
let checkRun = 0;
async function checkInput(kind) {
  const run = ++checkRun;                                   // a newer Apply replaces this check
  const line = $('#input-check');
  line.hidden = false; line.className = 'status info';
  if (kind === 'none') { line.textContent = t('View only: the picture, sound and tools work; nothing presses buttons.'); line.className = 'status ok'; return; }
  line.textContent = t('Saved. Connecting…');
  let s = null;
  for (let i = 0; i < 16; i++) {
    await new Promise((r) => setTimeout(r, 500));
    s = (await api('/api/status').catch(() => null))?.input;
    if (run !== checkRun) return;
    if (s?.connected && s.usb === 'configured') break;
  }
  if (s?.connected && s.usb === 'configured') { line.className = 'status ok'; line.textContent = `${t('Connected. The console sees the controller.')} (${Math.max(1, Math.round(s.rtt_ms ?? 1))} ms)`; }
  else if (s?.connected) { line.className = 'status warn'; line.textContent = t('Connected to the Bridge, but the console hasn’t accepted it yet. Wake the console and check the USB cable to the console.'); }
  else { line.className = 'status bad'; line.textContent = `${s?.detail || t('Not connected yet.')} ${t('Check the device is powered (the console must be awake), its address or port, and the token file. It keeps trying in the background.')}`; }
}
async function serialPorts() {
  const r = await api('/api/serial_ports').catch(() => null);
  $('#serial-ports').replaceChildren(...(r?.ports || []).map((p) => Object.assign(document.createElement('option'), { value: p })));
}

// ---------------------------------------------------------------- Devices: calibrate presses
$('#calibrate').onclick = run(async () => {
  if (!(await ask({ title: t('Calibrate presses?'), body: t('Go to the HOME menu first. NS2Bench presses Right and Left about 25 times over half a minute and watches the screen. Don’t drive while it runs.'), confirm: t('Calibrate') }))) return;
  const b = $('#calibrate'), note = $('#calibrate-note');
  b.disabled = true; note.textContent = t('Calibrating… about 30 s.');
  try {
    const r = await api('/api/calibrate', {});
    note.textContent = `${t('Done. Shortest press that always works:')} ${r.min_press_ms} ms · ${t('the console reacts in about')} ${Math.round(r.latency_ms_median ?? 0)} ms.`;
    toast(t('Calibration saved.'), 'ok');
  } catch (e) { note.textContent = String(e.message || e); }
  finally { b.disabled = false; }
});

// ---------------------------------------------------------------- Help: what to do about each check
const CHECK_ACTION = {
  video: ['Video settings', () => { dialog.close(); $('#pill-video').click(); }],
  'console picture': ['Video settings', () => { dialog.close(); $('#pill-video').click(); }],
  'NS2Bench Bridge': ['Input settings', () => showTab('devices')],
  'console USB': ['Input settings', () => showTab('devices')],
  'console input': ['Input settings', () => showTab('devices')],
  'agent input': ['Allow agents', () => { dialog.close(); $('#manual-only').scrollIntoView({ block: 'center' }); }],
  calibration: ['Calibrate', () => showTab('devices').then(() => $('#calibrate').scrollIntoView({ block: 'center' }))],
  'console network': ['Network settings', () => showTab('network')],
  'console network isolation': ['Network settings', () => showTab('network')],
  'power and wake': ['Power and wake', () => showTab('devices').then(() => $('#power-settings').scrollIntoView({ block: 'center' }))],
  audio: ['Turn sound on', () => { dialog.close(); if ($('#btn-sound').getAttribute('aria-pressed') !== 'true') $('#btn-sound').click(); }],
};
new MutationObserver(() => {
  for (const li of $('#help-checks').children) {
    if (li.dataset.acted || !/\b(warn|fail)\b/.test(li.className)) continue;
    li.dataset.acted = '1';
    const a = CHECK_ACTION[li.querySelector('b')?.textContent];
    if (!a) continue;
    const btn = Object.assign(document.createElement('button'), { type: 'button', className: 'check-act', textContent: t(a[0]) });
    btn.onclick = run(a[1]);
    li.append(btn);
  }
}).observe($('#help-checks'), { childList: true });

// ---------------------------------------------------------------- Help and Appearance: start over
$('#help-welcome').onclick = () => { dialog.close(); document.dispatchEvent(new CustomEvent('ns2bench:welcome')); };
$('#help-shortcuts').onclick = () => { dialog.close(); $('#shortcuts').showModal(); };
$('#prefs-reset').onclick = run(async () => {
  if (!(await ask({ title: t('Reset appearance and layout?'), body: t('Theme, text size, language, accessibility, layout and log filters go back to their defaults. Devices, network and API settings are kept.'), confirm: t('Reset'), destructive: true }))) return;
  const keep = /^ns2bench\.(update|board|inbox|notify|token|viewOnly|welcomeHidden|drove|lastSeenVersion)/;
  try { for (const k of Object.keys(localStorage)) if (k.startsWith('ns2bench.') && !keep.test(k)) localStorage.removeItem(k); } catch { /* storage blocked */ }
  await savePrefsNow();
  location.reload();
});

// ---------------------------------------------------------------- What's new (after an update) and release notes
// A little Markdown: headings, bullets (nested), bold, code and links. Text is set, never parsed as HTML.
export function renderNotes(el, md) {
  el.replaceChildren();
  let list = null, sub = null;
  const inline = (parent, text) => {
    for (const part of text.split(/(\*\*[^*]+\*\*|`[^`]+`|\[[^\]]+\]\([^)]+\))/)) {
      if (!part) continue;
      let node;
      if (part.startsWith('**')) node = Object.assign(document.createElement('b'), { textContent: part.slice(2, -2) });
      else if (part.startsWith('`')) node = Object.assign(document.createElement('code'), { textContent: part.slice(1, -1) });
      else if (part.startsWith('[')) {
        const [, label, href] = part.match(/^\[([^\]]+)\]\(([^)]+)\)$/);
        node = /^https:\/\//.test(href) ? Object.assign(document.createElement('a'), { textContent: label, href, target: '_blank', rel: 'noopener' }) : document.createTextNode(label);
      } else node = document.createTextNode(part);
      parent.append(node);
    }
  };
  for (const line of md.split('\n')) {
    const h = line.match(/^#{1,3}\s+(.*)/), b = line.match(/^(\s*)[-*]\s+(.*)/);
    if (h) { list = sub = null; const e = document.createElement('h4'); inline(e, h[1]); el.append(e); }
    else if (b) {
      const li = document.createElement('li'); inline(li, b[2]);
      if (b[1].length >= 2 && list) { if (!sub) { sub = document.createElement('ul'); list.lastElementChild?.append(sub); } sub.append(li); }
      else { if (!list) { list = document.createElement('ul'); el.append(list); } sub = null; list.append(li); }
    } else if (line.trim()) { list = sub = null; const p = document.createElement('p'); inline(p, line.trim()); el.append(p); }
  }
}
// The changelog sections newer than `since` and up to `upto` (the running version).
const newer = (a, b) => { const x = a.split('.').map(Number), y = b.split('.').map(Number); for (let i = 0; i < 3; i++) if ((x[i] || 0) !== (y[i] || 0)) return (x[i] || 0) > (y[i] || 0); return false; };
async function whatsNew(since, upto) {
  const r = await apiFetch('/api/changelog'); if (!r.ok) return '';
  const md = await r.text();
  const out = [];
  for (const sec of md.split(/^## /m).slice(1)) {
    const v = sec.split('\n', 1)[0].trim();
    if (!/^\d+\.\d+\.\d+$/.test(v) || newer(v, upto) || (since && !newer(v, since))) continue;
    out.push(`### ${v}\n${sec.slice(sec.indexOf('\n') + 1).trim()}`);
    if (!since) break;
  }
  return out.slice(0, 5).join('\n\n');
}
async function showWhatsNew(since, upto) {
  const md = await whatsNew(since, upto);
  if (!md) return;
  renderNotes($('#whatsnew-body'), md);
  $('#whatsnew-title').textContent = `${t('What’s new in NS2Bench')} ${upto}`;
  $('#whatsnew').showModal();
}
$('#whatsnew-open').onclick = run(async () => { const s = await api('/api/status'); await showWhatsNew('', s.version); });
// After an update (never on a first install): once, and a line in the bell.
prefsReady.then(() => api('/api/status')).then(async (s) => {
  if (!s.version) return;
  const seen = ls.get('ns2bench.lastSeenVersion');
  ls.set('ns2bench.lastSeenVersion', s.version);
  if (!seen || seen === s.version || !newer(s.version, seen)) return;
  inboxAdd('Update installed', `${t('NS2Bench was updated to')} ${s.version}. ${t('Open Settings → Updates → What’s new to see what changed.')}`);
  await showWhatsNew(seen, s.version);
}).catch(() => {});

// ---------------------------------------------------------------- Devices: power and wake
async function power() {
  const p = await api('/api/power');
  const s = p.settings;
  $('#power-cec').checked = !!s.cec; $('#power-cec-port').value = s.cec_port || ''; $('#power-cec-address').value = s.cec_address || 4;
  $('#power-command').value = s.button_command || ''; $('#power-wake-button').checked = !!s.wake_with_button;
  $('#power-command').placeholder = s.button_command === null && s.button_command_set ? t('(set; only visible in the NS2Bench app window)') : 'curl -X POST http://homeassistant.local:8123/api/webhook/switch-power';
  $('#power-cec-tool').textContent = p.cec_tool ? `${t('CEC tool found:')} ${p.cec_tool}` : t('No CEC tool on this computer yet: install libcec (macOS: brew install libcec; Windows: the Pulse-Eight installer; Linux: the libcec or cec-utils package).');
  powerButtons(p);
}
function powerButtons(p) {
  $('#power-sleep').disabled = !p.can_sleep;
  $('#power-button').disabled = !p.can_press_button;
  $('#power-cec-fields').hidden = !$('#power-cec').checked;
}
$('#power-cec').onchange = () => { $('#power-cec-fields').hidden = !$('#power-cec').checked; };
$('#power-settings').onsubmit = run(async () => {
  await api('/api/power/settings', { cec: $('#power-cec').checked, cec_port: $('#power-cec-port').value.trim(),
    cec_address: Number($('#power-cec-address').value) || 4, button_command: $('#power-command').value.trim(), wake_with_button: $('#power-wake-button').checked });
  toast(t('Power settings saved.'), 'ok');
  await power();
});
function powerResult(r) {
  const el = $('#power-result');
  el.hidden = false; el.className = `status ${r.ok ? 'ok' : 'bad'}`;
  const lines = [r.message || r.detail || (r.ok ? t('Done.') : t('Failed.'))];
  for (const s of r.steps || []) lines.push(`${s.ok ? '✓' : '✕'} ${s.step}${s.detail ? `: ${s.detail}` : ''}`);
  el.textContent = lines.join('\n');
}
export async function wakeConsole() {
  toast(t('Waking the console…'));
  const r = await api('/api/power', { action: 'wake' });
  toast(t(r.message), r.ok ? 'ok' : 'warn');
  return r;
}
$('#power-wake').onclick = run(async () => { $('#power-wake').disabled = true; try { powerResult(await api('/api/power', { action: 'wake' })); } finally { $('#power-wake').disabled = false; } });
$('#power-sleep').onclick = run(async () => {
  if (!(await ask({ title: t('Put the console to sleep?'), body: t('NS2Bench sends standby over HDMI-CEC. Whatever is running is suspended, and a Bridge powered from the console turns off until it wakes.'), confirm: t('Sleep') }))) return;
  powerResult(await api('/api/power', { action: 'sleep' }));
});
$('#power-button').onclick = run(async () => {
  if (!(await ask({ title: t('Press the power button?'), body: t('NS2Bench runs your power-button command. If the console is on, a short press puts it to sleep; if it is off or crashed, it starts.'), confirm: t('Press') }))) return;
  powerResult(await api('/api/power', { action: 'button' }));
});
document.addEventListener('ns2bench:wake', run(wakeConsole));

// ---------------------------------------------------------------- Appearance: the window and the tray (app only)
{
  const mac = /Mac/.test(navigator.platform || navigator.userAgent);
  const close = $('#tray-close'), min = $('#tray-min');
  const get = (k, d) => { const v = ls.get(`ns2bench.tray.${k}`); return v === null ? d : v === 'true'; };
  async function tray() {
    if (!invoke) return;
    try {
      const r = await invoke('tray_configure', { minimize: !mac && min.checked, close: close.checked,
        labels: { show: t('Show NS2Bench'), quit: t('Quit NS2Bench'), tooltip: 'NS2Bench' } });
      $('#tray-note').textContent = r.available ? '' : t('This system has no tray (on Linux, install libayatana-appindicator3), so closing the window quits NS2Bench.');
    } catch (e) { $('#tray-note').textContent = String(e); }
  }
  if (invoke) {
    $('#window-group').hidden = false;
    $('#tray-min-row').hidden = mac;
    const linux = /Linux/.test(navigator.platform || navigator.userAgent) && !/Android/.test(navigator.userAgent);
    prefsReady.then(() => { close.checked = get('close', !linux); min.checked = get('minimize', false); return tray(); });
    close.onchange = () => { ls.set('ns2bench.tray.close', String(close.checked)); tray(); };
    min.onchange = () => { ls.set('ns2bench.tray.minimize', String(min.checked)); tray(); };
  }
}

// ---------------------------------------------------------------- consoles (several on one bench)
// docs/design/multi-console.md. Engine text goes in as text, never as HTML.
const consoleApi = (body) => api('/api/consoles', body);
// capture cards, and which console uses each (a card serves one console)
async function captureCards() {
  const r = await apiFetch('/api/video', { headers: { 'X-NS2Bench-Console': '' } }).then((x) => x.json()).catch(() => ({}));
  return (r.devices || []).filter((d) => d.kind === 'video');
}
function cardSelect(cards, used, mine) {
  const sel = document.createElement('select');
  sel.append(new Option(t('Pick automatically'), ''));
  for (const d of cards) {
    const by = used.get(d.id);
    const o = new Option(by && d.id !== mine ? `${d.name || d.id} (${t('used by')} ${by})` : (d.name || d.id), d.id);
    o.disabled = !!by && d.id !== mine;
    sel.append(o);
  }
  sel.value = mine || '';
  return sel;
}
function field(label, input) {
  const l = Object.assign(document.createElement('label'), { className: 'field' });
  l.append(document.createTextNode(label), input);
  return l;
}
async function consolesPane() {
  const r = await api('/api/consoles');
  const cards = await captureCards();
  const used = new Map(r.consoles.filter((c) => c.video?.device).map((c) => [c.video.device, c.name]));
  const models = r.models || [];
  const modelSelect = (value) => {
    const s = document.createElement('select');
    for (const m of models) s.append(new Option(`${m.name} (${m.mode === 'pro' ? t('Pro Controller') : t('standard controller')})`, m.id));
    s.value = value; return s;
  };
  $('#console-add-model').replaceChildren(...modelSelect('switch1').children);
  $('#console-add-model').value = 'switch1';
  $('#console-add-card').replaceChildren(...cardSelect(cards, used, '').children);
  // a new console picks its own card ("automatic" would find another console's)
  $('#console-add-card').options[0].text = t('Pick its capture card');
  $('#console-add-card').options[0].disabled = true;
  $('#consoles-list').replaceChildren(...r.consoles.map((c) => {
    const box = Object.assign(document.createElement('div'), { className: 'group console-card' });
    box.style.setProperty('--tab-color', /^#[0-9a-f]{6}$/i.test(c.color) ? c.color : '#888888');
    const title = Object.assign(document.createElement('h3'), { textContent: c.name });
    if (c.controller_here) title.append(Object.assign(document.createElement('span'), { className: 'controller-badge', textContent: ` 🎮 ${t('controller here')}` }));
    const name = Object.assign(document.createElement('input'), { value: c.name, maxLength: 40 });
    const model = modelSelect(c.model);
    const color = Object.assign(document.createElement('input'), { type: 'color', value: /^#[0-9a-f]{6}$/i.test(c.color) ? c.color : '#888888' });
    const ip = Object.assign(document.createElement('input'), { value: c.network_ip || '', placeholder: '192.168.86.251', spellcheck: false });
    const card = cardSelect(cards, used, c.video?.device || '');
    const row = Object.assign(document.createElement('div'), { className: 'field-row wrap' });
    const save = Object.assign(document.createElement('button'), { type: 'button', className: 'primary', textContent: t('Save') });
    save.onclick = run(async () => {
      try {
        // the card first: if it's refused (another console uses it), nothing else changed either
        if ((card.value || '') !== (c.video?.device || '')) {
          await apiFetch('/api/video', { method: 'POST', headers: { 'Content-Type': 'application/json', 'X-NS2Bench-Console': c.id }, body: JSON.stringify({ device: card.value || null }) })
            .then(async (x) => { if (!x.ok) throw new Error(await x.text()); });
          cardChanged(c.id);
        }
        await consoleApi({ action: 'update', id: c.id, name: name.value.trim(), model: model.value, color: color.value, network_ip: ip.value.trim() });
        message(`${name.value.trim()}: ${t('saved')}`);
      } finally {
        await consolesPane();            // the form shows what was really saved
      }
    });
    row.append(save);
    if (!c.controller_here) {
      const here = Object.assign(document.createElement('button'), { type: 'button', textContent: t('The controller is plugged in here') });
      here.onclick = run(async () => { await consoleApi({ action: 'controller', id: c.id }); await consolesPane(); });
      row.append(here);
    }
    if (r.consoles.length > 1) {
      const rm = Object.assign(document.createElement('button'), { type: 'button', className: 'danger', textContent: t('Remove') });
      rm.onclick = run(async () => {
        if (!(await ask({ title: `${t('Remove')} ${c.name}?`, body: t('Its capture stops and its settings are removed. Clips and test reports stay.'), confirm: t('Remove'), destructive: true }))) return;
        await consoleApi({ action: 'remove', id: c.id }); await consolesPane();
      });
      row.append(rm);
    }
    box.append(title, field(t('Name'), name), field(t('Model'), model), field(t('Capture card'), card), field(t('Network address (optional)'), ip), field(t('Colour'), color), row);
    return box;
  }));
}
$('#console-add').onclick = run(async () => {
  const name = $('#console-add-name').value.trim();
  if (!name) throw new Error(t('Give the console a name.'));
  if (!$('#console-add-card').value) throw new Error(t('Pick its capture card'));
  const body = { action: 'add', name, model: $('#console-add-model').value, color: $('#console-add-color').value,
    network_ip: $('#console-add-ip').value.trim() || null, video: $('#console-add-card').value ? { device: $('#console-add-card').value } : {} };
  await consoleApi(body);
  $('#console-add-name').value = ''; $('#console-add-ip').value = '';
  message(`${name}: ${t('added. Its picture starts in a moment.')}`);
  await consolesPane();
});
