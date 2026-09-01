<script lang="ts">
  import ClockIcon from '@lucide/svelte/icons/clock-3';
  import FolderIcon from '@lucide/svelte/icons/folder-code';
  import LayersIcon from '@lucide/svelte/icons/layers-3';
  import LibraryIcon from '@lucide/svelte/icons/library';
  import MenuIcon from '@lucide/svelte/icons/menu';
  import NotebookIcon from '@lucide/svelte/icons/notebook-tabs';
  import RefreshIcon from '@lucide/svelte/icons/refresh-cw';
  import { onMount, tick } from 'svelte';

  import type {
    CaptureDetail,
    CaptureSummary,
    ContextRef,
    LibraryScope,
  } from '../lib/ipc-types';
  import CaptureDetailPanel from './CaptureDetailPanel.svelte';
  import CaptureStream from './CaptureStream.svelte';
  import {
    LibraryCommandError,
    libraryClient,
    type LibraryClient,
  } from './library-client';

  interface Props {
    client?: LibraryClient;
  }

  let { client = libraryClient }: Props = $props();
  let contexts = $state<ContextRef[]>([]);
  let scope = $state<LibraryScope>({ kind: 'recent' });
  let captures = $state<CaptureSummary[]>([]);
  let nextCursor = $state<string | null>(null);
  let selected = $state<CaptureDetail | null>(null);
  let selectedSummaryId = $state<string | null>(null);
  let branchName = $state<string | null>(null);
  let knownBranches = $state<string[]>([]);
  let loading = $state(true);
  let loadingMore = $state(false);
  let detailLoading = $state(false);
  let mediaBusy = $state(false);
  let playingMediaId = $state<string | null>(null);
  let error = $state<string | null>(null);
  let navigationOpen = $state(false);
  let listRequest = 0;
  let detailRequest = 0;

  const projects = $derived(
    contexts.filter((context) => context.kind === 'project'),
  );
  const standalone = $derived(
    contexts.filter((context) => context.kind === 'standalone'),
  );
  const activeContext = $derived.by(() => {
    if (scope.kind !== 'context') return null;
    const contextId = scope.contextId;
    return contexts.find((context) => context.id === contextId) ?? null;
  });
  const title = $derived(
    scope.kind === 'recent'
      ? 'Recent'
      : scope.kind === 'all'
        ? 'All captures'
        : (activeContext?.name ?? 'Context'),
  );

  onMount(() => {
    void initialise();
  });

  async function initialise() {
    loading = true;
    try {
      contexts = await client.listContexts();
      await loadCaptures(false);
    } catch (caught) {
      error = errorMessage(caught, 'The Library could not be loaded.');
      loading = false;
    }
  }

  async function chooseScope(nextScope: LibraryScope) {
    scope = nextScope;
    branchName = null;
    knownBranches = [];
    selected = null;
    selectedSummaryId = null;
    navigationOpen = false;
    await loadCaptures(false);
  }

  async function changeBranch(event: Event) {
    branchName = (event.currentTarget as HTMLSelectElement).value || null;
    selected = null;
    selectedSummaryId = null;
    await loadCaptures(false);
  }

  async function loadCaptures(append: boolean) {
    const request = ++listRequest;
    if (append) loadingMore = true;
    else loading = true;
    error = null;
    try {
      const page = await client.listCaptures(
        scope,
        { branchName },
        append ? nextCursor : null,
      );
      if (request !== listRequest) return;
      captures = append ? [...captures, ...page.items] : page.items;
      nextCursor = page.nextCursor;
      if (!branchName) {
        knownBranches = Array.from(
          new Set(
            captures.flatMap((capture) =>
              capture.branchName ? [capture.branchName] : [],
            ),
          ),
        ).sort((left, right) => left.localeCompare(right));
      }
    } catch (caught) {
      if (request === listRequest) {
        error = errorMessage(caught, 'Captures could not be loaded.');
      }
    } finally {
      if (request === listRequest) {
        loading = false;
        loadingMore = false;
      }
    }
  }

  async function inspectCapture(capture: CaptureSummary) {
    const request = ++detailRequest;
    selectedSummaryId = capture.id;
    detailLoading = true;
    error = null;
    try {
      const detail = await client.getCapture(capture.id);
      if (request === detailRequest) selected = detail;
    } catch (caught) {
      if (request === detailRequest) {
        selected = null;
        error = errorMessage(caught, 'The capture could not be opened.');
      }
    } finally {
      if (request === detailRequest) detailLoading = false;
    }
  }

  async function closeDetail() {
    const returnId = selectedSummaryId;
    selected = null;
    selectedSummaryId = null;
    await tick();
    if (returnId) document.getElementById(`capture-row-${returnId}`)?.focus();
  }

  async function toggleAudio() {
    const media = selected?.media;
    if (!media || mediaBusy) return;
    mediaBusy = true;
    error = null;
    try {
      if (playingMediaId === media.mediaId) {
        await client.stopPlayback(media.mediaId);
        playingMediaId = null;
      } else {
        await client.playMedia(media.mediaId);
        playingMediaId = media.mediaId;
      }
    } catch (caught) {
      error = errorMessage(caught, 'The voice note could not be played.');
    } finally {
      mediaBusy = false;
    }
  }

  async function openMedia() {
    const media = selected?.media;
    if (!media || mediaBusy) return;
    mediaBusy = true;
    error = null;
    try {
      await client.openMedia(media.mediaId);
    } catch (caught) {
      error = errorMessage(caught, 'The media could not be opened.');
    } finally {
      mediaBusy = false;
    }
  }

  function errorMessage(caught: unknown, fallback: string) {
    return caught instanceof LibraryCommandError ? caught.message : fallback;
  }

  function isCurrent(candidate: LibraryScope) {
    if (scope.kind !== candidate.kind) return false;
    return (
      scope.kind !== 'context' ||
      (candidate.kind === 'context' && scope.contextId === candidate.contextId)
    );
  }
</script>

<main class="library-shell" class:detail-open={selected || detailLoading}>
  <aside
    class:open={navigationOpen}
    class="library-navigation"
    aria-label="Library navigation"
  >
    <div class="library-brand">
      <LibraryIcon aria-hidden="true" /><span>Lyn</span>
    </div>
    <nav>
      <button
        class:active={isCurrent({ kind: 'recent' })}
        type="button"
        onclick={() => chooseScope({ kind: 'recent' })}
        ><ClockIcon aria-hidden="true" />Recent</button
      >
      <button
        class:active={isCurrent({ kind: 'all' })}
        type="button"
        onclick={() => chooseScope({ kind: 'all' })}
        ><LayersIcon aria-hidden="true" />All captures</button
      >

      {#if projects.length}
        <h2>Projects</h2>
        {#each projects as context (context.id)}
          <button
            class:active={isCurrent({ kind: 'context', contextId: context.id })}
            type="button"
            onclick={() =>
              chooseScope({ kind: 'context', contextId: context.id })}
            ><FolderIcon aria-hidden="true" /><span>{context.name}</span
            ></button
          >
        {/each}
      {/if}

      {#if standalone.length}
        <h2>Contexts</h2>
        {#each standalone as context (context.id)}
          <button
            class:active={isCurrent({ kind: 'context', contextId: context.id })}
            type="button"
            onclick={() =>
              chooseScope({ kind: 'context', contextId: context.id })}
            ><NotebookIcon aria-hidden="true" /><span>{context.name}</span
            ></button
          >
        {/each}
      {/if}
    </nav>
  </aside>

  <section class="library-stream" aria-labelledby="library-title">
    <header class="library-toolbar">
      <button
        class="navigation-toggle"
        type="button"
        aria-label="Toggle Library navigation"
        aria-expanded={navigationOpen}
        onclick={() => (navigationOpen = !navigationOpen)}
        ><MenuIcon aria-hidden="true" /></button
      >
      <div>
        <p>Library</p>
        <h1 id="library-title">{title}</h1>
      </div>
      {#if activeContext?.kind === 'project' && knownBranches.length}
        <label class="branch-filter">
          <span>Branch</span>
          <select value={branchName ?? ''} onchange={changeBranch}>
            <option value="">All branches</option>
            {#each knownBranches as branch}
              <option value={branch}>{branch}</option>
            {/each}
          </select>
        </label>
      {/if}
    </header>

    {#if error}
      <div class="library-error" role="alert">
        <span>{error}</span>
        <button type="button" onclick={() => loadCaptures(false)}
          ><RefreshIcon aria-hidden="true" />Retry</button
        >
      </div>
    {/if}

    <div class="stream-scroll-region" aria-busy={loading}>
      {#if loading}
        <p class="library-status">Loading captures…</p>
      {:else if captures.length === 0}
        <div class="library-empty">
          <h2>Nothing captured yet</h2>
          <p>
            Use <kbd>Ctrl</kbd> <kbd>Shift</kbd> <kbd>Space</kbd> to save your first
            note.
          </p>
        </div>
      {:else}
        <CaptureStream
          {captures}
          selectedId={selectedSummaryId}
          onselect={inspectCapture}
        />
        {#if nextCursor}
          <button
            class="load-more-button"
            type="button"
            disabled={loadingMore}
            onclick={() => loadCaptures(true)}
            >{loadingMore ? 'Loading…' : 'Load more'}</button
          >
        {/if}
      {/if}
    </div>
  </section>

  {#if detailLoading}
    <aside class="library-detail">
      <p class="library-status">Loading capture…</p>
    </aside>
  {:else if selected}
    <aside class="library-detail">
      <CaptureDetailPanel
        capture={selected}
        compact={true}
        playing={playingMediaId === selected.media?.mediaId}
        busy={mediaBusy}
        onback={closeDetail}
        onplay={toggleAudio}
        onopen={openMedia}
      />
    </aside>
  {:else}
    <aside class="library-detail detail-placeholder" aria-hidden="true">
      <p>Select a capture to inspect it.</p>
    </aside>
  {/if}
</main>
