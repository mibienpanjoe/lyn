// Lyn Browser Context Companion (Manifest V3)
if (typeof importScripts === 'function') {
  importScripts('sanitize.js');
}

const HOST_NAME = 'com.mibienpanjoe.lyn';

let instanceId = null;

function getInstanceId() {
  if (!instanceId) {
    instanceId = crypto.randomUUID();
  }
  return instanceId;
}

function sendNative(payload) {
  chrome.runtime.sendNativeMessage(HOST_NAME, payload, () => {
    // Ignore runtime errors if desktop host is offline
    void chrome.runtime.lastError;
  });
}

function sendObservation(state, tab = {}) {
  sendNative(createBrowserObservation(getInstanceId(), state, tab));
}

function reportActiveTab(state = 'focused') {
  chrome.tabs.query({ active: true, currentWindow: true }, (tabs) => {
    if (tabs && tabs[0]) {
      sendObservation(state, tabs[0]);
    }
  });
}

function invokeFromFocusedTab() {
  chrome.tabs.query({ active: true, lastFocusedWindow: true }, (tabs) => {
    sendNative(createBrowserInvoke(getInstanceId(), (tabs && tabs[0]) || {}));
  });
}

chrome.windows.onFocusChanged.addListener((windowId) => {
  if (windowId === chrome.windows.WINDOW_ID_NONE) {
    reportActiveTab('unfocused');
  } else {
    reportActiveTab('focused');
  }
});

chrome.tabs.onActivated.addListener(() => {
  reportActiveTab('focused');
});

chrome.tabs.onUpdated.addListener((tabId, changeInfo, tab) => {
  if (tab.active && (changeInfo.status === 'complete' || changeInfo.url)) {
    sendObservation('focused', tab);
  }
});

chrome.commands.onCommand.addListener((command) => {
  if (command === 'lyn.capture') {
    invokeFromFocusedTab();
  }
});

// Initial report on startup
reportActiveTab('focused');
