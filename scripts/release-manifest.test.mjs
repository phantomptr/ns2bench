import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { generateKeyPairSync, verify } from 'node:crypto';
import { manifest, platformFor, signedText } from './release-manifest.mjs';
const { privateKey, publicKey } = generateKeyPairSync('ed25519');
const pem = privateKey.export({ type: 'pkcs8', format: 'pem' });
test('Tauri installer names map to updater platform names', () => {
  assert.equal(platformFor('NS2Bench_0.2.0_aarch64.dmg'), 'darwin-aarch64');
  assert.equal(platformFor('NS2Bench_0.2.0_x64.dmg'), 'darwin-x86_64');
  assert.equal(platformFor('NS2Bench_0.2.0_x64-setup.exe'), 'windows-x86_64');
  assert.equal(platformFor('NS2Bench_0.2.0_amd64.AppImage'), 'linux-x86_64');
  assert.equal(platformFor('ns2bench-board-aarch64'), null);
  // publish.yml's normalised names
  assert.equal(platformFor('NS2Bench-0.0.1-mac-arm64.dmg'), 'darwin-aarch64');
  assert.equal(platformFor('NS2Bench-0.0.1-mac-x64.dmg'), 'darwin-x86_64');
  assert.equal(platformFor('NS2Bench-0.0.1-win-x64-setup.exe'), 'windows-x86_64');
  assert.equal(platformFor('NS2Bench-0.0.1-win-arm64-setup.exe'), 'windows-aarch64');
  assert.equal(platformFor('NS2Bench-0.0.1-linux-x64.AppImage'), 'linux-x86_64');
  assert.equal(platformFor('NS2Bench-0.0.1-linux-arm64.AppImage'), 'linux-aarch64');
  assert.equal(platformFor('NS2Bench-0.0.1-linux-x64.deb'), null);
  assert.equal(platformFor('ns2bench-cli-0.0.1-windows-x64.exe'), null);
});
test('release requires every supported installer and hashes the actual bytes', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'ns2bench-manifest-'));
  try {
    await assert.rejects(manifest(dir, 'v0.3.0', pem), /Missing installer/);
    for (const name of ['NS2Bench_aarch64.dmg', 'NS2Bench_x64.dmg', 'NS2Bench_x64-setup.exe', 'NS2Bench_arm64-setup.exe', 'NS2Bench_amd64.AppImage']) await writeFile(join(dir, name), 'abc');
    await assert.rejects(manifest(dir, 'v0.3.0', ''), /NS2BENCH_UPDATE_KEY/);
    const m = await manifest(dir, 'v0.3.0', pem);
    assert.equal(Object.keys(m.assets).length, 5);
    assert.equal(m.sha256['darwin-aarch64'], 'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad');
    for (const p of Object.keys(m.assets)) {
      assert.ok(verify(null, Buffer.from(signedText('0.3.0', p, m.assets[p], m.sha256[p])), publicKey, Buffer.from(m.signatures[p], 'base64')), p);
    }
    await assert.rejects(manifest(dir, 'v0.3.0-beta.1', pem), /Stable/);
  } finally { await rm(dir, { recursive: true, force: true }); }
});
