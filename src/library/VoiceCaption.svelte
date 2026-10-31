<script lang="ts">
  import { getTranslations } from '../lib/i18n';
  import type { CaptureDetail, LanguageSetting } from '../lib/ipc-types';

  let {
    capture,
    language = 'english',
    onsave,
    onretry,
  }: {
    capture: CaptureDetail;
    language?: LanguageSetting;
    onsave: (
      captureId: string,
      revision: number,
      caption: string,
    ) => Promise<boolean>;
    onretry: (captureId: string, revision: number) => Promise<boolean>;
  } = $props();
  const t = $derived(getTranslations(language));
  let editing = $state(false);
  let draft = $state('');
  let revision = $state(0);
  let busy = $state(false);
  let editor: HTMLTextAreaElement | undefined = $state();
  let editorCaptureId = $state<string | null>(null);
  $effect(() => {
    if (capture.id !== editorCaptureId) {
      editorCaptureId = capture.id;
      editing = false;
      busy = false;
    }
  });
  $effect(() => {
    if (editing) editor?.focus();
  });

  function edit() {
    draft = capture.caption ?? '';
    revision = capture.captionRevision;
    editing = true;
  }
  async function save() {
    if (busy) return;
    busy = true;
    const id = capture.id;
    try {
      if (await onsave(id, revision, draft)) {
        if (capture.id === id) editing = false;
      }
    } finally {
      if (capture.id === id) busy = false;
    }
  }
  async function retry() {
    if (busy) return;
    busy = true;
    const id = capture.id;
    try {
      await onretry(id, capture.captionRevision);
    } finally {
      if (capture.id === id) busy = false;
    }
  }
</script>

<div class="voice-caption">
  {#if editing}
    <form
      onsubmit={(event) => {
        event.preventDefault();
        void save();
      }}
    >
      <label for="voice-caption-editor">{t.voiceCaption}</label>
      <textarea
        id="voice-caption-editor"
        bind:this={editor}
        bind:value={draft}
        rows="5"
        disabled={busy}
        aria-describedby="voice-caption-limit"></textarea>
      <p id="voice-caption-limit" class="detail-eyebrow">
        {t.voiceCaptionLimit}
      </p>
      <div class="voice-caption-actions">
        <button
          type="submit"
          class="secondary-action"
          disabled={busy || [...draft.trim()].length > 500}
        >
          {busy ? t.voiceCaptionSaving : t.voiceCaptionSave}
        </button>
        <button
          type="button"
          class="secondary-action"
          disabled={busy}
          onclick={() => (editing = false)}>{t.voiceCaptionCancel}</button
        >
      </div>
    </form>
  {:else}
    {#if capture.caption}
      {#if capture.captionSource === 'transcript_generated'}<p
          class="detail-eyebrow"
        >
          {t.voiceCaptionGenerated}
        </p>{/if}
      <p class="detail-caption">{capture.caption}</p>
    {/if}
    {#if capture.enrichmentStatus === 'pending'}
      <p role="status">{t.voiceTranscribing}</p>
    {:else if capture.enrichmentStatus === 'failed'}
      <p role="status">{t.voiceTranscriptionFailed}</p>
    {/if}
    <div class="voice-caption-actions">
      <button
        type="button"
        class="secondary-action"
        disabled={busy}
        onclick={edit}>{t.voiceCaptionEdit}</button
      >
      {#if !capture.caption && capture.media?.available && ['failed', 'skipped'].includes(capture.enrichmentStatus)}
        <button
          type="button"
          class="secondary-action"
          disabled={busy}
          onclick={retry}>{t.voiceTranscriptionRetry}</button
        >
      {/if}
    </div>
  {/if}
</div>

<style>
  .voice-caption {
    display: grid;
    gap: var(--space-3, 0.75rem);
  }
  form {
    display: grid;
    gap: var(--space-2, 0.5rem);
  }
  textarea {
    box-sizing: border-box;
    width: 100%;
    resize: vertical;
    font: inherit;
    color: inherit;
    background: var(--color-surface);
    border: 1px solid var(--color-input-border);
    border-radius: var(--radius-control);
    padding: 0.75rem;
  }
  .voice-caption-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
  }
</style>
