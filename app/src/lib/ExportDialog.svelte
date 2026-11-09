<script lang="ts">
  import { onMount } from "svelte";
  import { api, describeError, pickFolder, pickSavePath, toAppError } from "./api";
  import { say } from "./announce.svelte";
  import Modal from "./Modal.svelte";
  import type { ExportChoices, ExportSummary, Finding, Recording } from "./types";

  let {
    title,
    firstStepMs = 0,
    doc,
    onreplace,
    save,
    onclose,
  }: {
    title: string;
    firstStepMs?: number;
    /** The editor's document as it is now, saved or not. Scanned, never changed here. */
    doc: Recording;
    /** Hands the editor a new copy of the document; the editor saves it like any other edit. */
    onreplace: (rec: Recording) => void;
    save: () => Promise<boolean>;
    onclose: () => void;
  } = $props();

  // Pre-export scan (B13, B14, E5). A warning only: findings never block export.
  // `null` while scanning. `scanFailed` is shown instead of any "clean" message.
  let findings = $state<Finding[] | null>(null);
  let scanFailed = $state(false);
  let hiding = $state(false);
  let summaryEl: HTMLElement | undefined = $state();
  let scanSeq = 0;
  const NOTHING_FLAGGED = "Nothing flagged. Detection can miss things, so still review each step.";

  async function scan(rec: Recording, announce = true) {
    const mine = ++scanSeq;
    try {
      const found = await api.scanRecording($state.snapshot(rec) as Recording);
      if (mine !== scanSeq) return;
      findings = found;
      scanFailed = false;
      if (announce) say(found.length ? `Possible sensitive text found: ${found.length}.` : NOTHING_FLAGGED);
    } catch (e) {
      if (mine !== scanSeq) return;
      describeError(e, "checking for sensitive text"); // logs the kind only
      findings = [];
      scanFailed = true;
      if (announce) say("The check for sensitive text could not run.");
    }
  }
  onMount(() => {
    scan(doc);
  });

  async function hide(f: Finding) {
    if (hiding) return;
    hiding = true;
    try {
      const next = await api.hideFinding($state.snapshot(doc) as Recording, f);
      if (next) onreplace(next);
      // Scan what the editor now holds (`doc` updates a moment later).
      await scan(next ?? doc, false);
      say(
        (next ? "Hidden. " : "That text had changed, so nothing was hidden. ") +
          (findings?.length ? `Possible sensitive text found: ${findings.length}.` : scanFailed ? "The check could not run again." : NOTHING_FLAGGED),
      );
    } catch (e) {
      describeError(e, "hiding text");
      say("That text could not be hidden. Try again, or edit it in the editor.");
    } finally {
      hiding = false;
      summaryEl?.focus();
    }
  }

  let format = $state<"pdf" | "markdown" | "html">("pdf");
  let headerFooter = $state(false);
  // PSR-2 and PSR-6: both off by default, every time the dialog opens.
  let stepTimes = $state(false);
  let technicalDetails = $state(false);
  let c1 = $state(false);
  let c2 = $state(false);
  let c3 = $state(false);
  let busy = $state(false);
  let error = $state("");
  let result = $state<{ summary: ExportSummary; where: string } | null>(null);

  const ready = $derived(c1 && c2 && c3 && !busy);

  function fileName() {
    const base = title.replace(/[\\/:*?"<>|]+/g, " ").replace(/\s+/g, " ").trim() || "BabelSteps guide";
    return base + (format === "html" ? ".html" : ".pdf");
  }

  async function run() {
    error = "";
    result = null;
    try {
      const dest = format === "markdown" ? await pickFolder() : await pickSavePath(fileName(), format);
      if (!dest) return; // the user cancelled the picker
      busy = true;
      say("Saving, then exporting.");
      // Exports read the saved recording, so save first.
      if (!(await save())) {
        error = "The recording could not be saved, so nothing was exported. Fix the save problem shown in the editor and try again.";
        return;
      }
      // Times use the offset in force when the recording was made (or now, if it has no timed steps).
      const choices: ExportChoices = {
        stepTimes,
        technicalDetails,
        utcOffsetMinutes: -new Date(firstStepMs || Date.now()).getTimezoneOffset(),
      };
      const summary =
        format === "pdf"
          ? await api.exportPdf(dest, headerFooter, choices)
          : format === "html"
            ? await api.exportSingleHtml(dest, choices)
            : await api.exportMarkdown(dest, choices);
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
      <label><input type="radio" bind:group={format} value="html" /> Single HTML file (one file with the pictures inside)</label>
    </fieldset>

    {#if format === "pdf"}
      <label class="check">
        <input type="checkbox" bind:checked={headerFooter} disabled={busy} />
        Add a header and footer (title and page number)
      </label>
    {:else if format === "html"}
      <p class="hint">
        The pictures are stored inside the file, so it is about a third larger than the pictures
        themselves. Open it in any web browser. It does not load anything from the internet.
      </p>
    {:else}
      <p class="hint">You will choose a folder. It must be empty.</p>
    {/if}

    <fieldset disabled={busy}>
      <legend>Include</legend>
      <label class="check">
        <input type="checkbox" bind:checked={stepTimes} />
        The time of each step
      </label>
      <label class="check">
        <input type="checkbox" bind:checked={technicalDetails} />
        A technical details section (step counts, Windows version, and the program and screen element for each step)
      </label>
    </fieldset>

    <section class="scan" aria-labelledby="scan-h">
      <h3 id="scan-h" tabindex="-1" bind:this={summaryEl}>
        {#if findings === null}
          Checking for sensitive text…
        {:else if scanFailed}
          The check for sensitive text could not run
        {:else if findings.length}
          Possible sensitive text found: {findings.length}
        {:else}
          {NOTHING_FLAGGED}
        {/if}
      </h3>
      {#if scanFailed}
        <p class="hint">
          Fix any problem with your custom words in Settings and open this window again. You can still export, but
          review every step yourself first.
        </p>
      {:else if findings !== null}
        {#if findings.length}
          <ul aria-labelledby="scan-h" class="findings">
            {#each findings as f (`${f.blockId}|${f.field.kind}|${"index" in f.field ? f.field.index : ""}|${f.start}`)}
              <li>
                <span class="where">{f.location}: {f.kindLabel} <span class="hint" aria-hidden="true">({f.preview})</span></span>
                <button type="button" onclick={() => hide(f)} disabled={busy || hiding} aria-label={`Hide it: ${f.kindLabel} in ${f.location}`}>
                  Hide it
                </button>
              </li>
            {/each}
          </ul>
        {/if}
        <p class="hint">
          {findings.length ? "Detection can miss things, so still review each step. " : ""}This check reads the text
          of the document, not the pictures. You can export either way.
        </p>
      {/if}
    </section>

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
        {format === "markdown" ? "Choose folder and export…" : "Choose file and export…"}
      </button>
    </div>
    {#if !c1 || !c2 || !c3}
      <p class="hint">Tick all three boxes to enable export.</p>
    {/if}
  {/if}
</Modal>

<style>
  .scan { border: 1px solid var(--border); border-radius: 6px; margin: 0.75rem 0; padding: 0.5rem 0.8rem; }
  .scan h3 { font-size: 1rem; margin: 0.2rem 0 0.4rem; }
  .findings { list-style: none; padding: 0; margin: 0 0 0.5rem; display: grid; gap: 0.35rem; }
  .findings li { display: flex; gap: 0.5rem; align-items: center; justify-content: space-between; }
  .where { min-width: 0; overflow-wrap: anywhere; }
</style>
