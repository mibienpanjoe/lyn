export function isLinuxPlatform(): boolean {
  if (typeof navigator === 'undefined') return false;
  const ua = navigator.userAgent || '';
  const plat = (navigator as { platform?: string }).platform || '';
  return /linux/i.test(ua) || /linux/i.test(plat);
}
