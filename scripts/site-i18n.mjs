// The website's words. English lives in the HTML (site/index.html, the FAQ and setup pages'
// template in scripts/site-build.mjs) and in t("key", "English") calls in site/app.js; the other
// languages in site/i18n/<code>.json, with flat dotted keys.
//   node scripts/site-i18n.mjs --check     fail if en.json is out of date or a language misses a key
//   node scripts/site-i18n.mjs --write-en  rewrite site/i18n/en.json from the HTML and app.js
// site-build.mjs runs the check first, so the pages build fails on any gap.
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const dir = join(root, 'site/i18n');

/** The app's languages, in the app's order (the same list as site/app.js). */
export const LANGS = [
  ['en', 'English'],
  ['zh-CN', '简体中文'],
  ['zh-TW', '繁體中文'],
  ['ja', '日本語'],
  ['ko', '한국어'],
  ['es', 'Español'],
  ['pt-BR', 'Português (Brasil)'],
  ['fr', 'Français'],
  ['de', 'Deutsch'],
  ['it', 'Italiano'],
  ['ru', 'Русский'],
];

const tidy = (s) => s.replace(/\s+/g, ' ').trim();
const unescape = (s) => s.replace(/&quot;/g, '"');

/** English words by key, from HTML (data-i18n, data-i18n-attr) and JS (t("key", "English")). */
export function extract({ html = [], js = [] }) {
  const words = {};
  const errors = [];
  const add = (key, value, where) => {
    if (key in words && words[key] !== value) errors.push(`${key}: different English in ${where}: "${value}" vs "${words[key]}"`);
    else words[key] = value;
  };
  for (const [where, src] of html) {
    for (const m of src.matchAll(/<(\w+)\b([^>]*?)\bdata-i18n="([^"]+)"[^>]*>([\s\S]*?)<\/\1>/g)) add(m[3], tidy(m[4]), where);
    for (const m of src.matchAll(/<\w+\b[^>]*\bdata-i18n-attr="([^"]+)"[^>]*>/g)) {
      for (const pair of m[1].split(';')) {
        const [attr, key] = pair.split(':');
        const v = m[0].match(new RegExp(`\\s${attr}="([^"]*)"`));
        if (!v) errors.push(`${key}: no ${attr}="..." beside data-i18n-attr in ${where}`);
        else add(key, tidy(unescape(v[1])), where);
      }
    }
  }
  for (const [where, src] of js) {
    for (const m of src.matchAll(/\bt\(\s*"([^"]+)",\s*"((?:[^"\\]|\\.)*)"/g)) add(m[1], JSON.parse(`"${m[2]}"`), where);
  }
  return { words, errors };
}

const placeholders = (s) => [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort().join(',');
const links = (s) => (s.match(/<a\s/g) || []).length;

async function sources(pageHtml) {
  if (!pageHtml) {
    const build = await import('./site-build.mjs');
    pageHtml = Object.values(build.pages).map((p) => build.page({ ...p, main: '' }));
  }
  return {
    html: [['site/index.html', readFileSync(join(root, 'site/index.html'), 'utf8')], ...pageHtml.map((h, i) => [`page template #${i + 1}`, h])],
    js: [['site/app.js', readFileSync(join(root, 'site/app.js'), 'utf8')]],
  };
}

/** Throws (listing every gap) unless en.json matches the pages and every language has every key. */
export function check({ pageHtml }) {
  const { words, errors } = extract({
    html: [['site/index.html', readFileSync(join(root, 'site/index.html'), 'utf8')], ...pageHtml.map((h, i) => [`page template #${i + 1}`, h])],
    js: [['site/app.js', readFileSync(join(root, 'site/app.js'), 'utf8')]],
  });
  const load = (code) => {
    try {
      return JSON.parse(readFileSync(join(dir, `${code}.json`), 'utf8'));
    } catch (e) {
      errors.push(`${code}.json: ${e.message}`);
      return {};
    }
  };
  const en = load('en');
  for (const [k, v] of Object.entries(words)) {
    if (!(k in en)) errors.push(`en.json: missing ${k} (run node scripts/site-i18n.mjs --write-en)`);
    else if (en[k] !== v) errors.push(`en.json: ${k} differs from the page (run node scripts/site-i18n.mjs --write-en)`);
  }
  for (const k of Object.keys(en)) if (!(k in words)) errors.push(`en.json: ${k} is not used by any page`);
  for (const [code] of LANGS.slice(1)) {
    const tr = load(code);
    for (const k of Object.keys(en)) {
      if (typeof tr[k] !== 'string' || !tr[k].trim()) errors.push(`${code}.json: missing ${k}`);
      else {
        if (placeholders(tr[k]) !== placeholders(en[k])) errors.push(`${code}.json: ${k} has placeholders {${placeholders(tr[k])}}, English {${placeholders(en[k])}}`);
        if (links(tr[k]) !== links(en[k])) errors.push(`${code}.json: ${k} has ${links(tr[k])} links, English ${links(en[k])}`);
      }
    }
    for (const k of Object.keys(tr)) if (!(k in en)) errors.push(`${code}.json: ${k} is not an English key`);
  }
  if (errors.length) throw new Error(`site translations:\n  ${errors.join('\n  ')}`);
  return Object.keys(en).length;
}

async function cli() {
  const arg = process.argv[2];
  const src = await sources();
  if (arg === '--write-en') {
    const { words, errors } = extract(src);
    if (errors.length) throw new Error(errors.join('\n'));
    const sorted = Object.fromEntries(Object.entries(words).sort(([a], [b]) => a.localeCompare(b)));
    writeFileSync(join(dir, 'en.json'), `${JSON.stringify(sorted, null, 2)}\n`);
    console.log(`site/i18n/en.json: ${Object.keys(sorted).length} keys`);
  } else if (arg === '--check') {
    const n = check({ pageHtml: src.html.slice(1).map(([, h]) => h) });
    console.log(`site translations: ${LANGS.length} languages, ${n} keys each, complete`);
  } else {
    console.error('usage: node scripts/site-i18n.mjs --check | --write-en');
    process.exit(2);
  }
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  cli().catch((e) => {
    console.error(e.message);
    process.exit(1);
  });
}
