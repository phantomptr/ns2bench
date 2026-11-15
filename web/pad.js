// On-screen Switch 2 controller in a flat style (like the console's own controller drawings).
// Keyboard keys are printed inside each button; presses light up in the side colour when you
// press, purple when an agent does. Hit areas are generous (min 44 px).

export const BIT = { Y: 0, B: 1, A: 2, X: 3, L: 4, R: 5, ZL: 6, ZR: 7, MINUS: 8, PLUS: 9,
  LSTICK: 10, RSTICK: 11, HOME: 12, CAPTURE: 13, C: 14, GL: 15, GR: 16 };   // C/GL/GR: Pro Controller mode
export const DPAD = { UP: 1, RIGHT: 2, DOWN: 4, LEFT: 8 };
const HAT_BITS = [1, 3, 2, 6, 4, 12, 8, 9, 0];   // hat 0..8 -> dpad bits
const TAP_MS = 220, TAP_TRAVEL = 0.18;          // a short stick tap = stick click

const tri = (turn) => `<svg class="g" viewBox="0 0 20 20" aria-hidden="true"><path transform="rotate(${turn} 10 10)" d="M10 5 L15 13.5 H5 Z"/></svg>`;
const GLYPH = {
  MINUS: '<svg class="g" viewBox="0 0 20 20" aria-hidden="true"><rect x="4.5" y="8.6" width="11" height="2.8" rx="1.4"/></svg>',
  PLUS: '<svg class="g" viewBox="0 0 20 20" aria-hidden="true"><path d="M8.6 4.5h2.8v4.1h4.1v2.8h-4.1v4.1H8.6v-4.1H4.5V8.6h4.1z"/></svg>',
  CAPTURE: '<svg class="g" viewBox="0 0 20 20" aria-hidden="true"><circle cx="10" cy="10" r="4.2" fill="none" stroke="currentColor" stroke-width="2.2"/></svg>',
  HOME: '<svg class="g" viewBox="0 0 20 20" aria-hidden="true"><path d="M4.3 10.3 10 5.2l5.7 5.1M6.6 8.6v6.3h6.8V8.6" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"/></svg>',
};
const letter = (l) => `<span class="g letter">${l}</span>`;

function make(tag, cls, html = '') {
  const e = document.createElement(tag);
  e.className = cls;
  e.innerHTML = html;
  return e;
}

// Marks text for the translation catalog (scripts/i18n.mjs). The page keeps the English text;
// web/i18n.js swaps it for the chosen language wherever it is shown.
const t = (english) => english;

function key(name, label, glyph, cls, unavailable = false) {
  const b = make('button', `key ${cls}${unavailable ? ' unavailable' : ''}`,
    `<span class="face">${glyph}<span class="kl"></span></span>`);
  b.type = 'button';
  b.dataset.b = name;
  b.dataset.label = label;
  b.setAttribute('aria-label', label);
  if (unavailable) b.setAttribute('aria-disabled', 'true');
  return b;
}

function stick(which) {
  const s = make('div', `stick ${which}`, '<div class="base"><div class="thumb"></div></div><div class="sl"></div>');
  s.dataset.stick = which;
  s.dataset.label = which === 'left' ? t('Left stick') : t('Right stick');
  s.setAttribute('role', 'slider');
  s.setAttribute('aria-label', which === 'left' ? t('Left stick: drag to tilt, tap to press') : t('Right stick: drag to tilt, tap to press'));
  return s;
}

const cluster = (cls, keys) => { const c = make('div', `cluster ${cls}`); c.append(...keys); return c; };

/** hints: {A: ['L'], ..., left: ['W','A','S','D'], LSTICK: ['V']}.
 *  Returns {state(), highlight(pad, agent), release(), onPress(fn)}. */
export function mountPad(leftRoot, rightRoot, { hints = {} } = {}) {
  const held = new Set();
  const taps = new Set();
  const vstick = { left: [0, 0], right: [0, 0] };
  let pressListener = () => {};
  let macroListener = () => {};

  leftRoot.append(make('div', 'shoulders'), make('div', 'body'));
  rightRoot.append(make('div', 'shoulders'), make('div', 'body'));
  const [lsh, lbody] = [leftRoot.querySelector('.shoulders'), leftRoot.querySelector('.body')];
  const [rsh, rbody] = [rightRoot.querySelector('.shoulders'), rightRoot.querySelector('.body')];

  lsh.append(key('ZL', 'ZL', letter('ZL'), 'pill'), key('L', 'L', letter('L'), 'pill'));
  rsh.append(key('R', 'R', letter('R'), 'pill'), key('ZR', 'ZR', letter('ZR'), 'pill'));
  lbody.append(
    key('MINUS', t('Minus'), GLYPH.MINUS, 'mini minus'),
    stick('left'),
    cluster('dirs', [key('UP', t('Up'), tri(0), 'n'), key('LEFT', t('Left'), tri(-90), 'w'),
      key('RIGHT', t('Right'), tri(90), 'e'), key('DOWN', t('Down'), tri(180), 's')]),
    key('CAPTURE', t('Capture'), GLYPH.CAPTURE, 'mini capture'),
    key('GL', t('GL (back button, Pro Controller mode)'), letter('GL'), 'grip gl'));
  rbody.append(
    key('PLUS', t('Plus'), GLYPH.PLUS, 'mini plus'),
    cluster('abxy', [key('X', 'X', letter('X'), 'n'), key('Y', 'Y', letter('Y'), 'w'),
      key('A', 'A', letter('A'), 'e'), key('B', 'B', letter('B'), 's')]),
    stick('right'),
    key('HOME', 'HOME', GLYPH.HOME, 'mini home'),
    key('C', 'C · GameChat', letter('C'), 'mini cbtn macro'),
    key('GR', t('GR (back button, Pro Controller mode)'), letter('GR'), 'grip gr'));

  const roots = [leftRoot, rightRoot];
  const all = (sel) => roots.flatMap((r) => [...r.querySelectorAll(sel)]);
  const swallow = (e) => e.stopPropagation();

  // keyboard key printed inside each button; stick keys under the stick
  for (const b of all('.key')) {
    const k = hints[b.dataset.b];
    if (k) b.querySelector('.kl').textContent = k.join(' ');
  }
  for (const s of all('.stick')) {
    const k = hints[s.dataset.stick];
    const click = hints[s.dataset.stick === 'left' ? 'LSTICK' : 'RSTICK'];
    if (k) s.querySelector('.sl').innerHTML = `<b>${k.join('')}</b>${click ? `<span>press ${click.join('')}</span>` : ''}`;
  }

  // hover tip (for unavailable keys, and for anyone with keys hidden)
  const tip = make('div', 'kbdtip');
  tip.setAttribute('role', 'tooltip');
  document.body.append(tip);
  const showTip = (el) => {
    const pro = leftRoot.closest('.pro');
    const why = el.classList.contains('macro') && !pro
      ? t('Opens GameChat from the HOME menu (switch to Pro Controller mode for a real C button)')
      : el.classList.contains('grip') && !pro ? 'Only in Pro Controller mode' : '';
    const k = hints[el.dataset.stick || el.dataset.b];
    tip.innerHTML = `<b>${el.dataset.label}</b>` + (why ? `<span class="none">${why}</span>`
      : k ? `<span>${k.map((x) => `<kbd>${x}</kbd>`).join('')}</span>` : '');
    const r = el.getBoundingClientRect();
    // Text size zooms the page. Where the browser measures in zoomed pixels, convert back to the
    // page's own (the ratio is 1 where it doesn't).
    const zoom = el.offsetWidth ? r.width / el.offsetWidth : 1;
    tip.style.left = `${(r.left + r.width / 2) / zoom}px`;
    tip.style.top = `${r.top / zoom - 8}px`;
    tip.classList.add('show');
  };
  for (const el of all('.key, .stick')) {
    el.addEventListener('pointerenter', (e) => {
      if (e.pointerType === 'mouse' && (el.classList.contains('macro') || !leftRoot.closest('.show-keys'))) showTip(el);
    });
    el.addEventListener('pointerleave', () => tip.classList.remove('show'));
  }

  for (const b of all('.key:not(.unavailable)')) {
    const name = b.dataset.b;
    // No context menu on a button: it would swallow the release and leave the button held.
    b.addEventListener('contextmenu', (e) => e.preventDefault());
    b.addEventListener('pointerdown', (e) => {
      e.preventDefault(); e.stopPropagation();
      if (e.pointerType === 'mouse' && e.button !== 0) return;
      if (b.classList.contains('macro') && !leftRoot.closest('.pro')) { macroListener(name); return; }
      b.setPointerCapture(e.pointerId); held.add(name);
    });
    const up = () => held.delete(name);
    b.addEventListener('pointerup', up);
    b.addEventListener('pointercancel', up);
    b.addEventListener('lostpointercapture', up);
    b.addEventListener('click', swallow);
  }

  for (const s of all('.stick')) {
    const which = s.dataset.stick;
    const click = which === 'left' ? 'LSTICK' : 'RSTICK';
    const base = s.querySelector('.base');
    let downAt = 0, travel = 0;
    const move = (e) => {
      const r = base.getBoundingClientRect();
      let x = (e.clientX - r.left - r.width / 2) / (r.width * 0.38);
      let y = (e.clientY - r.top - r.height / 2) / (r.height * 0.38);
      const len = Math.hypot(x, y);
      if (len > 1) { x /= len; y /= len; }
      travel = Math.max(travel, Math.hypot(x, y));
      vstick[which] = [x, y];
    };
    base.addEventListener('pointerdown', (e) => {
      e.preventDefault(); e.stopPropagation(); tip.classList.remove('show');
      base.setPointerCapture(e.pointerId); s.classList.add('grab');
      downAt = performance.now(); travel = 0; move(e);
      if (travel < TAP_TRAVEL) vstick[which] = [0, 0];
    });
    base.addEventListener('pointermove', (e) => { if (base.hasPointerCapture(e.pointerId)) move(e); });
    const end = (e) => {
      if (!s.classList.contains('grab')) return;
      s.classList.remove('grab');
      vstick[which] = [0, 0];
      if (e.type === 'pointerup' && performance.now() - downAt < TAP_MS && travel < TAP_TRAVEL) {
        taps.add(click);
        setTimeout(() => taps.delete(click), 120);
      }
    };
    base.addEventListener('pointerup', end);
    base.addEventListener('pointercancel', end);
    base.addEventListener('lostpointercapture', end);
    base.addEventListener('click', swallow);
  }

  const keys = all('.key');
  const thumbs = { left: leftRoot.querySelector('.thumb'), right: rightRoot.querySelector('.thumb') };
  const sticks = { left: leftRoot.querySelector('.stick'), right: rightRoot.querySelector('.stick') };
  let prev = new Set();

  return {
    state() {
      return { held: new Set([...held, ...taps]), sticks: [...vstick.left, ...vstick.right] };
    },
    /** pad = [buttons, hat, lx, ly, rx, ry]; agent = [buttons, dpad bits] held by agents. */
    highlight(pad, agent = [0, 0]) {
      const [buttons, hat, lx, ly, rx, ry] = pad;
      const dirs = HAT_BITS[hat] ?? 0;
      const now = new Set();
      for (const b of keys) {
        const n = b.dataset.b;
        const isBtn = n in BIT;
        const on = held.has(n) || (isBtn ? (buttons >> BIT[n]) & 1 : (dirs & (DPAD[n] ?? 0)) !== 0);
        const byAgent = on && !held.has(n) && (isBtn ? (agent[0] >> BIT[n]) & 1 : (agent[1] & (DPAD[n] ?? 0)) !== 0);
        b.classList.toggle('on', Boolean(on));
        b.classList.toggle('agent', Boolean(byAgent));
        if (on) {
          now.add(n);
          if (!prev.has(n)) pressListener(n, byAgent ? 'agent' : 'you');
        }
      }
      prev = now;
      const place = (which, x, y, bit) => {
        const dx = (x - 128) / 127.5, dy = (y - 128) / 127.5;
        thumbs[which].style.translate = `calc(${dx.toFixed(3)} * var(--travel)) calc(${dy.toFixed(3)} * var(--travel))`;
        sticks[which].classList.toggle('active', Math.abs(dx) > 0.06 || Math.abs(dy) > 0.06);
        sticks[which].classList.toggle('on', Boolean((buttons >> bit) & 1));
      };
      place('left', lx, ly, BIT.LSTICK);
      place('right', rx, ry, BIT.RSTICK);
    },
    onPress(fn) { pressListener = fn; },
    onMacro(fn) { macroListener = fn; },
    release() { held.clear(); taps.clear(); vstick.left = [0, 0]; vstick.right = [0, 0]; },
  };
}
