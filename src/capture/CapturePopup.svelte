<script lang="ts">
  import { onMount, tick } from 'svelte';

  import type {
    AppError,
    CaptureSession,
    ContextRef,
    ContextSourceOption,
    ListCaptureContextSourcesResult,
  } from '../lib/ipc-types';
  import {
    CaptureCommandError,
    captureClient,
    type CaptureClient,
  } from './capture-client';
  import ContextIndicator from './ContextIndicator.svelte';
  import ContextSourceChooser from './ContextSourceChooser.svelte';

  interface Props {
    client?: CaptureClient;
    dismiss?: () => Promise<void> | void;
  }

  let { client = captureClient, dismiss }: Props = $props();

  let draft = $state('');
  let session = $state<CaptureSession | null>(null);
  let sources = $state<ListCaptureContextSourcesResult>({
    liveSources: [],
    savedContexts: [],
  });
  let chooserOpen = $state(false);
  let sourcesLoading = $state(false);
  let sourceStale = $state(false);
  let error = $state<AppError | null>(null);
  let isSaving = $state(false);
  let isStagingImage = $state(false);
  let isCreatingContext = $state(false);
  let isComposing = $state(false);
  let cancelRequested = $state(false);
  let draftInput = $state<HTMLTextAreaElement>();
  let contextButton = $state<HTMLButtonElement>();

  onMount(() => {
    draftInput?.focus();
    void initialise();
    const unlisten = client.onSessionReady((readySession) => {
      session = readySession;
      draft = '';
      error = null;
      chooserOpen = false;
      sourceStale = false;
      void refreshSources();
      void tick().then(() => draftInput?.focus());
    });
    const unlistenSources = client.onContextSourcesChanged((sessionId) => {
      if (chooserOpen && session?.sessionId === sessionId)
        void refreshSources();
    });
    return () => {
      void unlisten.then((removeListener) => removeListener());
      void unlistenSources.then((removeListener) => removeListener());
    };
  });

  async function initialise() {
    try {
      const activeSession = await client.getActiveSession();
      session = activeSession;
      await refreshSources();
      if (cancelRequested) {
        await cancel();
      }
    } catch (caught) {
      error = toAppError(
        caught,
        'Lyn could not prepare this capture. Try again.',
      );
    }
  }

  function toAppError(caught: unknown, fallback: string): AppError {
    if (caught instanceof CaptureCommandError) {
      return caught.appError;
    }
    return {
      code: 'INTERNAL_ERROR',
      message: fallback,
      retryable: true,
      details: {},
    };
  }

  async function openChooser() {
    chooserOpen = true;
    error = null;
    await refreshSources();
    await tick();
  }

  async function refreshSources() {
    if (!session) return;
    sourcesLoading = true;
    try {
      sources = await client.listContextSources(session.sessionId);
    } catch (caught) {
      error = toAppError(caught, 'Contexts could not be refreshed. Try again.');
    } finally {
      sourcesLoading = false;
    }
  }

  async function closeChooser() {
    chooserOpen = false;
    await tick();
    contextButton?.focus();
  }

  async function selectContext(context: ContextRef) {
    if (!session) return;
    try {
      session = await client.selectContext(session.sessionId, context.id);
      sourceStale = false;
      error = null;
      await closeChooser();
    } catch (caught) {
      error = toAppError(
        caught,
        'That context could not be selected. Try again.',
      );
    }
  }

  async function selectLiveSource(source: ContextSourceOption) {
    if (!session) return;
    try {
      session = await client.selectLiveSource(
        session.sessionId,
        source.sourceId,
      );
      sourceStale = false;
      error = null;
      await closeChooser();
    } catch (caught) {
      error = toAppError(
        caught,
        'That live source is no longer available. Choose another context.',
      );
      sourceStale = true;
      await refreshSources();
    }
  }

  async function createContext(name: string) {
    if (!session || isCreatingContext || name.trim().length === 0) return false;
    isCreatingContext = true;
    error = null;
    try {
      const context = await client.createStandaloneContext(name.trim());
      sources = {
        ...sources,
        savedContexts: [...sources.savedContexts, context],
      };
      await selectContext(context);
      return true;
    } catch (caught) {
      error = toAppError(
        caught,
        'The context could not be created. Try again.',
      );
      return false;
    } finally {
      isCreatingContext = false;
    }
  }

  async function save() {
    if (!session || isSaving) return;
    if (session.contextResolution.state !== 'resolved') {
      error = {
        code:
          session.contextResolution.state === 'ambiguous'
            ? 'CONTEXT_AMBIGUOUS'
            : 'CONTEXT_REQUIRED',
        message: 'Choose a context before saving.',
        retryable: true,
        details: {},
      };
      await openChooser();
      return;
    }

    isSaving = true;
    error = null;
    try {
      if (session.stagedMedia?.kind === 'image') {
        await client.saveImage(
          session.sessionId,
          session.stagedMedia.stagedMediaId,
          draft,
        );
      } else {
        await client.saveText(session.sessionId, draft);
      }
      await (dismiss?.() ?? client.dismissPopup());
    } catch (caught) {
      error = toAppError(caught, 'The capture could not be saved. Try again.');
      if (error.code === 'CONTEXT_SOURCE_STALE') {
        sourceStale = true;
        await openChooser();
        return;
      }
      await tick();
      draftInput?.focus();
    } finally {
      isSaving = false;
    }
  }

  async function stageImage() {
    if (!session || isStagingImage) return;
    isStagingImage = true;
    error = null;
    try {
      const stagedMedia = await client.stageClipboardImage(session.sessionId);
      session = { ...session, stagedMedia };
    } catch (caught) {
      error = toAppError(
        caught,
        'The clipboard image could not be prepared. Try copying it again.',
      );
    } finally {
      isStagingImage = false;
    }
  }

  function handlePaste(event: ClipboardEvent) {
    if (
      Array.from(event.clipboardData?.items ?? []).some((item) =>
        item.type.startsWith('image/'),
      )
    ) {
      event.preventDefault();
      void stageImage();
    }
  }

  async function cancel() {
    if (!session) {
      cancelRequested = true;
      return;
    }
    try {
      await client.cancel(session.sessionId);
      await (dismiss?.() ?? client.dismissPopup());
    } catch (caught) {
      error = toAppError(
        caught,
        'The capture could not be cancelled. Try again.',
      );
    }
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.isComposing || isComposing) return;
    if (event.key === 'Escape') {
      event.preventDefault();
      if (chooserOpen) {
        void closeChooser();
      } else {
        void cancel();
      }
      return;
    }
    if (
      event.key === 'Enter' &&
      !event.shiftKey &&
      event.target === draftInput
    ) {
      event.preventDefault();
      void save();
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<main class="capture-shell" aria-label="Quick capture">
  <section class="capture-popup">
    <label class="sr-only" for="capture-text">Capture text</label>
    <textarea
      bind:this={draftInput}
      bind:value={draft}
      class="capture-input"
      id="capture-text"
      name="capture-text"
      placeholder="Type or paste anything…"
      aria-describedby={error ? 'capture-error' : undefined}
      oncompositionstart={() => (isComposing = true)}
      oncompositionend={() => (isComposing = false)}
      onpaste={handlePaste}></textarea>

    {#if session?.stagedMedia?.kind === 'image'}
      <figure class="image-preview">
        <img
          src={session.stagedMedia.previewUri}
          alt="Screenshot ready to save"
        />
        <figcaption>
          <span>Screenshot ready</span>
          <button type="button" disabled={isStagingImage} onclick={stageImage}
            >{isStagingImage ? 'Replacing…' : 'Replace'}</button
          >
        </figcaption>
      </figure>
    {/if}

    <ContextIndicator
      bind:button={contextButton}
      resolution={session?.contextResolution ?? null}
      open={chooserOpen}
      stale={sourceStale}
      onclick={() => (chooserOpen ? closeChooser() : openChooser())}
    />

    {#if chooserOpen}
      <ContextSourceChooser
        liveSources={sources.liveSources}
        savedContexts={sources.savedContexts}
        loading={sourcesLoading}
        creating={isCreatingContext}
        onselectlive={(source) => void selectLiveSource(source)}
        onselectsaved={(context) => void selectContext(context)}
        oncreate={createContext}
      />
    {/if}

    {#if error}
      <div id="capture-error" class="capture-error" role="alert">
        <span>{error.message}</span>
        {#if error.code === 'STORAGE_WRITE_FAILED'}
          <button type="button" aria-label="Retry save" onclick={save}
            >Retry</button
          >
        {/if}
      </div>
    {/if}

    <footer class="capture-actions">
      <p class="keyboard-hint">
        <span><kbd>Shift</kbd> + <kbd>Enter</kbd> newline</span>
        <span><kbd>Enter</kbd> save</span>
      </p>
      <div class="action-buttons">
        <button type="button" class="cancel-button" onclick={cancel}
          >Cancel</button
        >
        <button
          type="button"
          class="save-button"
          disabled={!session || isSaving}
          onclick={save}
        >
          {isSaving ? 'Saving…' : 'Save'}
        </button>
      </div>
    </footer>
  </section>
</main>
