const assert = require('node:assert');
const crypto = require('node:crypto');
const fs = require('node:fs');
const path = require('node:path');
const test = require('node:test');
const {
  createBrowserInvoke,
  createBrowserObservation,
  localContextUrl,
  sanitizeUrl,
} = require('./sanitize.cjs');

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

test('only file and localhost-family URLs are correlation hints', () => {
  assert.strictEqual(
    localContextUrl('http://localhost:5173/dashboard?auth=1#x'),
    'http://localhost:5173/dashboard',
  );
  assert.strictEqual(
    localContextUrl('http://127.0.0.1:8080/api/v1/resource?debug=true'),
    'http://127.0.0.1:8080/api/v1/resource',
  );
  assert.strictEqual(
    localContextUrl('http://[::1]:9090/'),
    'http://[::1]:9090/',
  );
  assert.strictEqual(
    localContextUrl('https://example.com/app?token=secret'),
    null,
  );
  assert.strictEqual(localContextUrl('javascript:alert(1)'), null);
});

test('incognito tabs are flagged and stripped of url and title', () => {
  const obs = createBrowserObservation('instance-1', 'focused', {
    url: 'http://localhost:3000/admin',
    title: 'Secret Page',
    incognito: true,
  });

  assert.strictEqual(obs.incognito, true);
  assert.strictEqual(obs.url, null);
  assert.strictEqual(obs.title, undefined);
  assert.strictEqual(obs.state, 'ended');
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
  assert.strictEqual(obs.title, undefined);
  assert.strictEqual(obs.incognito, false);
});

test('ended state clears url and title', () => {
  const obs = createBrowserObservation('instance-3', 'ended', {
    url: 'http://localhost:3000',
    title: 'Some App',
  });

  assert.strictEqual(obs.state, 'ended');
  assert.strictEqual(obs.url, null);
  assert.strictEqual(obs.title, undefined);
});

test('remote https observations do not send a url', () => {
  const obs = createBrowserObservation('instance-4', 'focused', {
    url: 'https://github.com/org/repo?tab=readme',
    title: 'Repo',
    incognito: false,
  });
  assert.strictEqual(obs.url, null);
});

test('v2 invoke requests carry a generation and no native ids', () => {
  const request = createBrowserInvoke(
    'instance-5',
    {
      url: 'http://localhost:5173/app?token=secret#frag',
      title: 'App',
      incognito: false,
    },
    'c3b1a2d0-1111-4aaa-8bbb-0123456789ab',
  );

  assert.deepStrictEqual(request, {
    version: 2,
    kind: 'invoke',
    instanceId: 'instance-5',
    requestId: 'c3b1a2d0-1111-4aaa-8bbb-0123456789ab',
    url: 'http://localhost:5173/app',
    incognito: false,
  });
  assert.ok(!('title' in request));
  assert.ok(!('windowId' in request));
  assert.ok(!('processId' in request));
  assert.ok(!('cwd' in request));
});

test('v2 invoke mints a request id when the caller omits one', () => {
  const request = createBrowserInvoke('instance-6', {
    url: 'file:///tmp/lyn-cl11-alpha/index.html',
  });
  assert.strictEqual(request.version, 2);
  assert.strictEqual(request.kind, 'invoke');
  assert.match(
    request.requestId,
    /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i,
  );
  assert.strictEqual(request.url, 'file:///tmp/lyn-cl11-alpha/index.html');
});

test('incognito invoke opens without a url', () => {
  const request = createBrowserInvoke('instance-7', {
    url: 'http://localhost:3000/secret',
    title: 'Private',
    incognito: true,
  });
  assert.strictEqual(request.kind, 'invoke');
  assert.strictEqual(request.url, null);
  assert.strictEqual(request.incognito, true);
  assert.ok(!('title' in request));
});

test('remote invoke does not send the page url', () => {
  const request = createBrowserInvoke('instance-8', {
    url: 'https://example.com/dashboard?auth=1',
  });
  assert.strictEqual(request.kind, 'invoke');
  assert.strictEqual(request.url, null);
});

test('manifest.json defines consistent Chromium key and Firefox Gecko ID', () => {
  const manifest = JSON.parse(
    fs.readFileSync(path.join(__dirname, 'manifest.json'), 'utf8'),
  );
  assert.strictEqual(
    manifest.browser_specific_settings?.gecko?.id,
    'lyn-context-provider@mibienpanjoe.com',
  );

  assert.ok(manifest.key, 'manifest.key must be present');
  const der = Buffer.from(manifest.key, 'base64');
  const hash = crypto.createHash('sha256').update(der).digest();
  const id = Array.from(hash.subarray(0, 16))
    .map(
      (b) =>
        String.fromCharCode(97 + (b >> 4)) +
        String.fromCharCode(97 + (b & 0xf)),
    )
    .join('');
  assert.strictEqual(id, 'aecihlceemkggejjmpphmnhpdcgnhife');
  assert.strictEqual(
    manifest.commands?.['lyn.capture']?.suggested_key?.default,
    'Alt+Shift+L',
  );
  assert.strictEqual(manifest.background?.service_worker, 'background.js');
});

test('manifest.firefox.json uses an event page instead of a service worker', () => {
  const manifest = JSON.parse(
    fs.readFileSync(path.join(__dirname, 'manifest.firefox.json'), 'utf8'),
  );
  assert.strictEqual(
    manifest.browser_specific_settings?.gecko?.id,
    'lyn-context-provider@mibienpanjoe.com',
  );
  assert.deepStrictEqual(manifest.background?.scripts, [
    'sanitize.js',
    'background.js',
  ]);
  assert.strictEqual(manifest.background?.service_worker, undefined);
  assert.strictEqual(
    manifest.commands?.['lyn.capture']?.suggested_key?.default,
    'Alt+Shift+L',
  );
});
