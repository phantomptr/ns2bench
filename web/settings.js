import { apiFetch, authURL, setToken } from './client.js';
import { t } from './i18n.js';
import { notify } from './notify.js';
const $ = (s) => document.querySelector(s);
const dialog = $('#settings');
let networkRevision = 0, inputRevision = 0;
$('#network-settings').addEventListener('change', () => { networkRevision++; });
$('#input-settings').addEventListener('input', () => { inputRevision++; });
$('#input-settings').addEventListener('change', () => { inputRevision++; });
const message = (s) => { $('#settings-message').textContent = s; };
async function api(path, body) {
  const r = await apiFetch(path, body === undefined ? {} : { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) });
  if (!r.ok) throw new Error(await r.text());
  return r.json();
}
function run(fn) { return async (event) => { event?.preventDefault(); try { await fn(event); } catch (e) { message(e.message || String(e)); } }; }
$('#btn-settings').onclick = run(async () => { document.activeElement?.blur(); dialog.showModal(); await refresh(); });
$('#settings-close').onclick = () => dialog.close();
dialog.addEventListener('keydown', (e) => e.stopPropagation());
async function showTab(tab) {
  dialog.querySelectorAll('[data-tab]').forEach((t) => t.setAttribute('aria-pressed', String(t.dataset.tab === tab)));
  dialog.querySelectorAll('[data-pane]').forEach((p) => { p.hidden = p.dataset.pane !== tab; });
  message(''); if (tab === 'network') await network();
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
  const results = await Promise.allSettled([input(), network(), integrations(), about()]);
  const errors = results.filter((r) => r.status === 'rejected').map((r) => r.reason.message);
  message(errors.join(' · '));
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
  if (kind === 'hidd') Object.assign(s, { host: $('#input-host').value.trim(), port: Number($('#input-port').value), token_file: $('#input-token-file').value.trim() || null });
  if (['serial', 'ch9329'].includes(kind)) Object.assign(s, { serial_port: $('#input-path').value.trim(), baud: Number($('#input-baud').value) });
  await api('/api/input', s); message('Input settings saved. The previous connection was closed; Bridge readiness appears in the main toolbar.');
  try { localStorage.setItem('ns2bench.viewOnly', JSON.stringify(kind === 'none')); } catch { /* private mode */ }
});
function networkFields() {
  const direct = $('#network-topology').value === 'direct';
  for (const key of ['dhcp', 'dns', 'connectivity']) { const el = $(`#network-${key}`); el.disabled = !direct; if (!direct) el.checked = false; }
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
}
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
  $('#mcp-count').textContent = `${i.tools.length} available MCP tools`;
  $('#mcp-tools').replaceChildren(...i.tools.map((t) => { const p = document.createElement('p'); const b = document.createElement('b'); b.textContent = t.name; p.append(b, document.createTextNode(` — ${t.description || ''}`)); return p; }));
}
$('#api-token-apply').onclick = () => setToken($('#api-token').value);
for (const [button, content] of [['#api-copy', '#api-example'], ['#mcp-copy', '#mcp-config']]) $(button).onclick = run(async () => { await navigator.clipboard.writeText($(content).textContent); message('Copied.'); });
// Storage can be blocked (private mode, strict settings): never let that stop the UI.
const ls = {
  get(k) { try { return localStorage.getItem(k); } catch { return null; } },
  set(k, v) { try { localStorage.setItem(k, v); } catch { /* blocked */ } },
};
const pref = (key) => ls.get(`ns2bench.update.${key}`) === 'true';
let update = null, busy = false, lastActivity = Date.now(), failedAutoInstall = false, lastAttempt = 0, failedDownload = false;
for (const event of ['keydown', 'pointerdown', 'pointermove']) addEventListener(event, () => { lastActivity = Date.now(); }, { passive: true });
for (const [id, key, fallback] of [['#update-auto-check', 'check', true], ['#update-auto-download', 'download', false], ['#update-auto-install', 'install', false]]) {
  const el = $(id); el.checked = ls.get(`ns2bench.update.${key}`) === null ? fallback : pref(key);
  el.onchange = () => { ls.set(`ns2bench.update.${key}`, String(el.checked)); failedAutoInstall = false; };
}
const invoke = window.__TAURI__?.core?.invoke;
function renderUpdate() {
  $('#update-status').textContent = update ? `Installed ${update.current_version}${update.latest_version ? ` · Latest ${update.latest_version}` : ''} · ${update.message}` : 'Updates are available in the NS2Bench desktop app.';
  $('#update-notes').textContent = update?.notes || '';
  $('#update-check').disabled = busy || !invoke;
  $('#update-download').disabled = busy || !update?.available || update.downloaded || update.manual;
  $('#update-install').disabled = busy || !update?.downloaded;
  for (const id of ['#update-auto-check', '#update-auto-download', '#update-auto-install']) $(id).disabled = !invoke;
}
async function updateAction(action) {
  if (busy || !invoke) return;
  if (action === 'check') { lastAttempt = Date.now(); failedDownload = false; }
  busy = true; renderUpdate(); $('#update-status').textContent = `${action === 'download' ? 'Downloading and verifying update…' : action === 'install' ? 'Preparing installation…' : 'Checking update…'}`;
  try { update = await invoke('update_action', { action }); if (action === 'check') ls.set('ns2bench.update.lastCheck', String(Date.now())); }
  catch (e) { if (action === 'download') failedDownload = true; message(String(e)); if (update) update.message = String(e); }
  finally { busy = false; renderUpdate(); }
}
$('#update-check').onclick = run(() => updateAction('check'));
$('#update-download').onclick = run(() => updateAction('download'));
$('#update-install').onclick = run(() => updateAction('install'));
async function automaticUpdates() {
  if (!invoke || busy) return;
  if ($('#update-auto-check').checked && Date.now() - lastAttempt > 900000 && Date.now() - Number(ls.get('ns2bench.update.lastCheck') || 0) > 86400000) await updateAction('check');
  $('#btn-settings').classList.toggle('badge', !!update?.available);
  if (update?.available && update.latest_version && ls.get('ns2bench.update.notified') !== update.latest_version) {
    ls.set('ns2bench.update.notified', update.latest_version);
    notify('Update available', `NS2Bench ${update.latest_version} ${t('is ready to install from Settings → Updates.')}`);
  }
  $('#btn-settings').title = update?.available ? 'Update available' : 'Connections, appearance, API, MCP and updates';
  if (update?.available && !update.downloaded && !failedDownload && $('#update-auto-download').checked) await updateAction('download');
  if (update?.downloaded && $('#update-auto-install').checked && !failedAutoInstall && Date.now() - lastActivity > 60000 && !dialog.open) {
    const s = await api('/api/status');
    const gpActive = [...(navigator.getGamepads?.() || [])].some((p) => p && (p.buttons.some((b) => b.pressed) || p.axes.some((a) => Math.abs(a) > 0.12)));
    if (!gpActive && !s.active_operations && !s.input.agent_busy && s.input.pad?.[0] === 0 && s.input.pad?.[1] === 8 && s.input.pad.slice(2).every((v) => v === 128) && s.input.kbd?.every((v) => v === 0)) { failedAutoInstall = true; await updateAction('install'); }
  }
}
renderUpdate();
if (invoke) updateAction('status').then(automaticUpdates).catch((e) => message(String(e)));
setInterval(() => automaticUpdates().catch((e) => message(e.message)), 60000);
setInterval(() => { if (dialog.open && !$('[data-pane="network"]').hidden) network(false).catch((e) => message(e.message)); }, 5000);

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
$('#cli-uninstall').onclick = run(() => cliAction('uninstall'));
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
$('#pico-flash').onclick = run(() => installStep(['#pico-flash'], '#pico-status', t('Looking for a Pico in BOOTSEL mode… Hold BOOTSEL while you plug it in.'), async () => {
  say('#pico-status', (await component('pico', 'flash')).message);
}));
$('#board-probe').onclick = run(() => installStep(['#board-probe', '#board-install'], '#board-status', t('Connecting…'), async () => {
  const b = await component('board', 'probe', { host: $('#board-host').value.trim() });
  say('#board-status', `${b.arch} · ${b.installed ? `${t('Installed:')} ${b.installed}` : t('NS2Bench Bridge is not installed.')}${b.supported ? '' : ` · ${t('This board is not supported.')}`}`);
}));
$('#board-install').onclick = run(() => installStep(['#board-probe', '#board-install'], '#board-status', t('Installing… The Bridge restarts, so held buttons are released.'), async () => {
  const r = await component('board', 'install', { host: $('#board-host').value.trim() });
  say('#board-status', r.message); $('#board-log').textContent = r.log || ''; $('#board-log').hidden = !r.log;
}));
$('#netd-install').onclick = run(() => installStep(['#netd-install', '#netd-uninstall'], '#netd-status', t('Installing…'), async () => {
  const r = await component('netd', 'install'); netd = r.status; renderNetd(); $('#netd-status').textContent += ` · ${r.message}`;
}));
$('#netd-uninstall').onclick = run(() => installStep(['#netd-install', '#netd-uninstall'], '#netd-status', t('Removing…'), async () => {
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
