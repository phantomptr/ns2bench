// Sync the version in VERSION into every file that carries it.
//   node scripts/update-version.mjs 0.1.0   set VERSION and sync
//   node scripts/update-version.mjs         sync from VERSION
//   node scripts/update-version.mjs --check fail if any file disagrees with VERSION
import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const at = (p) => join(root, p);

// Each entry: file, how to read its version, how to write it.
const tomlPackage = (file) => ({
  file,
  get: (s) => s.match(/^\[(?:workspace\.)?package\][^[]*?^version = "([^"]+)"/m)?.[1],
  set: (s, v) => s.replace(/^(\[(?:workspace\.)?package\][^[]*?^version = ")[^"]+(")/m, `$1${v}$2`),
});
// Lockfile entries for our own crates only (third-party crates may share a version).
const lock = (file) => ({
  file,
  get: (s) => s.match(/^name = "ns2bench[^"]*"\nversion = "([^"]+)"/m)?.[1],
  set: (s, v) => s.replace(/^(name = "ns2bench[^"]*"\nversion = ")[^"]+(")/gm, `$1${v}$2`),
});
export const FILES = [
  tomlPackage('native/Cargo.toml'),
  lock('native/Cargo.lock'),
  tomlPackage('firmware/pico/Cargo.toml'),
  lock('firmware/pico/Cargo.lock'),
  {
    file: 'native/crates/app/tauri.conf.json',
    get: (s) => JSON.parse(s).version,
    set: (s, v) => s.replace(/("version":\s*")[^"]+(")/, `$1${v}$2`),
  },
];

const valid = (v) => /^\d+\.\d+\.\d+$/.test(v);

function main(args) {
  const check = args.includes('--check');
  const given = args.find((a) => !a.startsWith('--'));
  if (given) {
    if (!valid(given)) throw new Error(`version must be X.Y.Z, got ${given}`);
    writeFileSync(at('VERSION'), `${given}\n`);
  }
  const version = readFileSync(at('VERSION'), 'utf8').trim();
  if (!valid(version)) throw new Error(`VERSION must hold X.Y.Z, got ${version}`);
  let bad = 0;
  for (const f of FILES) {
    const text = readFileSync(at(f.file), 'utf8');
    const found = f.get(text);
    if (found === undefined) throw new Error(`${f.file}: version not found`);
    if (found === version) continue;
    if (check) {
      console.error(`${f.file}: ${found} (VERSION says ${version})`);
      bad++;
    } else {
      writeFileSync(at(f.file), f.set(text, version));
      console.log(`${f.file}: ${found} -> ${version}`);
    }
  }
  if (bad) process.exit(1);
  if (check) console.log(`all version files at ${version}`);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) main(process.argv.slice(2));
