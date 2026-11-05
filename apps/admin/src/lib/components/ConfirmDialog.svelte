<script lang="ts">
  import { onMount } from 'svelte';
  let {
    title,
    description,
    action,
    busy = false,
    error,
    onconfirm,
    oncancel
  }: {
    title: string;
    description: string;
    action: string;
    busy?: boolean;
    error?: string;
    onconfirm: () => void;
    oncancel: () => void;
  } = $props();
  let element: HTMLDialogElement;
  onMount(() => {
    element.showModal();
    return () => {
      element.close();
    };
  });
</script>

<dialog
  bind:this={element}
  class="confirm-dialog"
  aria-labelledby="confirm-title"
  aria-describedby="confirm-description"
  oncancel={(event) => {
    event.preventDefault();
    if (!busy) {
      oncancel();
    }
  }}
>
  <h2 id="confirm-title">{title}</h2>
  <p id="confirm-description">{description}</p>
  {#if error}<p class="notice notice-error" role="alert">{error}</p>{/if}
  <div class="dialog-actions">
    <button class="button button-secondary" disabled={busy} onclick={oncancel}>Cancelar</button>
    <button class="button button-danger" disabled={busy} onclick={onconfirm}
      >{busy ? 'Aplicando…' : action}</button
    >
  </div>
</dialog>
