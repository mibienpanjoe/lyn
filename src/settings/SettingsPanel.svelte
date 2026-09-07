<script lang="ts">
  import ArrowDownIcon from '@lucide/svelte/icons/arrow-down';
  import ArrowUpIcon from '@lucide/svelte/icons/arrow-up';
  import CheckIcon from '@lucide/svelte/icons/check';
  import CodeIcon from '@lucide/svelte/icons/code';
  import GlobeIcon from '@lucide/svelte/icons/globe';
  import MonitorIcon from '@lucide/svelte/icons/monitor';
  import MoonIcon from '@lucide/svelte/icons/moon';
  import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
  import SunIcon from '@lucide/svelte/icons/sun';
  import TerminalIcon from '@lucide/svelte/icons/terminal';
  import { onDestroy, onMount, tick } from 'svelte';

  import type {
    AppSettings,
    ContextProviderKind,
    IntegrationId,
    IntegrationStatus,
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
  import {
    integrationClient,
    type IntegrationClient,
    IntegrationCommandError,
  } from './integration-client';
  import { isLinuxPlatform } from '../lib/platform';

  interface Props {
    client?: SettingsClient;
    modelClient?: SpeechModelClient;
    intClient?: IntegrationClient;
    isLinux?: boolean;
  }

  let {
    client = settingsClient,
    modelClient = speechModelClient,
    intClient = integrationClient,
    isLinux = isLinuxPlatform(),
  }: Props = $props();
  let saved = $state<AppSettings | null>(null);
  let draft = $state<AppSettings | null>(null);
  let loading = $state(true);
  let saving = $state(false);
  let error = $state<string | null>(null);
  let savedNotice = $state(false);
  let model = $state<SpeechModelStatus | null>(null);
  let modelBusy = $state(false);
  let editingShortcut = $state(false);
  let shortcutInput = $state<HTMLInputElement>();
  let shortcutBeforeEdit = '';
  let pendingSave: AppSettings | null = null;
  let unsubscribeModel: (() => void) | null = null;

  let integrations = $state<IntegrationStatus[]>([]);
  let loadingIntegrations = $state(true);
  let integrationError = $state<string | null>(null);
  let installingIntegrationId = $state<IntegrationId | null>(null);
  let integrationFeedback = $state<
    Record<string, { success: boolean; message: string }>
  >({});
  let integrationRequestSeq = 0;

  const providerNames: Record<ContextProviderKind, string> = {
    manual: 'Manual selection',
    vscode: 'VS Code',
    cursor: 'Cursor',
    browser: 'Browser',
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
  const shortcutParts = $derived(
    (draft?.globalShortcut ?? '')
      .split('+')
      .map((part) => (part === 'Control' ? 'Ctrl' : part)),
  );

  onMount(() => {
    void load();
    void loadModel();
    if (isLinux) {
      void loadIntegrations();
    }
    void modelClient
      .subscribe((status) => (model = status))
      .then((unsubscribe) => {
        unsubscribeModel = unsubscribe;
      })
      .catch(() => {});
  });
  onDestroy(() => {
    unsubscribeModel?.();
  });

  async function loadIntegrations() {
    if (!isLinux) {
      loadingIntegrations = false;
      return;
    }
    const seq = ++integrationRequestSeq;
    loadingIntegrations = true;
    integrationError = null;
    try {
      const result = await intClient.list();
      if (seq === integrationRequestSeq) {
        integrations = result;
        integrationError = null;
      }
    } catch (caught) {
      if (seq === integrationRequestSeq) {
        integrationError =
          caught instanceof IntegrationCommandError
            ? caught.message
            : 'Integration discovery failed.';
      }
    } finally {
      if (seq === integrationRequestSeq) {
        loadingIntegrations = false;
      }
    }
  }

  async function installIntegration(id: IntegrationId) {
    if (!isLinux || installingIntegrationId) return;
    installingIntegrationId = id;
    try {
      const result = await intClient.install({ id });
      integrationFeedback = {
        ...integrationFeedback,
        [id]: { success: result.success, message: result.message },
      };
      await loadIntegrations();
    } catch (caught) {
      const msg =
        caught instanceof IntegrationCommandError
          ? caught.message
          : 'Integration installation failed.';
      integrationFeedback = {
        ...integrationFeedback,
        [id]: { success: false, message: msg },
      };
    } finally {
      installingIntegrationId = null;
    }
  }

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
        updateDraft({ ...draft, localSpeechEnabled: false });
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
    updateDraft({ ...draft, theme });
    applyTheme(theme);
  }

  function setShortcut(value: string) {
    if (!draft) return;
    draft = { ...draft, globalShortcut: value };
    savedNotice = false;
  }

  async function beginShortcutEdit() {
    shortcutBeforeEdit = draft?.globalShortcut ?? '';
    editingShortcut = true;
    await tick();
    shortcutInput?.focus();
    shortcutInput?.select();
  }

  function handleShortcutKeydown(event: KeyboardEvent) {
    if (event.key === 'Enter') {
      event.preventDefault();
      finishShortcutEdit();
    } else if (event.key === 'Escape') {
      event.preventDefault();
      setShortcut(shortcutBeforeEdit);
      editingShortcut = false;
    }
  }

  function finishShortcutEdit() {
    editingShortcut = false;
    if (draft && draft.globalShortcut !== shortcutBeforeEdit) queueSave(draft);
  }

  function moveProvider(index: number, direction: -1 | 1) {
    if (!draft) return;
    const target = index + direction;
    if (target < 0 || target >= draft.providerTieBreakOrder.length) return;
    const order = [...draft.providerTieBreakOrder];
    [order[index], order[target]] = [order[target], order[index]];
    updateDraft({ ...draft, providerTieBreakOrder: order });
  }

  function setLocalSpeechEnabled(localSpeechEnabled: boolean) {
    if (!draft) return;
    updateDraft({ ...draft, localSpeechEnabled });
  }

  function updateDraft(next: AppSettings) {
    draft = next;
    queueSave(next);
  }

  function queueSave(settings: AppSettings) {
    pendingSave = cloneSettings(settings);
    error = null;
    savedNotice = false;
    void flushSaves();
  }

  async function flushSaves() {
    if (saving) return;
    saving = true;
    while (pendingSave) {
      const snapshot = pendingSave;
      pendingSave = null;
      try {
        const updated = await client.update({
          globalShortcut: snapshot.globalShortcut,
          providerTieBreakOrder: snapshot.providerTieBreakOrder,
          theme: snapshot.theme,
          localSpeechEnabled: snapshot.localSpeechEnabled,
        });
        saved = cloneSettings(updated);
        if (draft && sameSettings(draft, snapshot)) {
          draft = cloneSettings(updated);
        }
        savedNotice = pendingSave === null;
      } catch (caught) {
        pendingSave = null;
        if (saved) {
          draft = cloneSettings(saved);
          applyTheme(saved.theme);
        }
        error = message(caught, 'Settings could not be saved.');
        break;
      }
    }
    saving = false;
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

  function sameSettings(left: AppSettings, right: AppSettings) {
    return (
      left.globalShortcut === right.globalShortcut &&
      left.theme === right.theme &&
      left.localSpeechEnabled === right.localSpeechEnabled &&
      left.providerTieBreakOrder.length ===
        right.providerTieBreakOrder.length &&
      left.providerTieBreakOrder.every(
        (provider, index) => provider === right.providerTieBreakOrder[index],
      )
    );
  }
</script>

<section class="settings-page" aria-labelledby="settings-title">
  <header class="settings-header">
    <div>
      <h1 id="settings-title">Settings</h1>
      <p>Local preferences for capture, context, and appearance.</p>
    </div>
    <span class="settings-save-status" role="status" aria-live="polite">
      {#if saving}Saving…
      {:else if dirty}Unsaved changes
      {:else if savedNotice}<CheckIcon aria-hidden="true" />Settings saved{/if}
    </span>
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
        <div class="shortcut-setting">
          <span class="control-label">Global shortcut</span>
          {#if editingShortcut}
            <div class="shortcut-editor">
              <input
                bind:this={shortcutInput}
                aria-label="Global shortcut"
                type="text"
                value={draft.globalShortcut}
                maxlength="100"
                autocomplete="off"
                oninput={(event) =>
                  setShortcut((event.currentTarget as HTMLInputElement).value)}
                onkeydown={handleShortcutKeydown}
              />
              <button type="button" onclick={finishShortcutEdit}>Done</button>
            </div>
          {:else}
            <div class="shortcut-display">
              <span class="shortcut-keycaps" aria-label={draft.globalShortcut}>
                {#each shortcutParts as part, index}
                  {#if index > 0}<span aria-hidden="true">+</span>{/if}
                  <kbd>{part}</kbd>
                {/each}
              </span>
              <button type="button" onclick={beginShortcutEdit}
                >Change shortcut</button
              >
            </div>
          {/if}
        </div>
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

      {#if isLinux}
        <section
          class="settings-section integrations-section"
          aria-labelledby="integrations-title"
        >
          <div class="integrations-header">
            <div>
              <h2 id="integrations-title">Integrations & Context Providers</h2>
              <p>
                Connect your editors, browsers, and terminals with 1-click so
                Lyn automatically associates captures with your active
                workspace.
              </p>
            </div>
            <button
              type="button"
              class="refresh-integrations-btn"
              title="Refresh status"
              aria-label="Refresh integration statuses"
              onclick={() => loadIntegrations()}
            >
              <RefreshCwIcon
                size={14}
                class={loadingIntegrations ? 'spin' : ''}
              />
            </button>
          </div>

          <div class="integrations-list">
            {#if loadingIntegrations && integrations.length === 0}
              <p class="integrations-loading">Scanning local environment…</p>
            {:else if integrationError}
              <div class="settings-error integration-error" role="alert">
                <span>{integrationError}</span>
                <button
                  type="button"
                  class="secondary-action"
                  onclick={() => loadIntegrations()}>Retry</button
                >
              </div>
            {:else}
              {#each integrations as item (item.id)}
                <div class="integration-card" data-installed={item.installed}>
                  <div class="integration-icon-wrap" aria-hidden="true">
                    {#if item.id === 'cursor' || item.id === 'vscode'}
                      <CodeIcon size={18} />
                    {:else if item.id === 'browser'}
                      <GlobeIcon size={18} />
                    {:else}
                      <TerminalIcon size={18} />
                    {/if}
                  </div>

                  <div class="integration-info">
                    <div class="integration-title-row">
                      <strong>{item.name}</strong>
                      {#if item.installed}
                        <span class="integration-badge installed">
                          <CheckIcon size={11} aria-hidden="true" /> Installed
                        </span>
                      {:else if item.detected}
                        <span class="integration-badge detected">Detected</span>
                      {:else}
                        <span class="integration-badge ready">Ready</span>
                      {/if}
                    </div>
                    <p class="integration-desc">{item.description}</p>

                    {#if integrationFeedback[item.id]}
                      <div
                        class="integration-feedback"
                        class:success={integrationFeedback[item.id].success}
                        class:failure={!integrationFeedback[item.id].success}
                      >
                        {integrationFeedback[item.id].message}
                      </div>
                    {:else if item.details}
                      <div class="integration-details-text">{item.details}</div>
                    {/if}
                  </div>

                  <div class="integration-action">
                    <button
                      type="button"
                      class="secondary-action"
                      class:installed-btn={item.installed}
                      disabled={installingIntegrationId !== null}
                      onclick={() => installIntegration(item.id)}
                    >
                      {#if installingIntegrationId === item.id}
                        Installing…
                      {:else if item.installed}
                        Reinstall
                      {:else if item.id === 'kitty'}
                        Enable Watcher
                      {:else if item.id === 'browser'}
                        Register Host
                      {:else if item.id === 'shell'}
                        Add to Shell
                      {:else}
                        Install Extension
                      {/if}
                    </button>
                  </div>
                </div>
              {/each}
            {/if}
          </div>
        </section>
      {/if}

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
            >
              {#if theme === 'system'}
                <MonitorIcon aria-hidden="true" />
              {:else if theme === 'light'}
                <SunIcon aria-hidden="true" />
              {:else}
                <MoonIcon aria-hidden="true" />
              {/if}
              <span>{theme[0].toUpperCase() + theme.slice(1)}</span></button
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
                data-state={model?.state === 'downloading'
                  ? 'downloading'
                  : model?.state === 'installed'
                    ? 'installed'
                    : model?.errorCode
                      ? 'failed'
                      : (model?.state ?? 'loading')}
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
                {:else if model.errorCode === 'MODEL_DOWNLOAD_FAILED'}Installation
                  failed
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
                  >{modelBusy
                    ? 'Starting…'
                    : model.errorCode === 'MODEL_DOWNLOAD_FAILED'
                      ? 'Retry installation'
                      : 'Install model'}</button
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
                  onchange={(event) =>
                    setLocalSpeechEnabled(event.currentTarget.checked)}
                />
              </label>
            </div>
          {/if}
        </div>
      </section>

      {#if error}<p class="settings-error" role="alert">{error}</p>{/if}
    </form>
  {:else}
    <div class="settings-error" role="alert">
      <span>{error ?? 'Settings could not be loaded.'}</span>
      <button type="button" onclick={load}>Retry</button>
    </div>
  {/if}
</section>
