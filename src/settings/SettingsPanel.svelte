<script lang="ts">
  import ArrowDownIcon from '@lucide/svelte/icons/arrow-down';
  import ArrowUpIcon from '@lucide/svelte/icons/arrow-up';
  import CheckIcon from '@lucide/svelte/icons/check';
  import { onDestroy, onMount } from 'svelte';

  import type {
    AppSettings,
    ContextProviderKind,
    ThemeSetting,
    SpeechModelStatus,
  } from '../lib/ipc-types';
  import {
    SettingsCommandError,
    applyTheme,
    settingsClient,
    type SettingsClient,
  } from './settings-client';
  import { speechModelClient, type SpeechModelClient } from './model-client';

  interface Props {
    client?: SettingsClient;
    modelClient?: SpeechModelClient;
  }

  let { client = settingsClient, modelClient = speechModelClient }: Props =
    $props();
  let saved = $state<AppSettings | null>(null);
  let draft = $state<AppSettings | null>(null);
  let loading = $state(true);
  let saving = $state(false);
  let error = $state<string | null>(null);
  let savedNotice = $state(false);
  let model = $state<SpeechModelStatus | null>(null);
  let modelBusy = $state(false);
  let unsubscribeModel: (() => void) | null = null;

  const providerNames: Record<ContextProviderKind, string> = {
    manual: 'Manual selection',
    vscode: 'VS Code',
    shell: 'Terminal',
    foreground_window: 'Foreground window',
  };
  const dirty = $derived(
    saved !== null &&
      draft !== null &&
      (saved.globalShortcut !== draft.globalShortcut ||
        saved.theme !== draft.theme ||
        saved.localSpeechEnabled !== draft.localSpeechEnabled ||
        saved.providerTieBreakOrder.some(
          (provider, index) => provider !== draft?.providerTieBreakOrder[index],
        )),
  );

  onMount(() => {
    void load();
    void loadModel();
    void modelClient
      .subscribe((status) => (model = status))
      .then((unsubscribe) => {
        unsubscribeModel = unsubscribe;
      })
      .catch(() => {});
  });
  onDestroy(() => {
    unsubscribeModel?.();
    if (saved) applyTheme(saved.theme);
  });

  async function loadModel() {
    try {
      model = await modelClient.status();
    } catch (caught) {
      error = message(caught, 'Local speech status could not be loaded.');
    }
  }

  async function changeModel(action: 'install' | 'cancel' | 'remove') {
    if (modelBusy) return;
    modelBusy = true;
    error = null;
    try {
      await modelClient[action]();
      await loadModel();
      if (action === 'remove' && draft) {
        const updated = await client.update({
          globalShortcut: null,
          providerTieBreakOrder: null,
          theme: null,
          localSpeechEnabled: false,
        });
        saved = updated;
        draft = cloneSettings(updated);
      }
    } catch (caught) {
      error = message(caught, 'The local speech model could not be changed.');
    } finally {
      modelBusy = false;
    }
  }

  async function load() {
    loading = true;
    error = null;
    try {
      saved = await client.get();
      draft = cloneSettings(saved);
      applyTheme(saved.theme);
    } catch (caught) {
      error = message(caught, 'Settings could not be loaded.');
    } finally {
      loading = false;
    }
  }

  function chooseTheme(theme: ThemeSetting) {
    if (!draft) return;
    draft = { ...draft, theme };
    applyTheme(theme);
    savedNotice = false;
  }

  function setShortcut(value: string) {
    if (!draft) return;
    draft = { ...draft, globalShortcut: value };
    savedNotice = false;
  }

  function moveProvider(index: number, direction: -1 | 1) {
    if (!draft) return;
    const target = index + direction;
    if (target < 0 || target >= draft.providerTieBreakOrder.length) return;
    const order = [...draft.providerTieBreakOrder];
    [order[index], order[target]] = [order[target], order[index]];
    draft = { ...draft, providerTieBreakOrder: order };
    savedNotice = false;
  }

  async function save() {
    if (!draft || saving || !dirty) return;
    saving = true;
    error = null;
    savedNotice = false;
    try {
      const updated = await client.update({
        globalShortcut: draft.globalShortcut,
        providerTieBreakOrder: draft.providerTieBreakOrder,
        theme: draft.theme,
        localSpeechEnabled: draft.localSpeechEnabled,
      });
      saved = updated;
      draft = cloneSettings(updated);
      applyTheme(updated.theme);
      savedNotice = true;
    } catch (caught) {
      if (saved) {
        draft = cloneSettings(saved);
        applyTheme(saved.theme);
      }
      error = message(caught, 'Settings could not be saved.');
    } finally {
      saving = false;
    }
  }

  function message(caught: unknown, fallback: string) {
    return caught instanceof SettingsCommandError ? caught.message : fallback;
  }

  function cloneSettings(settings: AppSettings): AppSettings {
    return {
      ...settings,
      providerTieBreakOrder: [...settings.providerTieBreakOrder],
    };
  }
</script>

<section class="settings-page" aria-labelledby="settings-title">
  <header class="settings-header">
    <h1 id="settings-title">Settings</h1>
    <p>Local preferences for capture, context, and appearance.</p>
  </header>

  {#if loading}
    <p class="settings-status" aria-live="polite">Loading settings…</p>
  {:else if draft}
    <form class="settings-form" onsubmit={(event) => event.preventDefault()}>
      <section class="settings-section" aria-labelledby="shortcut-title">
        <div>
          <h2 id="shortcut-title">Quick capture</h2>
          <p>The global shortcut used to open Lyn from another application.</p>
        </div>
        <label>
          <span>Global shortcut</span>
          <input
            type="text"
            value={draft.globalShortcut}
            maxlength="100"
            autocomplete="off"
            oninput={(event) =>
              setShortcut((event.currentTarget as HTMLInputElement).value)}
          />
        </label>
      </section>

      <section class="settings-section" aria-labelledby="providers-title">
        <div>
          <h2 id="providers-title">Context tie-break order</h2>
          <p>
            Used only when providers have equally strong invocation evidence.
          </p>
        </div>
        <ol class="provider-order">
          {#each draft.providerTieBreakOrder as provider, index (provider)}
            <li>
              <span class="provider-name"
                ><span class="provider-priority" aria-hidden="true"
                  >{index + 1}</span
                >{providerNames[provider]}</span
              >
              <span class="provider-order-actions">
                <button
                  type="button"
                  disabled={index === 0}
                  aria-label={`Move ${providerNames[provider]} earlier`}
                  onclick={() => moveProvider(index, -1)}
                  ><ArrowUpIcon aria-hidden="true" /></button
                >
                <button
                  type="button"
                  disabled={index === draft.providerTieBreakOrder.length - 1}
                  aria-label={`Move ${providerNames[provider]} later`}
                  onclick={() => moveProvider(index, 1)}
                  ><ArrowDownIcon aria-hidden="true" /></button
                >
              </span>
            </li>
          {/each}
        </ol>
      </section>

      <section class="settings-section" aria-labelledby="theme-title">
        <div>
          <h2 id="theme-title">Appearance</h2>
          <p>Follow the system or choose a deterministic Lyn theme.</p>
        </div>
        <div class="theme-options" role="group" aria-label="Theme">
          {#each ['system', 'light', 'dark'] as theme}
            <button
              type="button"
              class:active={draft.theme === theme}
              aria-pressed={draft.theme === theme}
              onclick={() => chooseTheme(theme as ThemeSetting)}
              >{theme[0].toUpperCase() + theme.slice(1)}</button
            >
          {/each}
        </div>
      </section>

      <section class="settings-section" aria-labelledby="speech-title">
        <div>
          <h2 id="speech-title">Local speech</h2>
          <p>
            Generate searchable captions for voice captures entirely on this
            device.
          </p>
        </div>
        <div class="speech-controls">
          <div class="model-row">
            <div class="model-identity">
              <strong>{model?.label ?? 'Multilingual base'}</strong>
              <span>Approximately 150 MB</span>
            </div>
            <div class="model-management">
              <span
                class="model-status"
                data-state={model?.state ?? 'loading'}
                aria-live="polite"
              >
                {#if !model}Loading model…
                {:else if model.state === 'downloading'}
                  Downloading {model.totalBytes && model.downloadedBytes
                    ? Math.round(
                        (model.downloadedBytes / model.totalBytes) * 100,
                      )
                    : 0}%
                {:else if model.state === 'installed'}Installed
                {:else if model.state === 'invalid'}Needs repair
                {:else}Model not installed{/if}
              </span>
              {#if model?.state === 'downloading'}
                <button
                  type="button"
                  class="secondary-action"
                  disabled={modelBusy}
                  onclick={() => changeModel('cancel')}>Cancel download</button
                >
              {:else if model?.state === 'installed'}
                <button
                  type="button"
                  class="secondary-action quiet-danger"
                  disabled={modelBusy}
                  onclick={() => changeModel('remove')}>Remove model</button
                >
              {:else if model}
                <button
                  type="button"
                  class="secondary-action"
                  disabled={modelBusy}
                  onclick={() => changeModel('install')}
                  >{modelBusy ? 'Starting…' : 'Install model'}</button
                >
              {/if}
            </div>
          </div>
          {#if model?.state === 'downloading'}
            <progress
              value={model.downloadedBytes ?? 0}
              max={model.totalBytes ?? 1}
              aria-label="Model download progress">Download progress</progress
            >
          {:else if model?.state === 'installed'}
            <div class="speech-preference">
              <div>
                <strong>Automatic transcription</strong>
                <span>Generate a caption after saving each voice capture.</span>
              </div>
              <label class="settings-switch">
                <input
                  type="checkbox"
                  aria-label="Automatic transcription"
                  checked={draft.localSpeechEnabled}
                  onchange={(event) => {
                    draft = {
                      ...draft!,
                      localSpeechEnabled: event.currentTarget.checked,
                    };
                    savedNotice = false;
                  }}
                />
              </label>
            </div>
          {/if}
        </div>
      </section>

      {#if error}<p class="settings-error" role="alert">{error}</p>{/if}
      {#if savedNotice}
        <p class="settings-saved" role="status">
          <CheckIcon aria-hidden="true" />Settings saved
        </p>
      {/if}
      <div class="settings-actions">
        <span class="settings-change-state" aria-live="polite">
          {dirty ? 'Unsaved changes' : ''}
        </span>
        <button type="button" disabled={saving || !dirty} onclick={save}
          >{saving ? 'Saving…' : 'Save settings'}</button
        >
      </div>
    </form>
  {:else}
    <div class="settings-error" role="alert">
      <span>{error ?? 'Settings could not be loaded.'}</span>
      <button type="button" onclick={load}>Retry</button>
    </div>
  {/if}
</section>
