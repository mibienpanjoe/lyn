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
  let isDiscardingMedia = $state(false);
  let mediaNotice = $state<string | null>(null);
  let isRecordingAction = $state(false);
  let isPlaying = $state(false);
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
      isPlaying = false;
      mediaNotice = null;
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
      } else if (session.stagedMedia?.kind === 'audio') {
        await client.saveAudio(
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

  async function toggleRecording() {
    if (!session || isRecordingAction || session.stagedMedia?.kind === 'image')
      return;
    isRecordingAction = true;
    error = null;
    try {
      if (session.recordingState.state === 'recording') {
        const stagedMedia = await client.stopAudioRecording(session.sessionId);
        session = {
          ...session,
          stagedMedia,
          recordingState: {
            state: 'stopped',
            elapsedMs: stagedMedia.durationMs ?? 0,
            stagedMediaId: stagedMedia.stagedMediaId,
          },
        };
      } else {
        const recordingState = await client.startAudioRecording(
          session.sessionId,
        );
        session = { ...session, stagedMedia: null, recordingState };
        isPlaying = false;
      }
    } catch (caught) {
      error = toAppError(
        caught,
        'Voice recording could not be completed. Try again.',
      );
    } finally {
      isRecordingAction = false;
    }
  }

  async function togglePlayback() {
    const staged = session?.stagedMedia;
    if (!session || staged?.kind !== 'audio') return;
    error = null;
    try {
      if (isPlaying) {
        await client.stopAudioPlayback(staged.stagedMediaId);
        isPlaying = false;
      } else {
        const result = await client.playStagedAudio(
          session.sessionId,
          staged.stagedMediaId,
        );
        isPlaying = result.playing;
      }
    } catch (caught) {
      isPlaying = false;
      error = toAppError(caught, 'Voice note playback failed. Try again.');
    }
  }

  function formatDuration(durationMs: number | null) {
    const totalSeconds = Math.max(0, Math.round((durationMs ?? 0) / 1000));
    const minutes = Math.floor(totalSeconds / 60);
    const seconds = String(totalSeconds % 60).padStart(2, '0');
    return `${minutes}:${seconds}`;
  }

  async function stageImage(silentUnsupported = false) {
    if (!session || isStagingImage) return;
    const replacingImage = session.stagedMedia?.kind === 'image';
    isStagingImage = true;
    error = null;
    try {
      const stagedMedia = await client.stageClipboardImage(session.sessionId);
      session = { ...session, stagedMedia };
      mediaNotice = replacingImage ? 'Screenshot replaced' : null;
    } catch (caught) {
      const stagedError = toAppError(
        caught,
        'The clipboard image could not be prepared. Try copying it again.',
      );
      if (
        silentUnsupported &&
        stagedError.code === 'UNSUPPORTED_CLIPBOARD_CONTENT'
      )
        return;
      error = stagedError;
    } finally {
      isStagingImage = false;
    }
  }

  async function discardMedia() {
    const staged = session?.stagedMedia;
    if (!session || !staged || isDiscardingMedia) return;
    isDiscardingMedia = true;
    error = null;
    try {
      if (staged.kind === 'audio' && isPlaying) {
        await client.stopAudioPlayback(staged.stagedMediaId).catch(() => {});
      }
      session = await client.discardStagedMedia(
        session.sessionId,
        staged.stagedMediaId,
      );
      isPlaying = false;
      mediaNotice = null;
      await tick();
      draftInput?.focus();
    } catch (caught) {
      error = toAppError(caught, 'The media could not be removed. Try again.');
    } finally {
      isDiscardingMedia = false;
    }
  }

  function handlePaste(event: ClipboardEvent) {
    const clipboard = event.clipboardData;
    const advertisesImage =
      Array.from(clipboard?.items ?? []).some((item) =>
        item.type.startsWith('image/'),
      ) ||
      Array.from(clipboard?.types ?? []).some((type) =>
        type.startsWith('image/'),
      );
    if (advertisesImage) {
      event.preventDefault();
    }
    // WebKitGTK may omit image items/types. Rust owns clipboard decoding, so
    // probe it on every paste and silently fall through for ordinary text.
    void stageImage(true);
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
  <section
    class="capture-popup"
    class:media-mode={session?.stagedMedia != null}
  >
    {#if session?.stagedMedia?.kind === 'image'}
      <figure class="image-preview">
        <figcaption>
          <span>Screenshot preview</span>
          <div class="media-actions">
            <button
              type="button"
              disabled={isStagingImage || isDiscardingMedia}
              onclick={() => stageImage()}
              >{isStagingImage ? 'Pasting…' : 'Paste new image'}</button
            >
            <button
              type="button"
              disabled={isStagingImage || isDiscardingMedia}
              onclick={discardMedia}
              >{isDiscardingMedia ? 'Removing…' : 'Remove image'}</button
            >
          </div>
        </figcaption>
        <div class="image-preview-frame">
          <img
            src={session.stagedMedia.previewUri}
            alt="Screenshot ready to save"
          />
        </div>
        {#if mediaNotice}
          <p class="media-notice" role="status">{mediaNotice}</p>
        {/if}
      </figure>
    {/if}

    <label
      class:sr-only={session?.stagedMedia == null}
      class="capture-label"
      for="capture-text"
      >{session?.stagedMedia?.kind === 'image'
        ? 'Screenshot caption (optional)'
        : session?.stagedMedia?.kind === 'audio'
          ? 'Voice caption (optional)'
          : 'Capture text'}</label
    >
    <textarea
      bind:this={draftInput}
      bind:value={draft}
      class="capture-input"
      id="capture-text"
      name="capture-text"
      placeholder={session?.stagedMedia?.kind === 'image'
        ? 'Add a note about this screenshot…'
        : session?.stagedMedia?.kind === 'audio'
          ? 'Add a note about this voice recording…'
          : 'Type or paste anything…'}
      aria-describedby={error ? 'capture-error' : undefined}
      oncompositionstart={() => (isComposing = true)}
      oncompositionend={() => (isComposing = false)}
      onpaste={handlePaste}></textarea>

    {#if session?.stagedMedia?.kind !== 'image'}
      <section class="voice-controls" aria-label="Voice capture">
        {#if session?.stagedMedia == null}
          <button
            type="button"
            disabled={!session || isStagingImage}
            onclick={() => stageImage()}
            >{isStagingImage ? 'Pasting…' : 'Paste screenshot'}</button
          >
        {/if}
        <button
          type="button"
          class:recording={session?.recordingState.state === 'recording'}
          disabled={!session || isRecordingAction}
          aria-pressed={session?.recordingState.state === 'recording'}
          onclick={toggleRecording}
        >
          <span class="record-dot" aria-hidden="true"></span>
          {session?.recordingState.state === 'recording'
            ? isRecordingAction
              ? 'Stopping…'
              : 'Stop recording'
            : isRecordingAction
              ? 'Starting…'
              : session?.stagedMedia?.kind === 'audio'
                ? 'Record again'
                : 'Record voice'}
        </button>

        {#if session?.stagedMedia?.kind === 'audio'}
          <span class="voice-duration"
            >{formatDuration(session.stagedMedia.durationMs)}</span
          >
          <button
            type="button"
            class="playback-button"
            aria-pressed={isPlaying}
            onclick={togglePlayback}
            >{isPlaying ? 'Stop playback' : 'Play'}</button
          >
          <button
            type="button"
            disabled={isDiscardingMedia}
            onclick={discardMedia}
            >{isDiscardingMedia ? 'Removing…' : 'Remove recording'}</button
          >
        {/if}
        {#if session?.recordingState.state === 'recording'}
          <span class="recording-status" role="status" aria-live="polite"
            >Recording voice note</span
          >
        {/if}
      </section>
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
