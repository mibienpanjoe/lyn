<script lang="ts">
  import CapturePopup from './capture/CapturePopup.svelte';
  import LibraryPage from './library/LibraryPage.svelte';

  const surface = new URLSearchParams(window.location.search).get('surface');
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
