<script lang="ts">
  import { tick } from "svelte";
  import { api, describeError } from "./api";
  import { say } from "./announce.svelte";
  import ExportDialog from "./ExportDialog.svelte";
  import Modal from "./Modal.svelte";
  import StepShot from "./StepShot.svelte";
  import type { Block, CalloutKind, Recording } from "./types";

  let { initial, onback }: { initial: Recording; onback: () => void } = $props();

  // svelte-ignore state_referenced_locally
  let rec = $state<Recording>(structuredClone($state.snapshot(initial)) as Recording);
  // svelte-ignore state_referenced_locally
  let savedJson = $state(JSON.stringify(rec));
  const dirty = $derived(JSON.stringify(rec) !== savedJson);

  let saving = $state(false);
  let error = $state("");
  let showExport = $state(false);
  let confirmBack = $state(false);
  let activeId = $state<string | null>(null);
  let undo = $state<{ block: Block; index: number } | null>(null);
  let root: HTMLElement | undefined = $state();

  function newId() {
    return "b" + Date.now().toString(36) + Math.random().toString(36).slice(2, 8);
  }

  async function save(): Promise<boolean> {
    saving = true;
    error = "";
    try {
      const snap = $state.snapshot(rec) as Recording;
      if (!snap.title.trim()) {
        rec.title = savedTitle;
        snap.title = savedTitle;
      }
      await api.save(snap);
      savedTitle = snap.title;
      savedJson = JSON.stringify(snap);
      say("Saved.");
      return true;
    } catch (e) {
      error = describeError(e, "saving your changes") + " Your changes are still here and not yet saved.";
      return false;
    } finally {
      saving = false;
    }
  }
  // svelte-ignore state_referenced_locally
  let savedTitle = initial.title;

  async function back(force = false) {
    if (dirty && !force) {
      confirmBack = true;
      return;
    }
    try {
      await api.close();
    } catch (e) {
      describeError(e, "closing the recording");
    }
    onback();
  }

  // ---- blocks ----

  const stepNumber = (i: number) => rec.blocks.slice(0, i + 1).filter((b) => b.type === "step").length;

  function describeBlock(b: Block, i: number): string {
    switch (b.type) {
      case "step": return `Step ${stepNumber(i)}`;
      case "heading": return "Heading";
      case "text": return "Text";
      case "callout": return b.kind[0].toUpperCase() + b.kind.slice(1);
      case "list": return "List";
      case "image": return "Image";
    }
  }

  async function focusIn(id: string, sel: string) {
    await tick();
    root?.querySelector<HTMLElement>(`[data-block="${id}"] ${sel}`)?.focus();
  }

  function insert(block: Block) {
    const at = activeId ? rec.blocks.findIndex((b) => b.id === activeId) + 1 : rec.blocks.length;
    const idx = at > 0 ? at : rec.blocks.length;
    rec.blocks.splice(idx, 0, block);
    activeId = block.id;
    say(`${describeBlock(block, idx)} block added at position ${idx + 1}.`);
    focusIn(block.id, "[data-first]");
  }
  const addText = () => insert({ type: "text", id: newId(), text: "" });
  const addHeading = () => insert({ type: "heading", id: newId(), level: 2, text: "" });
  const addCallout = (kind: CalloutKind) => insert({ type: "callout", id: newId(), kind, text: "" });

  function move(i: number, d: -1 | 1) {
    const j = i + d;
    if (j < 0 || j >= rec.blocks.length) return;
    const [b] = rec.blocks.splice(i, 1);
    rec.blocks.splice(j, 0, b);
    say(`Moved to position ${j + 1} of ${rec.blocks.length}.`);
    const edge = d === -1 ? j === 0 : j === rec.blocks.length - 1;
    focusIn(b.id, edge ? `[data-act="${d === -1 ? "down" : "up"}"]` : `[data-act="${d === -1 ? "up" : "down"}"]`);
  }

  function remove(i: number) {
    const [b] = rec.blocks.splice(i, 1);
    undo = { block: b, index: i };
    activeId = null;
    say("Block deleted. Use Undo delete to bring it back.");
    const next = rec.blocks[Math.min(i, rec.blocks.length - 1)];
    if (next) focusIn(next.id, "[data-act]");
    else tick().then(() => root?.querySelector<HTMLElement>("[data-undo]")?.focus());
  }

  function undoDelete() {
    if (!undo) return;
    rec.blocks.splice(Math.min(undo.index, rec.blocks.length), 0, undo.block);
    const id = undo.block.id;
    undo = null;
    say("Block restored.");
    focusIn(id, "[data-act]");
  }

  function blockKey(e: KeyboardEvent, i: number) {
    if (e.altKey && (e.key === "ArrowUp" || e.key === "ArrowDown")) {
      e.preventDefault();
      move(i, e.key === "ArrowUp" ? -1 : 1);
    }
  }

  function actionText(b: Block & { type: "step" }): string {
    const a = b.action;
    switch (a.kind) {
      case "click": return `${a.button[0].toUpperCase() + a.button.slice(1)}-click at (${a.x}, ${a.y})`;
      case "text": return "Typed text";
      case "shortcut": return `Keyboard shortcut: ${a.keys}`;
      case "comment": return "Comment";
      case "manual": return "Added by hand";
    }
  }

  function windowKey(e: KeyboardEvent) {
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "s") {
      e.preventDefault();
      if (!saving) save();
    }
  }

  $effect(() => {
    const h = (e: BeforeUnloadEvent) => {
      if (dirty) e.preventDefault();
    };
    window.addEventListener("beforeunload", h);
    return () => window.removeEventListener("beforeunload", h);
  });
</script>

<svelte:window onkeydown={windowKey} />

<section class="editor" bind:this={root} aria-label="Document editor">
  <div class="editor-head">
    <button type="button" onclick={() => back()}>← Library</button>
    <label class="title-field">
      <span class="sr-only">Recording title</span>
      <input type="text" bind:value={rec.title} aria-label="Recording title" />
    </label>
    <span class="status" role="status">{saving ? "Saving…" : dirty ? "Unsaved changes" : "All changes saved"}</span>
    <button type="button" class="primary" onclick={save} disabled={saving || !dirty}>Save</button>
    <button type="button" onclick={() => (showExport = true)}>Export…</button>
  </div>

  {#if error}
    <p class="error" role="alert">{error}</p>
  {/if}

  <div class="insert-bar" role="toolbar" aria-label="Add a block">
    <span class="hint">Add after the block you last used:</span>
    <button type="button" onclick={addText}>Text</button>
    <button type="button" onclick={addHeading}>Heading</button>
    <button type="button" onclick={() => addCallout("note")}>Note</button>
    <button type="button" onclick={() => addCallout("tip")}>Tip</button>
    <button type="button" onclick={() => addCallout("warning")}>Warning</button>
    {#if undo}
      <button type="button" data-undo onclick={undoDelete}>Undo delete</button>
    {/if}
  </div>
  <p class="hint">
    Reorder with the Move buttons, or press Alt + Up / Down Arrow while focus is inside a block.
    Ctrl + S saves.
  </p>

  {#if rec.blocks.length === 0}
    <p class="empty">This recording has no content yet. Add a text, heading, or note block above.</p>
  {/if}

  <ol class="blocks">
    {#each rec.blocks as block, i (block.id)}
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <li
        class="block {block.type}"
        data-block={block.id}
        aria-label={`${describeBlock(block, i)}, block ${i + 1} of ${rec.blocks.length}`}
        onfocusin={() => (activeId = block.id)}
        onkeydown={(e) => blockKey(e, i)}
      >
        <div class="block-head">
          <strong>{describeBlock(block, i)}</strong>
          {#if block.type === "step"}<span class="hint">{actionText(block)}</span>{/if}
          <span class="spacer"></span>
          <button type="button" data-act="up" disabled={i === 0} onclick={() => move(i, -1)} aria-label={`Move block ${i + 1} up`}>Move up</button>
          <button type="button" data-act="down" disabled={i === rec.blocks.length - 1} onclick={() => move(i, 1)} aria-label={`Move block ${i + 1} down`}>Move down</button>
          <button type="button" data-act="delete" class="danger" onclick={() => remove(i)} aria-label={`Delete block ${i + 1}`}>Delete</button>
        </div>

        {#if block.type === "step"}
          <label>Title
            <input type="text" data-first bind:value={block.title} />
          </label>
          <label>Description
            <textarea rows="3" bind:value={block.description}></textarea>
          </label>
          {#if block.action.kind === "text"}
            <div class="typed">
              <span class="label">Typed text recorded for this step (check it before exporting)</span>
              <code>{block.action.text === "" ? "(nothing recorded)" : block.action.text}</code>
            </div>
          {/if}
          <StepShot
            path={block.screenshot}
            withheld={block.screenshot_withheld}
            bind:redactions={block.redactions}
            label={`Screenshot for step ${stepNumber(i)}`}
          />
        {:else if block.type === "heading"}
          <div class="row">
            <label>Level
              <select bind:value={block.level}>
                <option value={1}>1 (largest)</option>
                <option value={2}>2</option>
                <option value={3}>3</option>
              </select>
            </label>
            <label class="grow">Heading text
              <input type="text" data-first bind:value={block.text} />
            </label>
          </div>
        {:else if block.type === "text"}
          <label>Text
            <textarea rows="4" data-first bind:value={block.text}></textarea>
          </label>
        {:else if block.type === "callout"}
          <div class="row">
            <label>Kind
              <select bind:value={block.kind}>
                <option value="note">Note</option>
                <option value="tip">Tip</option>
                <option value="warning">Warning</option>
              </select>
            </label>
            <label class="grow">Text
              <textarea rows="2" data-first bind:value={block.text}></textarea>
            </label>
          </div>
        {:else if block.type === "list"}
          <label class="check"><input type="checkbox" bind:checked={block.ordered} /> Numbered list</label>
          <label>Items (one per line)
            <textarea
              rows="4"
              data-first
              value={block.items.join("\n")}
              oninput={(e) => (block.items = e.currentTarget.value.split("\n"))}
            ></textarea>
          </label>
        {:else if block.type === "image"}
          <label>Caption
            <input type="text" data-first bind:value={block.caption} />
          </label>
          <StepShot path={block.image} label={block.caption || "Image in the document"} />
          <p class="hint">Redaction boxes are not available for image blocks yet. Review this image carefully.</p>
        {/if}
      </li>
    {/each}
  </ol>
</section>

{#if confirmBack}
  <Modal title="You have unsaved changes" onclose={() => (confirmBack = false)}>
    <p>If you leave now, your latest edits will be lost.</p>
    <div class="actions">
      <button type="button" onclick={() => (confirmBack = false)}>Keep editing</button>
      <button type="button" class="danger" onclick={() => { confirmBack = false; back(true); }}>Discard changes</button>
      <button
        type="button"
        class="primary"
        onclick={async () => {
          confirmBack = false;
          if (await save()) back(true);
        }}
      >Save and leave</button>
    </div>
  </Modal>
{/if}

{#if showExport}
  <ExportDialog title={rec.title} {save} onclose={() => (showExport = false)} />
{/if}
