// Local, privacy-bounded URL sanitization for Lyn browser observations.

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
      title: null,
      incognito: true,
    };
  }

  const cleanUrl = state === 'ended' ? null : sanitizeUrl(tab.url);

  return {
    version: 1,
    instanceId,
    state,
    url: cleanUrl,
    title: state === 'ended' ? null : tab.title || null,
    incognito: false,
  };
}

module.exports = {
  createBrowserObservation,
  sanitizeUrl,
};
