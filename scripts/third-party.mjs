// Third-party notices for the binaries we ship: every crate compiled into the engine,
// app, CLI, board daemon, network daemon and Pico firmware, with its license and the
// license/notice texts from its source. MIT/BSD-style licenses require shipping these.
//   node scripts/third-party.mjs [out-file]     (default: THIRD-PARTY-LICENSES.txt)
import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const out = process.argv[2] || join(root, 'THIRD-PARTY-LICENSES.txt');

function packages(dir) {
  const meta = JSON.parse(execFileSync('cargo', ['metadata', '--locked', '--format-version', '1'],
    { cwd: join(root, dir), maxBuffer: 1 << 28 }).toString());
  const ours = new Set(meta.workspace_members);
  return meta.packages.filter((p) => !ours.has(p.id));
}

const seen = new Map();
for (const dir of ['native', 'firmware/pico']) {
  for (const p of packages(dir)) seen.set(`${p.name} ${p.version}`, p);
}
const list = [...seen.values()].sort((a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version));

const parts = [
  'NS2Bench third-party licenses',
  '==============================',
  '',
  `NS2Bench is GPL-3.0-or-later (see LICENSE and NOTICE). Its binaries include the ${list.length} open-source`,
  'Rust packages below. Each is listed with its declared license and the license and',
  'notice files shipped in its source. GP2040-CE-derived data is covered in LICENSES/.',
  '',
];
for (const p of list) {
  parts.push('-'.repeat(78), `${p.name} ${p.version}`, `License: ${p.license || p.license_file || 'see source'}`);
  if (p.repository) parts.push(`Source: ${p.repository}`);
  const src = dirname(p.manifest_path);
  const files = existsSync(src) ? readdirSync(src).filter((f) => /^(LICEN[SC]E|COPYING|NOTICE|COPYRIGHT|UNLICENSE)/i.test(f)).sort() : [];
  for (const f of files) {
    try { parts.push('', `[${f}]`, readFileSync(join(src, f), 'utf8').trim()); } catch { /* unreadable: skip */ }
  }
  parts.push('');
}
writeFileSync(out, parts.join('\n') + '\n');
console.log(`${list.length} packages -> ${out}`);
