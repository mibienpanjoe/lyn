<script lang="ts">
  import ArrowLeftIcon from '@lucide/svelte/icons/arrow-left';
  import ExternalLinkIcon from '@lucide/svelte/icons/external-link';
  import PauseIcon from '@lucide/svelte/icons/pause';
  import PlayIcon from '@lucide/svelte/icons/play';

  import type { CaptureDetail } from '../lib/ipc-types';

  interface Props {
    capture: CaptureDetail;
    compact?: boolean;
    playing?: boolean;
    busy?: boolean;
    onback?: () => void;
    onplay: () => void;
    onopen: () => void;
  }

  let {
    capture,
    compact = false,
    playing = false,
    busy = false,
    onback,
    onplay,
    onopen,
  }: Props = $props();

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
</script>

<article class="detail-panel" aria-labelledby="capture-detail-title">
  <header class="detail-header">
    {#if compact}
      <button class="icon-label-button" type="button" onclick={onback}>
        <ArrowLeftIcon aria-hidden="true" />
        Back
      </button>
    {/if}
    <div>
      <p class="detail-eyebrow">{capture.kind} capture</p>
      <h2 id="capture-detail-title">{capture.context.name}</h2>
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
        disabled={busy}
        onclick={onopen}
      >
        <ExternalLinkIcon aria-hidden="true" />
        Open in default app
      </button>
    {/if}
  </div>
</article>
