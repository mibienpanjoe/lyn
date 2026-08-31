<script lang="ts">
  import { onMount } from 'svelte';
  import type { ContextRef, ContextSourceOption } from '../lib/ipc-types';

  interface Props {
    liveSources: ContextSourceOption[];
    savedContexts: ContextRef[];
    loading: boolean;
    creating: boolean;
    onselectlive: (source: ContextSourceOption) => void;
    onselectsaved: (context: ContextRef) => void;
    oncreate: (name: string) => Promise<boolean>;
    onclose: () => void;
  }

  let {
    liveSources,
    savedContexts,
    loading,
    creating,
    onselectlive,
    onselectsaved,
    oncreate,
    onclose,
  }: Props = $props();
  let query = $state('');
  let contextName = $state('');
  let searchInput = $state<HTMLInputElement>();
  const normalized = $derived(query.trim().toLocaleLowerCase());
  const filteredLive = $derived(
    liveSources.filter(
      (source) =>
        !normalized ||
        `${source.applicationName} ${source.label}`
          .toLocaleLowerCase()
          .includes(normalized),
    ),
  );
  const filteredSaved = $derived(
    savedContexts.filter(
      (context) =>
        !normalized || context.name.toLocaleLowerCase().includes(normalized),
    ),
  );

  onMount(() => searchInput?.focus());
</script>

<section
  id="context-chooser"
  class="context-chooser"
  aria-label="Choose context"
>
  <div class="chooser-toolbar">
    <label class="sr-only" for="context-search">Search contexts</label>
    <input
      bind:this={searchInput}
      bind:value={query}
      id="context-search"
      class="context-search"
      type="search"
      placeholder="Search contexts"
      autocomplete="off"
    />
    <button class="chooser-close" type="button" onclick={onclose}
      >Back to note</button
    >
  </div>

  {#if loading}
    <p class="chooser-status" role="status">Loading contexts…</p>
  {:else}
    {#if filteredLive.length > 0}
      <h2 class="chooser-label">Live sessions</h2>
      <div class="context-list" aria-label="Live sessions">
        {#each filteredLive as source (source.sourceId)}
          <button
            class="context-option"
            type="button"
            aria-label={`Use live context ${source.label}${source.branchName ? `, branch ${source.branchName}` : ''}${source.isForeground ? ', current window' : ''}`}
            onclick={() => onselectlive(source)}
          >
            <span class="option-copy">
              <span>{source.label}</span>
              <span class="context-kind"
                >{source.applicationName}{source.branchName
                  ? ` · ${source.branchName}`
                  : ''}</span
              >
            </span>
            {#if source.isForeground}<span class="source-status"
                >Current window</span
              >{/if}
          </button>
        {/each}
      </div>
    {/if}

    {#if filteredSaved.length > 0}
      <h2 class="chooser-label">Saved contexts</h2>
      <div class="context-list" aria-label="Saved contexts">
        {#each filteredSaved as context (context.id)}
          <button
            class="context-option"
            type="button"
            aria-label={`Use context ${context.name}`}
            onclick={() => onselectsaved(context)}
          >
            <span>{context.name}</span><span class="context-kind"
              >{context.kind}</span
            >
          </button>
        {/each}
      </div>
    {/if}

    {#if filteredLive.length === 0 && filteredSaved.length === 0}
      <p class="chooser-status" role="status">No matching contexts.</p>
    {/if}
  {/if}

  <form
    class="context-create"
    onsubmit={async (event) => {
      event.preventDefault();
      if (contextName.trim()) {
        if (await oncreate(contextName.trim())) contextName = '';
      }
    }}
  >
    <label for="context-name">New context</label>
    <div class="context-create-row">
      <input
        bind:value={contextName}
        id="context-name"
        name="context-name"
        aria-label="New context name"
        maxlength="100"
        autocomplete="off"
      />
      <button type="submit" disabled={creating || !contextName.trim()}
        >{creating ? 'Creating…' : 'Create context'}</button
      >
    </div>
  </form>
</section>
