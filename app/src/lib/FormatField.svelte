<script lang="ts">
  // A multi-line field for restricted inline Markdown with Bold, Italic,
  // Underline and Code buttons (B6). Ctrl+B and Ctrl+I work inside the field.
  import { tick } from "svelte";
  import { say } from "./announce.svelte";
  import { BOLD, CODE, ITALIC, UNDERLINE, wrapLines, wrapSelection, type Marker } from "./format";

  let {
    label,
    value,
    onchange,
    rows = 3,
    first = false,
    perLine = false,
  }: {
    label: string;
    value: string;
    onchange: (v: string) => void;
    rows?: number;
    first?: boolean;
    /** Format each selected line on its own (list items). */
    perLine?: boolean;
  } = $props();

  const uid = $props.id();
  let area: HTMLTextAreaElement | undefined = $state();

  const buttons: { marker: Marker; name: string; key: string; shown: string }[] = [
    { marker: BOLD, name: "Bold", key: "Ctrl+B", shown: "B" },
    { marker: ITALIC, name: "Italic", key: "Ctrl+I", shown: "I" },
    { marker: UNDERLINE, name: "Underline", key: "", shown: "U" },
    { marker: CODE, name: "Code", key: "", shown: "</>" },
  ];

  async function format(marker: Marker, name: string) {
    if (!area) return;
    const r = (perLine ? wrapLines : wrapSelection)(area.value, area.selectionStart, area.selectionEnd, marker);
    if (r.text === area.value) return;
    onchange(r.text);
    area.value = r.text;
    area.focus();
    area.setSelectionRange(r.selStart, r.selEnd);
    say(`${name} applied or removed.`);
    await tick();
    // The parent may re-render the value; keep the selection where we put it.
    area?.setSelectionRange(r.selStart, r.selEnd);
  }

  function key(e: KeyboardEvent) {
    if (!(e.ctrlKey || e.metaKey) || e.altKey || e.shiftKey) return;
    const k = e.key.toLowerCase();
    const b = k === "b" ? buttons[0] : k === "i" ? buttons[1] : null;
    if (!b) return;
    e.preventDefault();
    format(b.marker, b.name);
  }
</script>

<div class="format-field">
  <span class="field-label" id="{uid}-label">{label}</span>
  <div class="format-bar" role="group" aria-label="Formatting for {label}">
    {#each buttons as b (b.marker)}
      <button
        type="button"
        class="fmt fmt-{b.name.toLowerCase()}"
        aria-label={b.key ? `${b.name} (${b.key})` : b.name}
        title={b.key ? `${b.name} (${b.key})` : b.name}
        onclick={() => format(b.marker, b.name)}
      >{b.shown}</button>
    {/each}
  </div>
  <textarea
    bind:this={area}
    {rows}
    data-first={first ? "" : undefined}
    aria-labelledby="{uid}-label"
    {value}
    oninput={(e) => onchange(e.currentTarget.value)}
    onkeydown={key}
  ></textarea>
</div>

<style>
  .format-field { display: flex; flex-direction: column; gap: 0.25rem; margin: 0.5rem 0; flex: 1; }
  .field-label { font-weight: 600; font-size: 0.9rem; }
  .format-bar { display: flex; gap: 0.25rem; }
  .fmt { min-width: 2rem; padding: 0.1rem 0.5rem; }
  .fmt-bold { font-weight: 700; }
  .fmt-italic { font-style: italic; }
  .fmt-underline { text-decoration: underline; }
  .fmt-code { font-family: ui-monospace, Consolas, monospace; }
</style>
