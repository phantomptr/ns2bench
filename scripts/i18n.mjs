// Interface strings for web/i18n.js (English text is the key).
//   node scripts/i18n.mjs            write web/i18n/en.json from web/index.html + web/*.js
//   node scripts/i18n.mjs --check    fail if en.json is stale or a locale misses/has extra keys
import { readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const web = join(root, 'web');
const dir = join(web, 'i18n');

// &amp; last, so "&amp;lt;" stays the text "&lt;" instead of becoming "<".
const decode = (s) => s.replace(/&lt;/g, '<').replace(/&gt;/g, '>')
  .replace(/&quot;/g, '"').replace(/&#39;/g, "'").replace(/&nbsp;/g, ' ').replace(/&amp;/g, '&');
// Worth translating: has letters, is not code, an identifier or a pure key name.
const wanted = (s) => /\p{L}{2}/u.test(s) && !/^[A-Za-z0-9]{25,}$/.test(s) && !/^[A-Z0-9_]+$/.test(s) && !/[{}=]|;$|^\/|https?:|^[\w-]+\.(js|css|svg|json)$/.test(s)
  && !['NS2Bench', 'MCP', 'HTTP API', 'FPS', 'WASD', 'TFGH', 'Bksp', 'Enter', 'Escape', 'POST', 'GameChat', 'VideoDecoder',
    'Content-Type', 'Content-Type: application/json', 'Discord', 'UndefinedLab Discord', 'GitHub', 'Ko-fi', 'X (@phantomptr)'].includes(s);

export function extract() {
  const keys = new Set();
  const add = (s) => { s = decode(s).replace(/\s+/g, ' ').trim(); if (s && wanted(s)) keys.add(s); };
  const html = readFileSync(join(web, 'index.html'), 'utf8')
    .replace(/<script[\s\S]*?<\/script>/g, '').replace(/<style[\s\S]*?<\/style>/g, '')
    .replace(/<pre[^>]*>[\s\S]*?<\/pre>/g, '').replace(/<svg[\s\S]*?<\/svg>/g, '');
  for (const m of html.matchAll(/>([^<>]+)</g)) add(m[1]);
  for (const m of html.matchAll(/\s(?:title|aria-label|placeholder)="([^"]*)"/g)) add(m[1]);
  for (const f of readdirSync(web).filter((f) => f.endsWith('.js') && f !== 'i18n.js')) {
    const js = readFileSync(join(web, f), 'utf8');
    // Plain string literals that read like interface text (start upper-case, or end in …).
    for (const m of js.matchAll(/'((?:[A-Z]|Bridge)[^'`$\\\n]{2,})'/g)) if (/\s|…$|\.$/.test(m[1])) add(m[1]);
    for (const m of js.matchAll(/\bt\('([^'\\\n]+)'\)/g)) add(m[1]);
  }
  return [...keys].sort((a, b) => a.localeCompare(b));
}

function main(args) {
  const keys = extract();
  const en = Object.fromEntries(keys.map((k) => [k, k]));
  const enPath = join(dir, 'en.json');
  if (!args.includes('--check')) {
    writeFileSync(enPath, JSON.stringify(en, null, 2) + '\n');
    console.log(`${keys.length} strings -> web/i18n/en.json`);
  }
  let bad = 0;
  const current = JSON.parse(readFileSync(enPath, 'utf8'));
  if (JSON.stringify(Object.keys(current)) !== JSON.stringify(keys)) {
    console.error('web/i18n/en.json is stale: run node scripts/i18n.mjs');
    bad++;
  }
  for (const f of readdirSync(dir).filter((f) => f.endsWith('.json') && f !== 'en.json')) {
    const loc = JSON.parse(readFileSync(join(dir, f), 'utf8'));
    const missing = keys.filter((k) => typeof loc[k] !== 'string' || !loc[k].trim());
    const extra = Object.keys(loc).filter((k) => !(k in en));
    if (missing.length || extra.length) {
      console.error(`${f}: ${missing.length} missing, ${extra.length} unused${missing.length ? ` (e.g. "${missing[0]}")` : ''}`);
      bad++;
    }
  }
  if (bad) process.exit(1);
  console.log(`i18n ok: ${keys.length} strings`);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) main(process.argv.slice(2));
