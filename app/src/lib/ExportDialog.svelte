<script lang="ts">
  import { api, describeError, pickFolder, pickSavePath, toAppError } from "./api";
  import { say } from "./announce.svelte";
  import Modal from "./Modal.svelte";
  import type { ExportSummary } from "./types";

  let {
    title,
    save,
    onclose,
  }: { title: string; save: () => Promise<boolean>; onclose: () => void } = $props();

  let format = $state<"pdf" | "markdown">("pdf");
  let headerFooter = $state(false);
  let c1 = $state(false);
  let c2 = $state(false);
  let c3 = $state(false);
  let busy = $state(false);
  let error = $state("");
  let result = $state<{ summary: ExportSummary; where: string } | null>(null);

  const ready = $derived(c1 && c2 && c3 && !busy);

  function fileName() {
    const base = title.replace(/[\\/:*?"<>|]+/g, " ").replace(/\s+/g, " ").trim() || "BabelSteps guide";
    return base + ".pdf";
  }

  async function run() {
    error = "";
    result = null;
    try {
      const dest = format === "pdf" ? await pickSavePath(fileName()) : await pickFolder();
      if (!dest) return; // the user cancelled the picker
      busy = true;
      say("Saving, then exporting.");
      // Exports read the saved recording, so save first.
      if (!(await save())) {
        error = "The recording could not be saved, so nothing was exported. Fix the save problem shown in the editor and try again.";
        return;
      }
      const summary =
        format === "pdf" ? await api.exportPdf(dest, headerFooter) : await api.exportMarkdown(dest);
      result = { summary, where: dest };
      say("Export finished.");
    } catch (e) {
      error = describeError(e, "exporting");
      if (toAppError(e).kind === "output_not_empty") {
        error = "That folder is not empty. BabelSteps only exports into an empty folder so it never overwrites your files. Nothing was changed. Choose an empty folder (make a new one if needed) and try again.";
      }
    } finally {
      busy = false;
    }
  }
</script>

<Modal title="Export" onclose={() => !busy && onclose()}>
  {#if result}
    <div class="result" role="status">
      <p><strong>Export finished.</strong> Saved to <code>{result.where}</code></p>
      <ul>
        <li>{result.summary.steps} step{result.summary.steps === 1 ? "" : "s"} exported</li>
        <li>{result.summary.imagesWritten} screenshot{result.summary.imagesWritten === 1 ? "" : "s"} included</li>
        <li>{result.summary.imagesWithRedactions} screenshot{result.summary.imagesWithRedactions === 1 ? "" : "s"} with redaction boxes</li>
        <li>{result.summary.withheld} screenshot{result.summary.withheld === 1 ? "" : "s"} withheld</li>
      </ul>
      <p class="hint">
        Automatic detection can miss things. Open the exported file and look through it before you share it.
      </p>
    </div>
    <div class="actions">
      <button type="button" class="primary" onclick={onclose}>Done</button>
    </div>
  {:else}
    <fieldset disabled={busy}>
      <legend>Format</legend>
      <label><input type="radio" bind:group={format} value="pdf" /> PDF document (one file)</label>
      <label><input type="radio" bind:group={format} value="markdown" /> Markdown (a folder with document.md and images)</label>
    </fieldset>

    {#if format === "pdf"}
      <label class="check">
        <input type="checkbox" bind:checked={headerFooter} disabled={busy} />
        Add a header and footer (title and page number)
      </label>
    {:else}
      <p class="hint">You will choose a folder. It must be empty.</p>
    {/if}

    <fieldset disabled={busy}>
      <legend>Before you export, confirm you have:</legend>
      <label class="check"><input type="checkbox" bind:checked={c1} /> Reviewed every screenshot</label>
      <label class="check"><input type="checkbox" bind:checked={c2} /> Checked the typed text in the steps</label>
      <label class="check"><input type="checkbox" bind:checked={c3} /> Added redaction boxes where needed</label>
    </fieldset>
    <p class="hint">
      BabelSteps tries to spot password fields, but automatic detection can miss things. Exports
      are not guaranteed to be free of sensitive information, so check the result before sharing it.
    </p>

    {#if error}
      <p class="error" role="alert">{error}</p>
    {/if}
    {#if busy}
      <p role="status" class="busy"><span class="spinner" aria-hidden="true"></span> Saving and exporting. This can take several seconds for long recordings. Please wait.</p>
    {/if}

    <div class="actions">
      <button type="button" onclick={onclose} disabled={busy}>Cancel</button>
      <button type="button" class="primary" onclick={run} disabled={!ready}>
        {format === "pdf" ? "Choose file and export…" : "Choose folder and export…"}
      </button>
    </div>
    {#if !c1 || !c2 || !c3}
      <p class="hint">Tick all three boxes to enable export.</p>
    {/if}
  {/if}
</Modal>
