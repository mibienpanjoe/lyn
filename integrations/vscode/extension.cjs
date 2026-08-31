const crypto = require('node:crypto');
const net = require('node:net');
const vscode = require('vscode');

const { createObservation, providerSocketPath } = require('./observation.cjs');

const HEARTBEAT_INTERVAL_MS = 10_000;
const SOCKET_TIMEOUT_MS = 750;

let activeProvider;

function sendObservation(observation) {
  const socketPath = providerSocketPath();
  if (!socketPath) {
    return Promise.resolve();
  }

  return new Promise((resolve) => {
    let completed = false;
    const complete = () => {
      if (completed) return;
      completed = true;
      clearTimeout(timeout);
      resolve();
    };
    const socket = net.createConnection(socketPath, () => {
      socket.end(JSON.stringify(observation));
    });
    const timeout = setTimeout(() => {
      socket.destroy();
      complete();
    }, SOCKET_TIMEOUT_MS);

    socket.once('error', complete);
    socket.once('close', complete);
  });
}

function activate(context) {
  const instanceId = crypto.randomUUID();
  let queue = Promise.resolve();

  const report = (state) => {
    const observation = createObservation(
      instanceId,
      state,
      vscode.workspace.workspaceFolders,
    );
    queue = queue
      .catch(() => undefined)
      .then(() => sendObservation(observation));
    return queue;
  };
  const reportCurrentState = () =>
    report(vscode.window.state.focused ? 'focused' : 'unfocused');

  context.subscriptions.push(
    vscode.window.onDidChangeWindowState(({ focused }) =>
      report(focused ? 'focused' : 'unfocused'),
    ),
    vscode.workspace.onDidChangeWorkspaceFolders(reportCurrentState),
  );

  const heartbeat = setInterval(reportCurrentState, HEARTBEAT_INTERVAL_MS);
  context.subscriptions.push({ dispose: () => clearInterval(heartbeat) });

  activeProvider = {
    end: () => report('ended'),
  };
  void reportCurrentState();
}

function deactivate() {
  const provider = activeProvider;
  activeProvider = undefined;
  return provider?.end();
}

module.exports = { activate, deactivate };
