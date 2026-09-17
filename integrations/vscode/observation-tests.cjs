const path = require('node:path');
const test = require('node:test');
const assert = require('node:assert/strict');

const {
  createInvokeRequest,
  createObservation,
  localWorkspacePaths,
  providerSocketPath,
} = require('./observation.cjs');

const instanceId = '7af0a690-8948-4f0a-b9f0-51e43c583efa';

function folder(scheme, fsPath) {
  return { uri: { scheme, fsPath } };
}

test('reports one local workspace without editor or terminal content', () => {
  assert.deepEqual(
    createObservation(instanceId, 'focused', [
      folder('file', '/home/person/projects/lyn'),
    ]),
    {
      version: 1,
      instanceId,
      state: 'focused',
      workspaceFolders: [path.normalize('/home/person/projects/lyn')],
    },
  );
});

test('omits remote workspaces and deduplicates local folders', () => {
  assert.deepEqual(
    localWorkspacePaths([
      folder('vscode-remote', '/remote/project'),
      folder('file', '/home/person/project'),
      folder('file', '/home/person/project'),
    ]),
    [path.normalize('/home/person/project')],
  );
});

test('preserves multiple local roots so Lyn can reject ambiguity', () => {
  const observation = createObservation(instanceId, 'unfocused', [
    folder('file', '/home/person/first'),
    folder('file', '/home/person/second'),
  ]);

  assert.equal(observation.workspaceFolders.length, 2);
});

test('ended observations carry no workspace path', () => {
  const observation = createObservation(instanceId, 'ended', [
    folder('file', '/home/person/project'),
  ]);

  assert.deepEqual(observation.workspaceFolders, []);
  assert.equal(observation.intent, undefined);
  assert.equal(observation.kind, undefined);
});

test('v2 invoke requests carry a generation and no native ids', () => {
  const requestId = 'c3b1a2d0-1111-4aaa-8bbb-0123456789ab';
  const request = createInvokeRequest(
    instanceId,
    [folder('file', '/tmp/lyn-cl01-alpha')],
    requestId,
  );
  assert.deepEqual(request, {
    version: 2,
    kind: 'invoke',
    instanceId,
    requestId,
    workspaceFolders: [path.normalize('/tmp/lyn-cl01-alpha')],
  });
  assert.equal(request.windowId, undefined);
  assert.equal(request.pid, undefined);
});

test('v2 invoke mints a request id when the caller omits one', () => {
  const request = createInvokeRequest(instanceId, [
    folder('file', '/tmp/lyn-cl01-beta'),
  ]);
  assert.equal(request.version, 2);
  assert.match(
    request.requestId,
    /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i,
  );
});

test('uses only an absolute Linux user runtime directory', () => {
  assert.equal(
    providerSocketPath({ XDG_RUNTIME_DIR: '/run/user/1000' }, 'linux'),
    path.join('/run/user/1000', 'lyn-context-v1.sock'),
  );
  assert.equal(
    providerSocketPath({ XDG_RUNTIME_DIR: 'relative' }, 'linux'),
    undefined,
  );
  assert.equal(
    providerSocketPath({ XDG_RUNTIME_DIR: '/run/user/1000' }, 'darwin'),
    undefined,
  );
});
