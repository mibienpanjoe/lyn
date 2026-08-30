<script lang="ts">
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { onMount, tick } from 'svelte';

  import type { AppError, CaptureSession, ContextRef } from '../lib/ipc-types';
  import {
    CaptureCommandError,
    captureClient,
    type CaptureClient,
  } from './capture-client';

  interface Props {
    client?: CaptureClient;
    dismiss?: () => Promise<void> | void;
  }

  let {
    client = captureClient,
    dismiss = () => getCurrentWindow().close(),
  }: Props = $props();

  let draft = $state('');
  let session = $state<CaptureSession | null>(null);
  let contexts = $state<ContextRef[]>([]);
  let chooserOpen = $state(false);
  let contextName = $state('');
  let error = $state<AppError | null>(null);
  let isSaving = $state(false);
  let isCreatingContext = $state(false);
  let isComposing = $state(false);
  let cancelRequested = $state(false);
  let draftInput = $state<HTMLTextAreaElement>();
  let contextButton = $state<HTMLButtonElement>();
  let contextNameInput = $state<HTMLInputElement>();

  const selectedContext = $derived(
    session?.contextResolution.state === 'resolved'
      ? session.contextResolution.candidate.context
      : null,
  );

  onMount(() => {
    draftInput?.focus();
    void initialise();
  });

  async function initialise() {
    try {
      const [activeSession, savedContexts] = await Promise.all([
        client.getActiveSession(),
        client.listContexts(),
      ]);
      session = activeSession;
      contexts = savedContexts;
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
    await tick();
    const firstOption =
      document.querySelector<HTMLButtonElement>('.context-option');
    (firstOption ?? contextNameInput)?.focus();
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
      error = null;
      await closeChooser();
    } catch (caught) {
      error = toAppError(
        caught,
        'That context could not be selected. Try again.',
      );
    }
  }

  async function createContext() {
    if (!session || isCreatingContext || contextName.trim().length === 0)
      return;
    isCreatingContext = true;
    error = null;
    try {
      const context = await client.createStandaloneContext(contextName.trim());
      contexts = [...contexts, context];
      contextName = '';
      await selectContext(context);
    } catch (caught) {
      error = toAppError(
        caught,
        'The context could not be created. Try again.',
      );
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
      await client.saveText(session.sessionId, draft);
      await dismiss();
    } catch (caught) {
      error = toAppError(caught, 'The capture could not be saved. Try again.');
      await tick();
      draftInput?.focus();
    } finally {
      isSaving = false;
    }
  }

  async function cancel() {
    if (!session) {
      cancelRequested = true;
      return;
    }
    try {
      await client.cancel(session.sessionId);
      await dismiss();
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
      oncompositionend={() => (isComposing = false)}></textarea>

    <button
      bind:this={contextButton}
      class:context-required={!selectedContext}
      class="context-control"
      type="button"
      aria-expanded={chooserOpen}
      aria-controls="context-chooser"
      aria-label={selectedContext
        ? `Context ${selectedContext.name}. Change context`
        : 'Choose context'}
      onclick={() => (chooserOpen ? closeChooser() : openChooser())}
    >
      <span aria-hidden="true" class="context-mark"></span>
      <span>{selectedContext?.name ?? 'Choose context'}</span>
      <span aria-hidden="true" class="context-chevron">⌄</span>
    </button>

    {#if chooserOpen}
      <section
        id="context-chooser"
        class="context-chooser"
        aria-label="Choose context"
      >
        {#if contexts.length > 0}
          <p class="chooser-label">Saved contexts</p>
          <div class="context-list">
            {#each contexts as context (context.id)}
              <button
                class="context-option"
                type="button"
                aria-label={`Use context ${context.name}`}
                onclick={() => selectContext(context)}
              >
                <span>{context.name}</span>
                <span class="context-kind">{context.kind}</span>
              </button>
            {/each}
          </div>
        {/if}

        <form
          class="context-create"
          onsubmit={(event) => {
            event.preventDefault();
            void createContext();
          }}
        >
          <label for="context-name">New context</label>
          <div class="context-create-row">
            <input
              bind:this={contextNameInput}
              bind:value={contextName}
              id="context-name"
              name="context-name"
              aria-label="New context name"
              maxlength="100"
              autocomplete="off"
            />
            <button
              type="submit"
              disabled={isCreatingContext || contextName.trim().length === 0}
            >
              {isCreatingContext ? 'Creating…' : 'Create context'}
            </button>
          </div>
        </form>
      </section>
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
