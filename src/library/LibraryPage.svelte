<script lang="ts">
  import ClockIcon from '@lucide/svelte/icons/clock-3';
  import FolderIcon from '@lucide/svelte/icons/folder-code';
  import LayersIcon from '@lucide/svelte/icons/layers-3';
  import MenuIcon from '@lucide/svelte/icons/menu';
  import NotebookIcon from '@lucide/svelte/icons/notebook-tabs';
  import RefreshIcon from '@lucide/svelte/icons/refresh-cw';
  import SearchIcon from '@lucide/svelte/icons/search';
  import SettingsIcon from '@lucide/svelte/icons/settings-2';
  import { onDestroy, onMount, tick } from 'svelte';
  import logoUrl from '../../src-tauri/icons/lyn-icon.svg?url';

  import type {
    CaptureDetail,
    CaptureKind,
    CaptureSummary,
    ContextRef,
    LibraryScope,
  } from '../lib/ipc-types';
  import CaptureDetailPanel from './CaptureDetailPanel.svelte';
  import CaptureStream from './CaptureStream.svelte';
  import SearchFilters from './SearchFilters.svelte';
  import SettingsPanel from '../settings/SettingsPanel.svelte';
  import type { SettingsClient } from '../settings/settings-client';
  import {
    LibraryCommandError,
    libraryClient,
    type LibraryClient,
  } from './library-client';

  interface Props {
    client?: LibraryClient;
    settings?: SettingsClient;
  }

  let { client = libraryClient, settings }: Props = $props();
  let contexts = $state<ContextRef[]>([]);
  let scope = $state<LibraryScope>({ kind: 'recent' });
  let captures = $state<CaptureSummary[]>([]);
  let nextCursor = $state<string | null>(null);
  let selected = $state<CaptureDetail | null>(null);
  let selectedSummaryId = $state<string | null>(null);
  let branchName = $state<string | null>(null);
  let searchMode = $state(false);
  let settingsMode = $state(false);
  let query = $state('');
  let searchContextId = $state<string | null>(null);
  let captureKinds = $state<CaptureKind[]>([]);
  let capturedFromDate = $state('');
  let capturedToDate = $state('');
  let datePreset = $state<'any' | 'today' | '7-days' | '30-days' | 'custom'>(
    'any',
  );
  let snippets = $state<Record<string, string>>({});
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
  let searchInput = $state<HTMLInputElement>();
  let searchTimer: ReturnType<typeof setTimeout> | undefined;

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
    searchMode
      ? 'Search'
      : scope.kind === 'recent'
        ? 'Recent'
        : scope.kind === 'all'
          ? 'All captures'
          : (activeContext?.name ?? 'Context'),
  );
  const activeFilterCount = $derived(
    Number(Boolean(searchContextId)) +
      Number(Boolean(branchName)) +
      Number(captureKinds.length > 0) +
      Number(datePreset !== 'any'),
  );

  onMount(() => {
    void initialise();
  });

  onDestroy(() => clearTimeout(searchTimer));

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
    searchMode = false;
    settingsMode = false;
    scope = nextScope;
    branchName = null;
    knownBranches = [];
    selected = null;
    selectedSummaryId = null;
    navigationOpen = false;
    await loadCaptures(false);
  }

  async function chooseSearch() {
    searchMode = true;
    settingsMode = false;
    branchName = null;
    selected = null;
    selectedSummaryId = null;
    navigationOpen = false;
    await loadCaptures(false);
    await tick();
    searchInput?.focus();
  }

  function chooseSettings() {
    settingsMode = true;
    searchMode = false;
    selected = null;
    selectedSummaryId = null;
    navigationOpen = false;
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
      const cursor = append ? nextCursor : null;
      const searchQuery = query.trim();
      const searchScope: LibraryScope = searchContextId
        ? { kind: 'context', contextId: searchContextId }
        : { kind: 'all' };
      const filters = {
        branchName,
        captureKinds,
        capturedFrom: capturedFromDate ? `${capturedFromDate}T00:00:00Z` : null,
        capturedTo: capturedToDate ? `${capturedToDate}T23:59:59.999Z` : null,
      };
      const page =
        searchMode && searchQuery
          ? await client.searchCaptures(
              searchQuery,
              { ...filters, contextId: searchContextId },
              cursor,
            )
          : await client.listCaptures(
              searchMode ? searchScope : scope,
              searchMode ? filters : { branchName },
              cursor,
            );
      if (request !== listRequest) return;
      const pageCaptures = page.items.map((item) =>
        'capture' in item ? item.capture : item,
      );
      const pageSnippets = Object.fromEntries(
        page.items.flatMap((item) =>
          'capture' in item ? [[item.capture.id, item.snippet]] : [],
        ),
      );
      captures = append ? [...captures, ...pageCaptures] : pageCaptures;
      snippets = append ? { ...snippets, ...pageSnippets } : pageSnippets;
      nextCursor = page.nextCursor;
      if (!searchMode && !branchName) {
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

  function scheduleSearch(event: Event) {
    query = (event.currentTarget as HTMLInputElement).value;
    clearTimeout(searchTimer);
    searchTimer = setTimeout(() => void loadCaptures(false), 250);
  }

  function applySearchFilter() {
    clearTimeout(searchTimer);
    void loadCaptures(false);
  }

  function changeSearchContext(value: string) {
    searchContextId = value || null;
    applySearchFilter();
  }

  function changeSearchBranch(value: string) {
    branchName = value.trim() || null;
    applySearchFilter();
  }

  function changeSearchDate(value: string, boundary: 'from' | 'to') {
    if (boundary === 'from') capturedFromDate = value;
    else capturedToDate = value;
    applySearchFilter();
  }

  function changeDatePreset(value: typeof datePreset) {
    datePreset = value;
    if (datePreset === 'any') {
      capturedFromDate = '';
      capturedToDate = '';
    } else if (datePreset !== 'custom') {
      const today = new Date();
      const start = new Date(today);
      if (datePreset === '7-days') start.setDate(today.getDate() - 6);
      if (datePreset === '30-days') start.setDate(today.getDate() - 29);
      capturedFromDate = localDate(start);
      capturedToDate = localDate(today);
    }
    applySearchFilter();
  }

  function localDate(value: Date) {
    const year = value.getFullYear();
    const month = String(value.getMonth() + 1).padStart(2, '0');
    const day = String(value.getDate()).padStart(2, '0');
    return `${year}-${month}-${day}`;
  }

  function changeCaptureKind(kind: CaptureKind) {
    const selected = captureKinds.includes(kind);
    captureKinds = selected
      ? captureKinds.filter((candidate) => candidate !== kind)
      : [...captureKinds, kind];
    applySearchFilter();
  }

  function clearSearchFilters() {
    searchContextId = null;
    branchName = null;
    captureKinds = [];
    capturedFromDate = '';
    capturedToDate = '';
    datePreset = 'any';
    applySearchFilter();
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
    if (searchMode) return false;
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
      <img src={logoUrl} alt="" /><span>Lyn</span>
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
      <button class:active={searchMode} type="button" onclick={chooseSearch}
        ><SearchIcon aria-hidden="true" />Search</button
      >
      <button class:active={settingsMode} type="button" onclick={chooseSettings}
        ><SettingsIcon aria-hidden="true" />Settings</button
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

  {#if settingsMode}
    <SettingsPanel client={settings} />
  {:else}
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

      {#if searchMode}
        <div class="search-panel">
          <label class="search-field">
            <span class="sr-only">Search captures</span>
            <SearchIcon aria-hidden="true" />
            <input
              bind:this={searchInput}
              type="search"
              value={query}
              maxlength="200"
              placeholder="Search text and captions"
              oninput={scheduleSearch}
            />
          </label>
          <SearchFilters
            {contexts}
            contextId={searchContextId}
            {branchName}
            {captureKinds}
            {datePreset}
            {capturedFromDate}
            {capturedToDate}
            activeCount={activeFilterCount}
            oncontext={changeSearchContext}
            onbranch={changeSearchBranch}
            onkind={changeCaptureKind}
            onpreset={changeDatePreset}
            ondate={changeSearchDate}
            onclear={clearSearchFilters}
          />
          <p class="search-scope-note">
            Literal local search across note text and media captions.
          </p>
        </div>
      {/if}

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
            {#if searchMode && query.trim()}
              <h2>No matches</h2>
              <p>Try fewer words or adjust the filters.</p>
            {:else}
              <h2>Nothing captured yet</h2>
              <p>
                Press <kbd>Ctrl</kbd> <kbd>Shift</kbd> <kbd>Space</kbd> to make
                your first capture{activeContext
                  ? ` in ${activeContext.name}`
                  : ''}.
              </p>
            {/if}
          </div>
        {:else}
          <CaptureStream
            {captures}
            selectedId={selectedSummaryId}
            {snippets}
            query={searchMode ? query.trim() : ''}
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
          backLabel={title}
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
  {/if}
</main>
