const crypto = require('node:crypto');
const path = require('node:path');

const SOCKET_NAME = 'lyn-context-v1.sock';
const SUPPORTED_STATES = new Set(['focused', 'unfocused', 'ended']);

function providerSocketPath(
  environment = process.env,
  platform = process.platform,
) {
  const runtimeDirectory = environment.XDG_RUNTIME_DIR;
  if (
    platform !== 'linux' ||
    typeof runtimeDirectory !== 'string' ||
    !path.isAbsolute(runtimeDirectory)
  ) {
    return undefined;
  }

  return path.join(runtimeDirectory, SOCKET_NAME);
}

function localFilePath(uri) {
  if (
    uri?.scheme !== 'file' ||
    typeof uri.fsPath !== 'string' ||
    uri.fsPath.length === 0
  ) {
    return undefined;
  }
  return path.normalize(uri.fsPath);
}

function localWorkspacePaths(workspaceFolders = []) {
  const paths = workspaceFolders
    .map((folder) => localFilePath(folder?.uri))
    .filter((workspacePath) => typeof workspacePath === 'string');

  return [...new Set(paths)];
}

// Live cwd from shell integration only. creationOptions.cwd is the initial
// directory and goes stale after `cd`; never send names, titles, or output.
function focusedTerminalCwd(terminal, terminalFocused) {
  if (!terminalFocused || !terminal) {
    return undefined;
  }
  return localFilePath(terminal.shellIntegration?.cwd);
}

function invokeWorkspaceFolders({
  surface = 'editor',
  workspaceFolders = [],
  terminal,
  terminalFocused = false,
} = {}) {
  if (surface === 'terminal') {
    const cwd = focusedTerminalCwd(terminal, terminalFocused);
    return cwd ? [cwd] : [];
  }
  return localWorkspacePaths(workspaceFolders);
}

function createObservation(instanceId, state, workspaceFolders) {
  if (typeof instanceId !== 'string' || !SUPPORTED_STATES.has(state)) {
    throw new TypeError('invalid VS Code provider observation');
  }

  return {
    version: 1,
    instanceId,
    state,
    workspaceFolders:
      state === 'ended' ? [] : localWorkspacePaths(workspaceFolders),
  };
}

function createInvokeRequest(
  instanceId,
  workspaceFolders,
  requestId,
  options = {},
) {
  const id =
    typeof requestId === 'string' && requestId.length > 0
      ? requestId
      : crypto.randomUUID();
  if (typeof instanceId !== 'string') {
    throw new TypeError('invalid VS Code provider invoke request');
  }
  const surface = options.surface === 'terminal' ? 'terminal' : 'editor';

  return {
    version: 2,
    kind: 'invoke',
    instanceId,
    requestId: id,
    surface,
    workspaceFolders: invokeWorkspaceFolders({
      surface,
      workspaceFolders,
      terminal: options.terminal,
      terminalFocused: options.terminalFocused,
    }),
  };
}

module.exports = {
  createInvokeRequest,
  createObservation,
  focusedTerminalCwd,
  invokeWorkspaceFolders,
  localWorkspacePaths,
  providerSocketPath,
};
