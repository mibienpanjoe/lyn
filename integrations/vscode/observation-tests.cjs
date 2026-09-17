const path = require('node:path');
const test = require('node:test');
const assert = require('node:assert/strict');

const {
  createInvokeRequest,
  createObservation,
  focusedTerminalCwd,
  invokeWorkspaceFolders,
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
    surface: 'editor',
    workspaceFolders: [path.normalize('/tmp/lyn-cl01-alpha')],
  });
  assert.equal(request.windowId, undefined);
  assert.equal(request.pid, undefined);
  assert.equal(request.cwd, undefined);
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

test('editor invoke ignores the last-used terminal cwd', () => {
  const folders = invokeWorkspaceFolders({
    surface: 'editor',
    workspaceFolders: [folder('file', '/home/person/projects/lyn')],
    terminalFocused: false,
    terminal: {
      shellIntegration: { cwd: { scheme: 'file', fsPath: '/tmp/other-project' } },
      creationOptions: { cwd: '/tmp/stale-after-cd' },
    },
  });
  assert.deepEqual(folders, [path.normalize('/home/person/projects/lyn')]);
  assert.equal(
    focusedTerminalCwd(
      {
        shellIntegration: { cwd: { scheme: 'file', fsPath: '/tmp/other-project' } },
      },
      false,
    ),
    undefined,
  );
});

test('terminal invoke uses live shell-integration cwd only when focused', () => {
  const terminal = {
    name: 'must-not-be-sent',
    creationOptions: { cwd: '/tmp/stale-after-cd' },
    shellIntegration: {
      cwd: { scheme: 'file', fsPath: '/home/person/worktree' },
    },
  };
  assert.deepEqual(
    invokeWorkspaceFolders({
      surface: 'terminal',
      workspaceFolders: [folder('file', '/home/person/projects/lyn')],
      terminal,
      terminalFocused: true,
    }),
    [path.normalize('/home/person/worktree')],
  );
  assert.deepEqual(
    invokeWorkspaceFolders({
      surface: 'terminal',
      workspaceFolders: [folder('file', '/home/person/projects/lyn')],
      terminal,
      terminalFocused: false,
    }),
    [],
  );
});

test('closed or remote terminals yield no cwd guess', () => {
  assert.deepEqual(
    invokeWorkspaceFolders({
      surface: 'terminal',
      terminalFocused: true,
      terminal: undefined,
    }),
    [],
  );
  assert.deepEqual(
    invokeWorkspaceFolders({
      surface: 'terminal',
      terminalFocused: true,
      terminal: {
        creationOptions: { cwd: '/tmp/stale-after-cd' },
        shellIntegration: {
          cwd: { scheme: 'vscode-remote', fsPath: '/remote/project' },
        },
      },
    }),
    [],
  );
});

test('v2 terminal invoke carries surface and no native ids', () => {
  const request = createInvokeRequest(
    instanceId,
    [folder('file', '/home/person/projects/lyn')],
    'c3b1a2d0-1111-4aaa-8bbb-0123456789ab',
    {
      surface: 'terminal',
      terminalFocused: true,
      terminal: {
        shellIntegration: {
          cwd: { scheme: 'file', fsPath: '/tmp/lyn-cl01-beta' },
        },
      },
    },
  );
  assert.equal(request.surface, 'terminal');
  assert.deepEqual(request.workspaceFolders, [
    path.normalize('/tmp/lyn-cl01-beta'),
  ]);
  assert.equal(request.cwd, undefined);
  assert.equal(request.pid, undefined);
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
