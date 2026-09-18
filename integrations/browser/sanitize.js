// Local, privacy-bounded URL sanitization for Lyn browser observations.
// Classic script (importScripts) and Node module.

function sanitizeUrl(rawUrl) {
  if (typeof rawUrl !== 'string' || !rawUrl) {
    return null;
  }

  try {
    const parsed = new URL(rawUrl);
    // Only accept http, https, and file schemes
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

    // Strip search query params and hash to prevent token/credential leakage
    const portPart = parsed.port ? `:${parsed.port}` : '';
    return `${parsed.protocol}//${parsed.hostname}${portPart}${parsed.pathname}`;
  } catch {
    return null;
  }
}

function isLocalContextHost(hostname) {
  return (
    hostname === 'localhost' ||
    hostname === '127.0.0.1' ||
    hostname === '0.0.0.0' ||
    hostname === '::1' ||
    hostname === '[::1]'
  );
}

// Remote sites are never correlation hints. Only file:// and localhost-family
// URLs may leave the extension, already stripped of query and fragment.
function localContextUrl(rawUrl) {
  const clean = sanitizeUrl(rawUrl);
  if (!clean) {
    return null;
  }
  if (clean.startsWith('file://')) {
    return clean;
  }
  try {
    return isLocalContextHost(new URL(clean).hostname) ? clean : null;
  } catch {
    return null;
  }
}

function createBrowserObservation(instanceId, state, tab = {}) {
  if (typeof instanceId !== 'string') {
    throw new TypeError('invalid instanceId');
  }

  if (tab.incognito) {
    return {
      version: 1,
      instanceId,
      state: 'ended',
      url: null,
      incognito: true,
    };
  }

  return {
    version: 1,
    instanceId,
    state,
    url: state === 'ended' ? null : localContextUrl(tab.url),
    incognito: false,
  };
}

function createBrowserInvoke(instanceId, tab = {}, requestId) {
  if (typeof instanceId !== 'string') {
    throw new TypeError('invalid instanceId');
  }
  const id =
    typeof requestId === 'string' && requestId.length > 0
      ? requestId
      : crypto.randomUUID();

  if (tab.incognito) {
    return {
      version: 2,
      kind: 'invoke',
      instanceId,
      requestId: id,
      url: null,
      incognito: true,
    };
  }

  return {
    version: 2,
    kind: 'invoke',
    instanceId,
    requestId: id,
    url: localContextUrl(tab.url),
    incognito: false,
  };
}

if (typeof module !== 'undefined' && module.exports) {
  module.exports = {
    createBrowserInvoke,
    createBrowserObservation,
    isLocalContextHost,
    localContextUrl,
    sanitizeUrl,
  };
}
