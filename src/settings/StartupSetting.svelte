<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';

  let {
    french = false,
    call = invoke,
  }: {
    french?: boolean;
    call?: <T>(command: string) => Promise<T>;
  } = $props();
  let enabled = $state<boolean | null>(null);
  let busy = $state(false);
  let failed = $state(false);

  async function refresh() {
    busy = true;
    failed = false;
    try {
      enabled = await call<boolean>('plugin:autostart|is_enabled');
    } catch {
      enabled = null;
      failed = true;
    } finally {
      busy = false;
    }
  }

  async function toggle() {
    if (busy || enabled === null) return;
    busy = true;
    failed = false;
    const expected = !enabled;
    try {
      await call<void>(`plugin:autostart|${expected ? 'enable' : 'disable'}`);
      const actual = await call<boolean>('plugin:autostart|is_enabled');
      enabled = actual;
      failed = actual !== expected;
    } catch {
      // A registration error may follow a write. Re-read before offering retry.
      try {
        enabled = await call<boolean>('plugin:autostart|is_enabled');
      } catch {
        enabled = null;
      }
      failed = true;
    } finally {
      busy = false;
    }
  }

  onMount(() => {
    void refresh();
  });
</script>

<section class="settings-section" aria-labelledby="startup-title">
  <div>
    <h2 id="startup-title">{french ? 'Démarrage' : 'Startup'}</h2>
    <p>
      {french
        ? 'Lyn démarre en arrière-plan à la connexion pour rendre les raccourcis disponibles.'
        : 'Start Lyn in the background when you log in so capture shortcuts are available.'}
    </p>
  </div>
  <button
    type="button"
    class="secondary-action"
    role="switch"
    aria-checked={enabled === true}
    disabled={busy || enabled === null}
    onclick={toggle}
  >
    {french ? 'Démarrer Lyn à la connexion' : 'Start Lyn when I log in'}
    — {enabled === null
      ? french
        ? 'Indisponible'
        : 'Unavailable'
      : enabled
        ? french
          ? 'Activé'
          : 'On'
        : french
          ? 'Désactivé'
          : 'Off'}
  </button>
  {#if failed}
    <p role="alert">
      {french
        ? 'Impossible de confirmer le démarrage automatique.'
        : 'Could not confirm automatic startup.'}
    </p>
    <button
      class="secondary-action"
      type="button"
      disabled={busy}
      onclick={refresh}>{french ? 'Vérifier à nouveau' : 'Check again'}</button
    >
  {/if}
</section>
