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

function localWorkspacePaths(workspaceFolders = []) {
  const paths = workspaceFolders
    .filter((folder) => folder?.uri?.scheme === 'file')
    .map((folder) => folder.uri.fsPath)
    .filter(
      (workspacePath) =>
        typeof workspacePath === 'string' && workspacePath.length > 0,
    )
    .map((workspacePath) => path.normalize(workspacePath));

  return [...new Set(paths)];
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

module.exports = {
  createObservation,
  localWorkspacePaths,
  providerSocketPath,
};
