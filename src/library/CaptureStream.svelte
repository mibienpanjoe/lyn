<script lang="ts">
  import AlertIcon from '@lucide/svelte/icons/circle-alert';
  import CheckIcon from '@lucide/svelte/icons/check';
  import CopyIcon from '@lucide/svelte/icons/copy';
  import FileTextIcon from '@lucide/svelte/icons/file-text';
  import ImageIcon from '@lucide/svelte/icons/image';
  import MicIcon from '@lucide/svelte/icons/mic';
  import Trash2Icon from '@lucide/svelte/icons/trash-2';

  import { getTranslations } from '../lib/i18n';
  import type { CaptureSummary, LanguageSetting } from '../lib/ipc-types';

  interface Props {
    captures: CaptureSummary[];
    selectedId?: string | null;
    snippets?: Record<string, string>;
    query?: string;
    language?: LanguageSetting;
    copyFeedback?: { id: string; state: 'copied' | 'failed' } | null;
    oldestFirst?: boolean;
    pendingDeleteId?: string | null;
    deleteBusy?: boolean;
    onselect: (capture: CaptureSummary) => void;
    oncopy?: (capture: CaptureSummary) => void;
    onrequestdelete?: (capture: CaptureSummary) => void;
    oncanceldelete?: () => void;
    onconfirmdelete?: (capture: CaptureSummary) => void;
  }

  let {
    captures,
    selectedId = null,
    snippets = {},
    query = '',
    language = 'english',
    copyFeedback = null,
    oldestFirst = false,
    pendingDeleteId = null,
    deleteBusy = false,
    onselect,
    oncopy,
    onrequestdelete,
    oncanceldelete,
    onconfirmdelete,
  }: Props = $props();
  // Captures arrive newest first; the conversation view reads oldest to newest.
  const ordered = $derived(oldestFirst ? [...captures].reverse() : captures);
  const groups = $derived(groupByLocalDay(ordered));
  const t = $derived(getTranslations(language));

  // Row excerpts are truncated, so the owning page resolves the exact body.
  function isCopyable(capture: CaptureSummary) {
    return (
      oncopy != null && (capture.kind === 'text' || capture.caption != null)
    );
  }

  function copyLabel(capture: CaptureSummary) {
    return `${t.copyText} — ${capture.context.name}, ${time(capture.capturedAt)}`;
  }

  function deleteLabel(capture: CaptureSummary) {
    return `${t.deleteCapture} — ${capture.context.name}, ${time(capture.capturedAt)}`;
  }

  function cancelOnEscape(event: KeyboardEvent) {
    if (event.key === 'Escape' && pendingDeleteId) {
      event.stopPropagation();
      oncanceldelete?.();
    }
  }

  // Confirming replaces the button that was just clicked, so focus must move.
  function focusOnMount(node: HTMLElement) {
    node.focus();
  }

  function groupByLocalDay(items: CaptureSummary[]) {
    const groups: Array<{ label: string; items: CaptureSummary[] }> = [];
    for (const capture of items) {
      const label = new Intl.DateTimeFormat(undefined, {
        dateStyle: 'medium',
      }).format(new Date(capture.capturedAt));
      const current = groups.at(-1);
      if (current?.label === label) current.items.push(capture);
      else groups.push({ label, items: [capture] });
    }
    return groups;
  }

  function time(value: string) {
    return new Intl.DateTimeFormat(undefined, {
      hour: '2-digit',
      minute: '2-digit',
    }).format(new Date(value));
  }

  function accessibleLabel(capture: CaptureSummary) {
    const content = displayText(capture);
    return `${capture.kind} capture in ${capture.context.name} at ${time(capture.capturedAt)}. ${content}`;
  }

  function displayText(capture: CaptureSummary) {
    return (
      snippets[capture.id] ??
      capture.textExcerpt ??
      capture.caption ??
      `${capture.kind} capture`
    );
  }

  function highlightedParts(value: string) {
    const terms = new Set(
      query
        .trim()
        .split(/\s+/)
        .filter(Boolean)
        .map((term) => term.toLocaleLowerCase()),
    );
    if (!terms.size) return [{ value, match: false }];
    const pattern = Array.from(terms)
      .map((term) => term.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'))
      .join('|');
    return value
      .split(new RegExp(`(${pattern})`, 'giu'))
      .filter(Boolean)
      .map((part) => ({
        value: part,
        match: terms.has(part.toLocaleLowerCase()),
      }));
  }
</script>

<svelte:window onkeydown={cancelOnEscape} />

<div class="stream-groups">
  {#each groups as group (group.label)}
    <section class="stream-day" aria-labelledby={`day-${group.items[0].id}`}>
      <h2 id={`day-${group.items[0].id}`}>{group.label}</h2>
      <ul class="capture-list">
        {#each group.items as capture (capture.id)}
          <li class="capture-row-item">
            <button
              id={`capture-row-${capture.id}`}
              class:selected={selectedId === capture.id}
              class="capture-row"
              type="button"
              aria-label={accessibleLabel(capture)}
              aria-current={selectedId === capture.id ? 'true' : undefined}
              onclick={() => onselect(capture)}
            >
              <span class="capture-kind-icon" aria-hidden="true">
                {#if capture.kind === 'text'}
                  <FileTextIcon />
                {:else if capture.kind === 'image'}
                  <ImageIcon />
                {:else}
                  <MicIcon />
                {/if}
              </span>
              <span class="capture-row-content">
                <span class="capture-row-meta">
                  <time datetime={capture.capturedAt}
                    >{time(capture.capturedAt)}</time
                  >
                  <span>{capture.context.name}</span>
                  {#if capture.branchName}
                    <span class="branch-label" title={capture.branchName}
                      >{capture.branchName}</span
                    >
                  {/if}
                </span>
                <span class="capture-excerpt">
                  {#each highlightedParts(displayText(capture)) as part}
                    {#if part.match}<mark>{part.value}</mark
                      >{:else}{part.value}{/if}
                  {/each}
                </span>
              </span>
              {#if capture.kind === 'image' && capture.media?.available}
                <img
                  class="capture-thumbnail media-preview"
                  src={capture.media.previewUri}
                  alt=""
                  loading="lazy"
                />
              {/if}
            </button>
            <div class="capture-row-actions">
              {#if pendingDeleteId === capture.id}
                <div
                  class="row-delete-confirm"
                  role="group"
                  aria-label={deleteLabel(capture)}
                >
                  <button
                    class="row-confirm-delete"
                    type="button"
                    disabled={deleteBusy}
                    onclick={() => onconfirmdelete?.(capture)}
                    use:focusOnMount
                    >{deleteBusy ? t.deleting : t.confirmDelete}</button
                  >
                  <button
                    class="row-cancel-delete"
                    type="button"
                    disabled={deleteBusy}
                    onclick={() => oncanceldelete?.()}>{t.cancelDelete}</button
                  >
                </div>
              {:else}
                {#if isCopyable(capture)}
                  {@const feedback =
                    copyFeedback?.id === capture.id ? copyFeedback.state : null}
                  <button
                    class:copied={feedback === 'copied'}
                    class:failed={feedback === 'failed'}
                    class="capture-row-action"
                    type="button"
                    aria-label={copyLabel(capture)}
                    title={feedback === 'copied'
                      ? t.copyTextCopied
                      : feedback === 'failed'
                        ? t.copyTextFailed
                        : t.copyText}
                    onclick={() => oncopy?.(capture)}
                  >
                    {#if feedback === 'copied'}
                      <CheckIcon aria-hidden="true" />
                    {:else if feedback === 'failed'}
                      <AlertIcon aria-hidden="true" />
                    {:else}
                      <CopyIcon aria-hidden="true" />
                    {/if}
                  </button>
                {:else}
                  <span class="capture-row-action-spacer" aria-hidden="true"
                  ></span>
                {/if}
                {#if onrequestdelete}
                  <button
                    class="capture-row-action capture-row-delete"
                    type="button"
                    aria-label={deleteLabel(capture)}
                    title={t.deleteCapture}
                    onclick={() => onrequestdelete?.(capture)}
                  >
                    <Trash2Icon aria-hidden="true" />
                  </button>
                {/if}
              {/if}
            </div>
          </li>
        {/each}
      </ul>
    </section>
  {/each}
  <span class="sr-only" role="status">
    {copyFeedback?.state === 'copied'
      ? t.copyTextCopied
      : copyFeedback?.state === 'failed'
        ? t.copyTextFailed
        : ''}
  </span>
</div>
