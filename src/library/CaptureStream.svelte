<script lang="ts">
  import FileTextIcon from '@lucide/svelte/icons/file-text';
  import ImageIcon from '@lucide/svelte/icons/image';
  import MicIcon from '@lucide/svelte/icons/mic';

  import type { CaptureSummary } from '../lib/ipc-types';

  interface Props {
    captures: CaptureSummary[];
    selectedId?: string | null;
    snippets?: Record<string, string>;
    query?: string;
    onselect: (capture: CaptureSummary) => void;
  }

  let {
    captures,
    selectedId = null,
    snippets = {},
    query = '',
    onselect,
  }: Props = $props();
  const groups = $derived(groupByLocalDay(captures));

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

<div class="stream-groups">
  {#each groups as group (group.label)}
    <section class="stream-day" aria-labelledby={`day-${group.items[0].id}`}>
      <h2 id={`day-${group.items[0].id}`}>{group.label}</h2>
      <ul class="capture-list">
        {#each group.items as capture (capture.id)}
          <li>
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
          </li>
        {/each}
      </ul>
    </section>
  {/each}
</div>
