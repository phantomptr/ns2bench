// CHANGELOG.md helpers for releases.
//   node scripts/changelog.mjs notes 0.1.0     print that version's section (fails if empty)
//   node scripts/changelog.mjs release 0.1.0   name the "## Unreleased" text "## 0.1.0" and
//                                              start a fresh "## Unreleased" (no-op if 0.1.0 exists)
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const path = join(dirname(fileURLToPath(import.meta.url)), '..', 'CHANGELOG.md');
const esc = (v) => v.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

export function section(text, version) {
  const m = text.match(new RegExp(`^## ${esc(version)}\\s*\\n([\\s\\S]*?)(?=^## |(?![\\s\\S]))`, 'm'));
  return m ? m[1].trim() : '';
}

export function release(text, version) {
  if (section(text, version)) return text;
  const unreleased = section(text, 'Unreleased');
  if (!unreleased) throw new Error(`CHANGELOG.md has no text under "## Unreleased" for ${version}`);
  return text.replace(/^## Unreleased\s*\n[\s\S]*?(?=^## |(?![\s\S]))/m,
    // a function: the notes are text, not a replacement pattern ($& and $' would be expanded)
    () => `## Unreleased\n\n## ${version}\n\n${unreleased}\n\n`);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const [cmd, version] = process.argv.slice(2);
  if (!/^\d+\.\d+\.\d+$/.test(version || '')) throw new Error('usage: changelog.mjs notes|release X.Y.Z');
  const text = readFileSync(path, 'utf8');
  if (cmd === 'notes') {
    const s = section(text, version);
    if (!s) { console.error(`no CHANGELOG section for ${version}`); process.exit(1); }
    console.log(s);
  } else if (cmd === 'release') {
    writeFileSync(path, release(text, version));
  } else {
    throw new Error('usage: changelog.mjs notes|release X.Y.Z');
  }
}
