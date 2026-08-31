<script lang="ts">
  import type { ContextResolution } from '../lib/ipc-types';

  interface Props {
    resolution: ContextResolution | null;
    open: boolean;
    stale?: boolean;
    button?: HTMLButtonElement;
    onclick: () => void;
  }

  let {
    resolution,
    open,
    stale = false,
    button = $bindable(),
    onclick,
  }: Props = $props();
  const resolved = $derived(
    resolution?.state === 'resolved' ? resolution.candidate : null,
  );
  const needsAttention = $derived(!resolved || stale);
  const label = $derived(
    stale
      ? 'Context source stale. Choose another context'
      : resolution?.state === 'ambiguous'
        ? 'Context ambiguous. Choose context'
        : resolved
          ? `${resolved.context.name}${resolved.branchName ? `, branch ${resolved.branchName}` : ''}`
          : 'Choose context',
  );
</script>

<button
  bind:this={button}
  class:context-attention={needsAttention}
  class="context-control"
  type="button"
  aria-expanded={open}
  aria-controls="context-chooser"
  aria-label={resolved && !stale ? `Context ${label}. Change context` : label}
  {onclick}
>
  {#if needsAttention}
    <span class="context-status-dot" aria-hidden="true"></span>
  {/if}
  <svg class="context-icon" viewBox="0 0 16 16" aria-hidden="true">
    {#if stale || resolution?.state === 'ambiguous'}
      <path d="M8 2.2 14 13H2L8 2.2Z M8 6v3.2 M8 11.5v.2" />
    {:else}
      <rect x="3" y="3" width="10" height="10" rx="2" />
    {/if}
  </svg>
  <span class="context-label">{label}</span>
  <svg class="context-chevron" viewBox="0 0 16 16" aria-hidden="true">
    <path d="m4 6 4 4 4-4" />
  </svg>
</button>
