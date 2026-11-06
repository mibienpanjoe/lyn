<script lang="ts">
  import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
  import FolderIcon from '@lucide/svelte/icons/folder-code';
  import GitBranchIcon from '@lucide/svelte/icons/git-branch';
  import NotebookIcon from '@lucide/svelte/icons/notebook-tabs';
  import TriangleAlertIcon from '@lucide/svelte/icons/triangle-alert';

  import type {
    ContextProviderKind,
    ContextResolution,
  } from '../lib/ipc-types';

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

  // Only providers that identify a concrete application are shown; generic
  // window and manual selections carry no extra meaning for the user.
  const providerLabels: Partial<Record<ContextProviderKind, string>> = {
    vscode: 'VS Code',
    cursor: 'Cursor',
    browser: 'Browser',
    shell: 'Terminal',
  };

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
  const visiblePrompt = $derived(
    stale
      ? 'Source changed — choose another context'
      : resolution?.state === 'ambiguous'
        ? 'Several contexts match — choose one'
        : 'Choose a context',
  );
  const providerLabel = $derived(
    resolved ? providerLabels[resolved.provider] : undefined,
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
  {#if resolved && !stale}
    <span class="context-chip" aria-hidden="true">
      {#if resolved.context.kind === 'project'}
        <FolderIcon class="context-icon" />
      {:else}
        <NotebookIcon class="context-icon" />
      {/if}
      <span class="context-label">{resolved.context.name}</span>
    </span>
    {#if resolved.branchName}
      <span
        class="context-chip context-branch"
        title={resolved.branchName}
        aria-hidden="true"
      >
        <GitBranchIcon class="context-icon" />
        <span class="context-branch-name">{resolved.branchName}</span>
      </span>
    {/if}
    {#if providerLabel}
      <span class="context-provider" aria-hidden="true">{providerLabel}</span>
    {/if}
  {:else}
    {#if stale || resolution?.state === 'ambiguous'}
      <TriangleAlertIcon class="context-icon" aria-hidden="true" />
    {/if}
    <span class="context-label" aria-hidden="true">{visiblePrompt}</span>
  {/if}
  <ChevronDownIcon class="context-chevron" aria-hidden="true" />
</button>
