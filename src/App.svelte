<script lang="ts">
  import { onMount } from 'svelte';

  import CapturePopup from './capture/CapturePopup.svelte';
  import LibraryPage from './library/LibraryPage.svelte';
  import { applyTheme, settingsClient } from './settings/settings-client';

  const surface = new URLSearchParams(window.location.search).get('surface');

  onMount(() => {
    void settingsClient
      .get()
      .then((settings) => applyTheme(settings.theme))
      .catch(() => undefined);
  });
</script>

<svelte:head>
  <meta
    name="description"
    content="Lyn is a local-first desktop working-memory companion."
  />
</svelte:head>

{#if surface === 'capture'}
  <CapturePopup />
{:else}
  <LibraryPage />
{/if}
