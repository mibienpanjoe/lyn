<script lang="ts">
  import ArrowLeftIcon from '@lucide/svelte/icons/arrow-left';
  import CheckIcon from '@lucide/svelte/icons/check';
  import CopyIcon from '@lucide/svelte/icons/copy';
  import ExternalLinkIcon from '@lucide/svelte/icons/external-link';
  import PauseIcon from '@lucide/svelte/icons/pause';
  import PlayIcon from '@lucide/svelte/icons/play';
  import Trash2Icon from '@lucide/svelte/icons/trash-2';

  import { copyTextToClipboard } from '../lib/clipboard';
  import { getTranslations } from '../lib/i18n';
  import type { CaptureDetail, LanguageSetting } from '../lib/ipc-types';

  interface Props {
    capture: CaptureDetail;
    compact?: boolean;
    backLabel?: string;
    playing?: boolean;
    busy?: boolean;
    deleting?: boolean;
    language?: LanguageSetting;
    onback?: () => void;
    onplay: () => void;
    onopen: () => void;
    ondelete: () => void;
  }

  let {
    capture,
    compact = false,
    backLabel = 'All captures',
    playing = false,
    busy = false,
    deleting = false,
    language = 'english',
    onback,
    onplay,
    onopen,
    ondelete,
  }: Props = $props();

  const t = $derived(getTranslations(language));
  const copyableText = $derived(
    capture.kind === 'text' ? capture.textBody : (capture.caption ?? null),
  );

  let confirmingDelete = $state(false);
  let copyState = $state<'idle' | 'copied' | 'failed'>('idle');
  let copyResetTimer: ReturnType<typeof setTimeout> | undefined;

  $effect(() => {
    // Reset confirmation state whenever selected capture changes
    capture.id;
    confirmingDelete = false;
    copyState = 'idle';
  });

  $effect(() => () => clearTimeout(copyResetTimer));

  async function copyCaptureText() {
    if (!copyableText) return;
    copyState = (await copyTextToClipboard(copyableText)) ? 'copied' : 'failed';
    clearTimeout(copyResetTimer);
    copyResetTimer = setTimeout(() => (copyState = 'idle'), 2000);
  }

  function fullDate(value: string) {
    return new Intl.DateTimeFormat(undefined, {
      dateStyle: 'medium',
      timeStyle: 'short',
    }).format(new Date(value));
  }

  function duration(milliseconds: number | null | undefined) {
    if (milliseconds == null) return '';
    const seconds = Math.round(milliseconds / 1000);
    return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, '0')}`;
  }
  function kindLabel() {
    if (capture.kind === 'image') return t.kindImage;
    if (capture.kind === 'audio') return t.kindAudio;
    return t.kindText;
  }

  function heading() {
    if (capture.kind !== 'text') return kindLabel();
    return (
      capture.textBody
        ?.split('\n')
        .map((line) => line.trim())
        .find(Boolean) ?? t.kindText
    );
  }
</script>

<article class="detail-panel" aria-labelledby="capture-detail-title">
  <header class="detail-header">
    {#if compact}
      <button
        class="detail-back"
        type="button"
        aria-label={t.backToScope(backLabel)}
        onclick={onback}
      >
        <ArrowLeftIcon aria-hidden="true" />
        {backLabel}
      </button>
    {/if}
    <div class="detail-header-identity">
      <h2 id="capture-detail-title" class="sr-only">{heading()}</h2>
      <p class="detail-eyebrow">
        {kindLabel()} · {fullDate(capture.capturedAt)}
      </p>
    </div>
  </header>

  <div class="detail-content">
    {#if capture.kind === 'text'}
      <p class="detail-text">{capture.textBody}</p>
    {:else if capture.kind === 'image'}
      {#if capture.media?.available}
        <img
          class="detail-image media-preview"
          src={capture.media.previewUri}
          alt={capture.caption ?? 'Captured screenshot'}
        />
      {:else}
        <div class="media-unavailable" role="status">
          Screenshot unavailable
        </div>
      {/if}
      {#if capture.caption}
        <p class="detail-caption">{capture.caption}</p>
      {/if}
    {:else}
      {#if capture.media?.available}
        <div class="audio-player">
          <button
            class="icon-label-button"
            type="button"
            disabled={busy}
            aria-label={playing ? 'Stop voice note' : 'Play voice note'}
            onclick={onplay}
          >
            {#if playing}<PauseIcon aria-hidden="true" />{:else}<PlayIcon
                aria-hidden="true"
              />{/if}
            {playing ? 'Stop' : 'Play'}
          </button>
          <span>{duration(capture.media.durationMs)}</span>
        </div>
      {:else}
        <div class="media-unavailable" role="status">
          Voice note unavailable
        </div>
      {/if}
      {#if capture.caption}
        <p class="detail-caption">{capture.caption}</p>
      {/if}
    {/if}

    {#if copyableText}
      <button
        class="copy-text-button"
        type="button"
        aria-label={t.copyText}
        onclick={copyCaptureText}
      >
        {#if copyState === 'copied'}<CheckIcon aria-hidden="true" />{:else}
          <CopyIcon aria-hidden="true" />
        {/if}
        {copyState === 'copied'
          ? t.copyTextCopied
          : copyState === 'failed'
            ? t.copyTextFailed
            : t.copyText}
      </button>
      <span class="sr-only" role="status">
        {copyState === 'copied'
          ? t.copyTextCopied
          : copyState === 'failed'
            ? t.copyTextFailed
            : ''}
      </span>
    {/if}

    <dl class="capture-metadata">
      <div>
        <dt>Captured</dt>
        <dd>{fullDate(capture.capturedAt)}</dd>
      </div>
      <div>
        <dt>Context</dt>
        <dd>{capture.context.name}</dd>
      </div>
      {#if capture.branchName}<div>
          <dt>Branch</dt>
          <dd title={capture.branchName}>{capture.branchName}</dd>
        </div>{/if}
      {#if capture.sourceApp}<div>
          <dt>Source</dt>
          <dd>{capture.sourceApp}</dd>
        </div>{/if}
    </dl>

    {#if capture.media?.available}
      <button
        class="open-media-button"
        type="button"
        disabled={busy || deleting}
        onclick={onopen}
      >
        <ExternalLinkIcon aria-hidden="true" />
        Open in default app
      </button>
    {/if}

    <div class="detail-actions">
      {#if confirmingDelete}
        <div class="delete-confirmation" role="alert">
          <span class="delete-warning-text"
            >Delete this capture permanently?</span
          >
          <div class="delete-buttons">
            <button
              class="cancel-delete-button"
              type="button"
              disabled={deleting}
              onclick={() => (confirmingDelete = false)}
            >
              Cancel
            </button>
            <button
              class="confirm-delete-button"
              type="button"
              disabled={deleting}
              onclick={ondelete}
            >
              {deleting ? 'Deleting…' : 'Delete'}
            </button>
          </div>
        </div>
      {:else}
        <button
          class="delete-capture-button"
          type="button"
          disabled={busy || deleting}
          onclick={() => (confirmingDelete = true)}
        >
          <Trash2Icon aria-hidden="true" />
          Delete capture
        </button>
      {/if}
    </div>
  </div>
</article>
