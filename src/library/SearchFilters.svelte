<script lang="ts">
  import SlidersIcon from '@lucide/svelte/icons/sliders-horizontal';
  import { onMount } from 'svelte';

  import type { CaptureKind, ContextRef } from '../lib/ipc-types';

  type DatePreset = 'any' | 'today' | '7-days' | '30-days' | 'custom';

  interface Props {
    contexts: ContextRef[];
    contextId: string | null;
    branchName: string | null;
    captureKinds: CaptureKind[];
    datePreset: DatePreset;
    capturedFromDate: string;
    capturedToDate: string;
    activeCount: number;
    oncontext: (value: string) => void;
    onbranch: (value: string) => void;
    onkind: (kind: CaptureKind) => void;
    onpreset: (value: DatePreset) => void;
    ondate: (value: string, boundary: 'from' | 'to') => void;
    onclear: () => void;
  }

  let {
    contexts,
    contextId,
    branchName,
    captureKinds,
    datePreset,
    capturedFromDate,
    capturedToDate,
    activeCount,
    oncontext,
    onbranch,
    onkind,
    onpreset,
    ondate,
    onclear,
  }: Props = $props();
  let filters = $state<HTMLDetailsElement>();
  let summary = $state<HTMLElement>();

  onMount(() => {
    function closeFromOutside(event: PointerEvent) {
      if (
        filters?.open &&
        event.target instanceof Node &&
        !filters.contains(event.target)
      ) {
        filters.open = false;
      }
    }

    function closeFromEscape(event: KeyboardEvent) {
      if (filters?.open && event.key === 'Escape') {
        event.preventDefault();
        filters.open = false;
        summary?.focus();
      }
    }

    document.addEventListener('pointerdown', closeFromOutside);
    document.addEventListener('keydown', closeFromEscape);
    return () => {
      document.removeEventListener('pointerdown', closeFromOutside);
      document.removeEventListener('keydown', closeFromEscape);
    };
  });
</script>

<details class="search-filters" bind:this={filters}>
  <summary bind:this={summary}>
    <SlidersIcon aria-hidden="true" />
    {activeCount ? `Filters · ${activeCount}` : 'Filters'}
  </summary>
  <div class="filter-grid">
    <label>
      <span>Context</span>
      <select
        value={contextId ?? ''}
        onchange={(event) =>
          oncontext((event.currentTarget as HTMLSelectElement).value)}
      >
        <option value="">All contexts</option>
        {#each contexts as context (context.id)}
          <option value={context.id}>{context.name}</option>
        {/each}
      </select>
    </label>
    <label>
      <span>Branch</span>
      <input
        type="text"
        maxlength="255"
        value={branchName ?? ''}
        placeholder="Any branch"
        onchange={(event) =>
          onbranch((event.currentTarget as HTMLInputElement).value)}
      />
    </label>
    <label class="date-preset">
      <span>Date</span>
      <select
        value={datePreset}
        onchange={(event) =>
          onpreset(
            (event.currentTarget as HTMLSelectElement).value as DatePreset,
          )}
      >
        <option value="any">Any time</option>
        <option value="today">Today</option>
        <option value="7-days">Last 7 days</option>
        <option value="30-days">Last 30 days</option>
        <option value="custom">Custom range</option>
      </select>
    </label>
    {#if datePreset === 'custom'}
      <label>
        <span>From</span>
        <input
          type="date"
          value={capturedFromDate}
          onchange={(event) =>
            ondate((event.currentTarget as HTMLInputElement).value, 'from')}
        />
      </label>
      <label>
        <span>To</span>
        <input
          type="date"
          value={capturedToDate}
          onchange={(event) =>
            ondate((event.currentTarget as HTMLInputElement).value, 'to')}
        />
      </label>
    {/if}
    <fieldset>
      <legend>Capture type</legend>
      {#each ['text', 'image', 'audio'] as kind}
        <button
          class="kind-filter"
          class:active={captureKinds.includes(kind as CaptureKind)}
          type="button"
          aria-pressed={captureKinds.includes(kind as CaptureKind)}
          onclick={() => onkind(kind as CaptureKind)}
          >{kind[0].toUpperCase() + kind.slice(1)}</button
        >
      {/each}
    </fieldset>
    {#if activeCount}
      <button class="clear-filters" type="button" onclick={onclear}
        >Clear filters</button
      >
    {/if}
  </div>
</details>
