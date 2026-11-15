import { test } from 'node:test';
import assert from 'node:assert/strict';
import { release, section } from './changelog.mjs';

const doc = '# Changelog\n\n## Unreleased\n\n- **New:** thing.\n\n## 0.0.1\n\nFirst.\n';
test('section reads one version, last one included', () => {
  assert.equal(section(doc, '0.0.1'), 'First.');
  assert.equal(section(doc, 'Unreleased'), '- **New:** thing.');
  assert.equal(section(doc, '9.9.9'), '');
});
test('release names the unreleased text and starts a fresh section', () => {
  const out = release(doc, '0.0.2');
  assert.equal(section(out, '0.0.2'), '- **New:** thing.');
  assert.equal(section(out, 'Unreleased'), '');
  assert.equal(section(out, '0.0.1'), 'First.');
  assert.equal(release(out, '0.0.2'), out);
  assert.throws(() => release(out, '0.0.3'), /no text under/);
});
