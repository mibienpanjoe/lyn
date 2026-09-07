const assert = require('node:assert');
const test = require('node:test');
const { createBrowserObservation, sanitizeUrl } = require('./sanitize.cjs');

test('strips sensitive query parameters and hash fragments from URLs', () => {
  const clean = sanitizeUrl(
    'http://localhost:5173/dashboard?auth_token=secret123&session=abc#section',
  );
  assert.strictEqual(clean, 'http://localhost:5173/dashboard');

  const cleanFile = sanitizeUrl(
    'file:///home/user/projects/repo/index.html?param=1',
  );
  assert.strictEqual(cleanFile, 'file:///home/user/projects/repo/index.html');
});

test('rejects non-web schemes like javascript: or data: or chrome://', () => {
  assert.strictEqual(sanitizeUrl('javascript:alert(1)'), null);
  assert.strictEqual(sanitizeUrl('data:text/html,hello'), null);
  assert.strictEqual(sanitizeUrl('chrome://settings'), null);
  assert.strictEqual(sanitizeUrl('about:blank'), null);
});

test('incognito tabs are flagged and stripped of url and title', () => {
  const obs = createBrowserObservation('instance-1', 'focused', {
    url: 'http://localhost:3000/admin',
    title: 'Secret Page',
    incognito: true,
  });

  assert.strictEqual(obs.incognito, true);
  assert.strictEqual(obs.url, null);
  assert.strictEqual(obs.title, null);
});

test('focused web observation preserves host, port, and sanitized path', () => {
  const obs = createBrowserObservation('instance-2', 'focused', {
    url: 'http://127.0.0.1:8080/api/v1/resource?debug=true',
    title: 'Dev Server',
    incognito: false,
  });

  assert.strictEqual(obs.version, 1);
  assert.strictEqual(obs.instanceId, 'instance-2');
  assert.strictEqual(obs.state, 'focused');
  assert.strictEqual(obs.url, 'http://127.0.0.1:8080/api/v1/resource');
  assert.strictEqual(obs.title, 'Dev Server');
  assert.strictEqual(obs.incognito, false);
});

test('ended state clears url and title', () => {
  const obs = createBrowserObservation('instance-3', 'ended', {
    url: 'http://localhost:3000',
    title: 'Some App',
  });

  assert.strictEqual(obs.state, 'ended');
  assert.strictEqual(obs.url, null);
  assert.strictEqual(obs.title, null);
});
