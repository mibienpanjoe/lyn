export async function copyTextToClipboard(text: string): Promise<boolean> {
  if (!text) return false;
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    // WebKitGTK can reject the async clipboard API even inside a user gesture.
    return copyWithSelection(text);
  }
}

function copyWithSelection(text: string): boolean {
  const holder = document.createElement('textarea');
  holder.value = text;
  holder.setAttribute('readonly', '');
  holder.setAttribute('aria-hidden', 'true');
  holder.style.position = 'fixed';
  holder.style.opacity = '0';
  document.body.append(holder);
  try {
    holder.select();
    return document.execCommand('copy');
  } catch {
    return false;
  } finally {
    holder.remove();
  }
}
