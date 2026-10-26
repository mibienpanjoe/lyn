<script lang="ts">
  import TrashIcon from '@lucide/svelte/icons/trash-2';
  import { tick } from 'svelte';
  import { getTranslations } from '../lib/i18n';
  import type { ContextRef, LanguageSetting } from '../lib/ipc-types';

  let {
    project,
    language,
    ondelete,
    busy = false,
    failure,
    onclear,
  }: {
    project: ContextRef;
    language: LanguageSetting;
    ondelete: (id: string) => Promise<void>;
    busy?: boolean;
    failure?: 'not_empty' | 'failed';
    onclear: (id: string) => void;
  } = $props();
  const t = $derived(getTranslations(language));
  let confirming = $state(false);
  const error = $derived(
    failure === 'not_empty'
      ? t.projectNotEmpty
      : failure === 'failed'
        ? t.projectDeleteFailed
        : null,
  );
  let trigger = $state<HTMLButtonElement>();

  function focusConfirmation(node: HTMLButtonElement) {
    node.focus();
  }
  async function cancel() {
    if (busy) return;
    confirming = false;
    onclear(project.id);
    await tick();
    trigger?.focus();
  }
  async function remove() {
    if (busy) return;
    try {
      await ondelete(project.id);
    } catch {
      // The parent retains the failure across navigation and component remounts.
    }
  }
</script>

<div class="project-actions" class:confirming class:failed={!!failure}>
  {#if confirming}
    <div
      role="alertdialog"
      aria-label={t.deleteProject}
      aria-describedby="project-delete-description"
      tabindex="-1"
      onkeydown={(event) => {
        if (event.key === 'Escape') {
          event.stopPropagation();
          void cancel();
        }
      }}
    >
      <p id="project-delete-description">
        {t.confirmProjectDelete(project.name)}
      </p>
      {#if error}<p role="alert">{error}</p>{/if}
      <div class="confirmation-buttons">
        <button
          type="button"
          disabled={busy}
          use:focusConfirmation
          onclick={cancel}>{t.cancelDelete}</button
        >
        <button
          type="button"
          class="danger"
          disabled={busy}
          onclick={remove}
          aria-busy={busy}>{busy ? t.deleting : t.confirmDelete}</button
        >
      </div>
    </div>
  {:else}
    <button
      bind:this={trigger}
      type="button"
      aria-label={t.deleteProject}
      title={t.deleteProject}
      disabled={busy}
      onclick={() => (confirming = true)}
      ><TrashIcon aria-hidden="true" /></button
    >
    {#if error}<p role="alert">{error}</p>{/if}
  {/if}
</div>

<style>
  .project-actions {
    margin-left: auto;
    min-width: 0;
  }
  .project-actions.confirming,
  .project-actions.failed {
    flex-basis: 100%;
    margin-left: 0;
  }
  button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-height: 32px;
    min-width: 32px;
    border: 1px solid var(--color-divider);
    border-radius: 6px;
    background: transparent;
    color: var(--color-text-muted);
    cursor: pointer;
    padding: 4px 8px;
    font: inherit;
  }
  button:hover {
    background: var(--color-surface-subtle);
  }
  button:disabled {
    opacity: 0.6;
    cursor: default;
  }
  button:focus-visible {
    outline: 2px solid var(--color-accent);
    outline-offset: 2px;
  }
  button :global(svg) {
    width: 16px;
    height: 16px;
  }
  [role='alertdialog'] {
    width: 100%;
    font-size: 12px;
  }
  p {
    margin: 0 0 8px;
    overflow-wrap: anywhere;
  }
  .confirmation-buttons {
    display: flex;
    gap: 8px;
    justify-content: flex-end;
  }
  .danger,
  [role='alert'] {
    color: var(--color-error);
  }
</style>
