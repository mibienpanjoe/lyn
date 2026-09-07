// Lyn Browser Context Companion (Manifest V3)
const HOST_NAME = 'com.mibienpanjoe.lyn';

let instanceId = null;

function getInstanceId() {
  if (!instanceId) {
    instanceId = crypto.randomUUID();
  }
  return instanceId;
}

function sanitizeUrl(rawUrl) {
  if (typeof rawUrl !== 'string' || !rawUrl) {
    return null;
  }

  try {
    const parsed = new URL(rawUrl);
    if (
      parsed.protocol !== 'http:' &&
      parsed.protocol !== 'https:' &&
      parsed.protocol !== 'file:'
    ) {
      return null;
    }

    if (parsed.protocol === 'file:') {
      return `file://${parsed.pathname}`;
    }

    const portPart = parsed.port ? `:${parsed.port}` : '';
    return `${parsed.protocol}//${parsed.hostname}${portPart}${parsed.pathname}`;
  } catch {
    return null;
  }
}

function sendObservation(state, tab = {}) {
  if (tab.incognito) {
    return;
  }

  const cleanUrl = state === 'ended' ? null : sanitizeUrl(tab.url);

  const payload = {
    version: 1,
    instanceId: getInstanceId(),
    state,
    url: cleanUrl,
    title: state === 'ended' ? null : tab.title || null,
    incognito: false,
  };

  chrome.runtime.sendNativeMessage(HOST_NAME, payload, () => {
    // Ignore runtime errors if desktop host is offline
    void chrome.runtime.lastError;
  });
}

function reportActiveTab(state = 'focused') {
  chrome.tabs.query({ active: true, currentWindow: true }, (tabs) => {
    if (tabs && tabs[0]) {
      sendObservation(state, tabs[0]);
    }
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

// Initial report on startup
reportActiveTab('focused');
