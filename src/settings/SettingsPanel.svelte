<script lang="ts">
  import ArrowDownIcon from '@lucide/svelte/icons/arrow-down';
  import ArrowUpIcon from '@lucide/svelte/icons/arrow-up';
  import CheckIcon from '@lucide/svelte/icons/check';
  import MonitorIcon from '@lucide/svelte/icons/monitor';
  import MoonIcon from '@lucide/svelte/icons/moon';
  import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
  import SunIcon from '@lucide/svelte/icons/sun';
  import { onDestroy, onMount, tick } from 'svelte';

  import type {
    AppSettings,
    ContextProviderKind,
    IntegrationId,
    IntegrationStatus,
    LanguageSetting,
    SpeechModelStatus,
    ThemeSetting,
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
  import { getTranslations } from '../lib/i18n';
  import BrowserIcon from '../lib/icons/BrowserIcon.svelte';
  import CursorIcon from '../lib/icons/CursorIcon.svelte';
  import KittyIcon from '../lib/icons/KittyIcon.svelte';
  import ShellIcon from '../lib/icons/ShellIcon.svelte';
  import VscodeIcon from '../lib/icons/VscodeIcon.svelte';

  interface Props {
    client?: SettingsClient;
    modelClient?: SpeechModelClient;
    intClient?: IntegrationClient;
    isLinux?: boolean;
    onLanguageChange?: (language: LanguageSetting) => void;
  }

  let {
    client = settingsClient,
    modelClient = speechModelClient,
    intClient = integrationClient,
    isLinux = isLinuxPlatform(),
    onLanguageChange,
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

  const t = $derived(getTranslations(draft?.language ?? 'english'));

  const providerNames = $derived<Record<ContextProviderKind, string>>({
    manual: t.providers.manual,
    vscode: t.providers.vscode,
    cursor: t.providers.cursor,
    browser: t.providers.browser,
    shell: t.providers.shell,
    foreground_window: t.providers.foreground_window,
  });

  const dirty = $derived(
    saved !== null &&
      draft !== null &&
      (saved.globalShortcut !== draft.globalShortcut ||
        saved.theme !== draft.theme ||
        saved.language !== draft.language ||
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
            : t.integrationDiscoveryFailed;
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

  async function load() {
    loading = true;
    error = null;
    try {
      const settings = await client.get();
      saved = cloneSettings(settings);
      draft = cloneSettings(settings);
      applyTheme(settings.theme);
      onLanguageChange?.(settings.language);
    } catch (caught) {
      error = message(caught, t.settingsLoadError);
    } finally {
      loading = false;
    }
  }

  async function loadModel() {
    try {
      model = await modelClient.status();
    } catch {
      model = null;
    }
  }

  async function changeModel(action: 'install' | 'remove' | 'cancel') {
    modelBusy = true;
    try {
      if (action === 'install') await modelClient.install();
      else if (action === 'remove') {
        await modelClient.remove();
        if (draft?.localSpeechEnabled) {
          updateDraft({ ...draft, localSpeechEnabled: false });
        }
      } else await modelClient.cancel();
      model = await modelClient.status();
    } catch (caught) {
      error = message(caught, 'Speech model operation failed.');
    } finally {
      modelBusy = false;
      loading = false;
    }
  }

  function chooseTheme(theme: ThemeSetting) {
    if (!draft) return;
    updateDraft({ ...draft, theme });
    applyTheme(theme);
  }

  function chooseLanguage(language: LanguageSetting) {
    if (!draft) return;
    updateDraft({ ...draft, language });
    onLanguageChange?.(language);
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
          language: snapshot.language,
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
          onLanguageChange?.(saved.language);
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
      left.language === right.language &&
      left.localSpeechEnabled === right.localSpeechEnabled &&
      left.providerTieBreakOrder.length ===
        right.providerTieBreakOrder.length &&
      left.providerTieBreakOrder.every(
        (provider, index) => provider === right.providerTieBreakOrder[index],
      )
    );
  }

  function parseIntegrationName(name: string) {
    if (name.includes('(')) {
      const match = name.match(/^(.*?)\s*\((.*?)\)$/);
      if (match) {
        return {
          title: match[1].trim(),
          subtitle: match[2].trim().replace(/,/g, ' ·'),
        };
      }
    }
    return { title: name, subtitle: null };
  }

  function getIntegrationDescription(id: string, fallback: string): string {
    return t.integrationDescriptions[id] ?? fallback;
  }

  function localizeDetails(detailsText: string): string {
    if (draft?.language !== 'french') return detailsText;
    return detailsText
      .replace(
        'Cursor detected on this system',
        'Cursor détecté sur ce système',
      )
      .replace(
        'Supported web browser detected',
        'Navigateur web pris en charge détecté',
      )
      .replace('Available for Bash and Zsh', 'Disponible pour Bash et Zsh')
      .replace(
        'Shell startup script configured. Lyn refreshes the helper on launch.',
        'Script de démarrage configuré. Lyn met à jour l’assistant à chaque lancement.',
      )
      .replace(
        'Shell startup script configured',
        'Script de démarrage du terminal configuré',
      )
      .replace('Extension active in', 'Extension active dans')
      .replace('Watcher configured in', 'Watcher configuré dans');
  }
</script>

{#snippet renderDetails(rawText: string)}
  {@const detailsText = localizeDetails(rawText)}
  {#if detailsText.includes('~/') || detailsText.includes('/.')}
    {@const parts = detailsText.split(/(~[\w./-]+)/)}
    <div class="integration-details-text">
      {#each parts as part}
        {#if part.startsWith('~')}
          <code class="path-pill">{part}</code>
        {:else}
          {part}
        {/if}
      {/each}
    </div>
  {:else}
    <div class="integration-details-text">{detailsText}</div>
  {/if}
{/snippet}

<section class="settings-page" aria-labelledby="settings-title">
  <header class="settings-header">
    <div>
      <h1 id="settings-title">{t.settingsTitle}</h1>
      <p>{t.settingsSubtitle}</p>
    </div>
    <span class="settings-save-status" role="status" aria-live="polite">
      {#if saving}{t.saving}
      {:else if dirty}{t.unsavedChanges}
      {:else if savedNotice}<CheckIcon
          aria-hidden="true"
        />{t.settingsSaved}{/if}
    </span>
  </header>

  {#if loading}
    <p class="settings-status" aria-live="polite">{t.loadingSettings}</p>
  {:else if draft}
    <form class="settings-form" onsubmit={(event) => event.preventDefault()}>
      <section class="settings-section" aria-labelledby="shortcut-title">
        <div>
          <h2 id="shortcut-title">{t.quickCaptureTitle}</h2>
          <p>{t.quickCaptureSubtitle}</p>
        </div>
        <div class="shortcut-setting">
          <span class="control-label">{t.globalShortcutLabel}</span>
          {#if editingShortcut}
            <div class="shortcut-editor">
              <input
                bind:this={shortcutInput}
                aria-label={t.globalShortcutLabel}
                type="text"
                value={draft.globalShortcut}
                maxlength="100"
                autocomplete="off"
                oninput={(event) =>
                  setShortcut((event.currentTarget as HTMLInputElement).value)}
                onkeydown={handleShortcutKeydown}
              />
              <button type="button" onclick={finishShortcutEdit}
                >{t.done}</button
              >
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
                >{t.changeShortcut}</button
              >
            </div>
          {/if}
        </div>
      </section>

      <section class="settings-section" aria-labelledby="providers-title">
        <div>
          <h2 id="providers-title">{t.contextPriorityTitle}</h2>
          <p>{t.contextPrioritySubtitle}</p>
        </div>
        <ol class="provider-order">
          {#each draft.providerTieBreakOrder as provider, index (provider)}
            <li>
              <span class="provider-name"
                ><span class="provider-priority" aria-hidden="true"
                  >{index + 1}</span
                >{providerNames[provider]}</span
              >
              <div class="provider-actions">
                <button
                  type="button"
                  aria-label={t.moveEarlier(providerNames[provider])}
                  disabled={index === 0}
                  onclick={() => moveProvider(index, -1)}
                >
                  <ArrowUpIcon aria-hidden="true" />
                </button>
                <button
                  type="button"
                  aria-label={t.moveLater(providerNames[provider])}
                  disabled={index === draft.providerTieBreakOrder.length - 1}
                  onclick={() => moveProvider(index, 1)}
                >
                  <ArrowDownIcon aria-hidden="true" />
                </button>
              </div>
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
              <h2 id="integrations-title">{t.integrationsTitle}</h2>
              <p>{t.integrationsSubtitle}</p>
            </div>
            <button
              type="button"
              class="refresh-integrations-btn"
              title={t.refreshIntegrations}
              aria-label={t.refreshIntegrations}
              disabled={loadingIntegrations}
              onclick={() => loadIntegrations()}
            >
              <RefreshCwIcon
                size={14}
                class={loadingIntegrations ? 'spinning' : ''}
                aria-hidden="true"
              />
            </button>
          </div>

          <div class="integrations-list">
            {#if loadingIntegrations && integrations.length === 0}
              <p class="integrations-loading">{t.scanningIntegrations}</p>
            {:else if integrationError}
              <div class="settings-error integration-error" role="alert">
                <span>{integrationError}</span>
                <button
                  type="button"
                  class="secondary-action"
                  onclick={() => loadIntegrations()}>{t.retry}</button
                >
              </div>
            {:else}
              {#each integrations as item (item.id)}
                {@const parsed = parseIntegrationName(item.name)}
                <div class="integration-card" data-installed={item.installed}>
                  <div
                    class="integration-icon-wrap"
                    data-platform={item.id}
                    aria-hidden="true"
                  >
                    {#if item.id === 'cursor'}
                      <CursorIcon size={22} />
                    {:else if item.id === 'vscode'}
                      <VscodeIcon size={22} />
                    {:else if item.id === 'browser'}
                      <BrowserIcon size={22} />
                    {:else if item.id === 'kitty'}
                      <KittyIcon size={22} />
                    {:else}
                      <ShellIcon size={22} />
                    {/if}
                  </div>

                  <div class="integration-info">
                    <div class="integration-title-row">
                      <div class="integration-title-group">
                        <strong>{parsed.title}</strong>
                        {#if parsed.subtitle}
                          <span class="integration-subname"
                            >{parsed.subtitle}</span
                          >
                        {/if}
                      </div>
                      {#if item.installed}
                        <span class="integration-badge installed">
                          <CheckIcon size={11} aria-hidden="true" />
                          {t.badgeInstalled}
                        </span>
                      {:else if item.detected}
                        <span class="integration-badge detected"
                          >{t.badgeDetected}</span
                        >
                      {:else}
                        <span class="integration-badge ready"
                          >{t.badgeReady}</span
                        >
                      {/if}
                    </div>

                    <p class="integration-desc">
                      {getIntegrationDescription(item.id, item.description)}
                    </p>

                    {#if integrationFeedback[item.id]}
                      <div
                        class="integration-feedback"
                        class:success={integrationFeedback[item.id].success}
                        class:failure={!integrationFeedback[item.id].success}
                      >
                        {integrationFeedback[item.id].message}
                      </div>
                    {:else if item.details}
                      {@render renderDetails(item.details)}
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
                        {t.btnInstalling}
                      {:else if item.installed}
                        {t.btnReinstall}
                      {:else if item.id === 'kitty'}
                        {t.btnEnableWatcher}
                      {:else if item.id === 'browser'}
                        {t.btnRegisterHost}
                      {:else if item.id === 'shell'}
                        {t.btnAddShell}
                      {:else}
                        {t.btnInstallExtension}
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
          <h2 id="theme-title">{t.appearanceTitle}</h2>
          <p>{t.appearanceSubtitle}</p>
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
              <span
                >{theme === 'system'
                  ? t.themeSystem
                  : theme === 'light'
                    ? t.themeLight
                    : t.themeDark}</span
              ></button
            >
          {/each}
        </div>
      </section>

      <section class="settings-section" aria-labelledby="language-title">
        <div>
          <h2 id="language-title">{t.languageTitle}</h2>
          <p>{t.languageSubtitle}</p>
        </div>
        <div class="language-options" role="group" aria-label={t.languageTitle}>
          <button
            type="button"
            class:active={draft.language === 'english'}
            aria-pressed={draft.language === 'english'}
            onclick={() => chooseLanguage('english')}
          >
            <span>{t.langEnglish}</span>
          </button>
          <button
            type="button"
            class:active={draft.language === 'french'}
            aria-pressed={draft.language === 'french'}
            onclick={() => chooseLanguage('french')}
          >
            <span>{t.langFrench}</span>
          </button>
        </div>
      </section>

      <section class="settings-section" aria-labelledby="speech-title">
        <div>
          <h2 id="speech-title">{t.speechTitle}</h2>
          <p>{t.speechSubtitle}</p>
        </div>
        <div class="speech-controls">
          <div class="model-row">
            <div class="model-identity">
              <strong>{model?.label ?? 'Multilingual base'}</strong>
              <span>{t.modelApproxSize}</span>
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
                {#if !model}{t.modelLoading}
                {:else if model.state === 'downloading'}
                  {t.modelDownloading(
                    model.totalBytes && model.downloadedBytes
                      ? Math.round(
                          (model.downloadedBytes / model.totalBytes) * 100,
                        )
                      : 0,
                  )}
                {:else if model.state === 'installed'}{t.modelInstalled}
                {:else if model.errorCode === 'MODEL_DOWNLOAD_FAILED'}{t.modelInstallFailed}
                {:else if model.state === 'invalid'}{t.modelNeedsRepair}
                {:else}{t.modelNotInstalled}{/if}
              </span>
              {#if model?.state === 'downloading'}
                <button
                  type="button"
                  class="secondary-action"
                  disabled={modelBusy}
                  onclick={() => changeModel('cancel')}
                  >{t.cancelDownload}</button
                >
              {:else if model?.state === 'installed'}
                <button
                  type="button"
                  class="secondary-action quiet-danger"
                  disabled={modelBusy}
                  onclick={() => changeModel('remove')}>{t.removeModel}</button
                >
              {:else if model}
                <button
                  type="button"
                  class="secondary-action"
                  disabled={modelBusy}
                  onclick={() => changeModel('install')}
                  >{modelBusy
                    ? t.starting
                    : model.errorCode === 'MODEL_DOWNLOAD_FAILED'
                      ? t.retryInstall
                      : t.installModel}</button
                >
              {/if}
            </div>
          </div>
          {#if model?.state === 'downloading'}
            <progress
              value={model.downloadedBytes ?? 0}
              max={model.totalBytes ?? 1}
              aria-label={t.downloadProgress}>{t.downloadProgress}</progress
            >
          {:else if model?.state === 'installed'}
            <div class="speech-preference">
              <div>
                <strong>{t.autoTranscription}</strong>
                <span>{t.autoTranscriptionDesc}</span>
              </div>
              <label class="settings-switch">
                <input
                  type="checkbox"
                  aria-label={t.autoTranscription}
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
      <span>{error ?? t.settingsLoadError}</span>
      <button type="button" onclick={load}>{t.retry}</button>
    </div>
  {/if}
</section>
