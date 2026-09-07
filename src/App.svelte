<script lang="ts">
  import { onMount } from 'svelte';
  import { isTauri } from '@tauri-apps/api/core';

  import CapturePopup from './capture/CapturePopup.svelte';
  import LibraryPage from './library/LibraryPage.svelte';
  import { applyTheme, settingsClient } from './settings/settings-client';
  import {
    devCaptureClient,
    devIntegrationClient,
    devLibraryClient,
    devSettingsClient,
    devSpeechModelClient,
  } from './lib/dev-mocks';

  const surface = new URLSearchParams(window.location.search).get('surface');
  const tauriActive = isTauri();

  onMount(() => {
    const client = tauriActive ? settingsClient : devSettingsClient;
    void client
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
  <CapturePopup client={tauriActive ? undefined : devCaptureClient} />
{:else}
  <LibraryPage
    client={tauriActive ? undefined : devLibraryClient}
    settings={tauriActive ? undefined : devSettingsClient}
    modelClient={tauriActive ? undefined : devSpeechModelClient}
    intClient={tauriActive ? undefined : devIntegrationClient}
  />
{/if}
