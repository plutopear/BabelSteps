<script lang="ts">
  import type { Snippet } from "svelte";

  let {
    title,
    onclose,
    children,
  }: { title: string; onclose: () => void; children: Snippet } = $props();

  let dlg: HTMLDialogElement;
  const tid = "dlg-" + Math.random().toString(36).slice(2);

  $effect(() => {
    dlg.showModal();
    return () => {
      if (dlg.open) dlg.close();
    };
  });
</script>

<dialog
  bind:this={dlg}
  aria-labelledby={tid}
  oncancel={(e) => {
    e.preventDefault();
    onclose();
  }}
>
  <h2 id={tid}>{title}</h2>
  {@render children()}
</dialog>
