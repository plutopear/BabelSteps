<script lang="ts">
  import { tick } from "svelte";
  import { api, describeError } from "./api";
  import { say } from "./announce.svelte";
  import { rs } from "./recording.svelte";
  import type { OpenedRecording } from "./types";

  let { onstopped }: { onstopped: (o: OpenedRecording) => void } = $props();

  const st = $derived(rs.status);
  let note = $state("");
  let error = $state("");
  let stopping = $state(false);
  let noteInput: HTMLInputElement | undefined = $state();
  // svelte-ignore state_referenced_locally
  let seenNotes = rs.status.noteRequests;

  // The "Add a note" hotkey raises noteRequests: bring the note box into focus.
  $effect(() => {
    const n = st.noteRequests;
    if (n > seenNotes) {
      seenNotes = n;
      say("Add a note: type your note and press Enter.");
      tick().then(() => noteInput?.focus());
    }
  });

  // The window was rebuilt from the tray by the "Add a note" shortcut, so the
  // request above happened before this page existed.
  $effect(() => {
    if (location.hash === "#note") {
      history.replaceState(null, "", location.pathname + location.search);
      say("Add a note: type your note and press Enter.");
      tick().then(() => noteInput?.focus());
    }
  });

  async function run(doing: string, f: () => Promise<void>) {
    error = "";
    try {
      await f();
    } catch (e) {
      error = describeError(e, doing);
    }
  }

  const togglePause = () =>
    run("pausing or resuming the recording", async () => {
      if (st.paused) await api.resumeRecording();
      else await api.pauseRecording();
      rs.status = { ...rs.status, paused: !st.paused };
    });

  function addNote(e: Event) {
    e.preventDefault();
    const text = note.trim();
    if (!text) return;
    if (text.length > 2000) {
      error = "That note is too long. Shorten it to 2000 characters or fewer, then press Add.";
      return;
    }
    run("adding the note", async () => {
      await api.addComment(text);
      note = "";
      say("Note added.");
      noteInput?.focus();
    });
  }

  async function stop() {
    error = "";
    stopping = true;
    rs.stopping = true;
    say("Saving the recording.");
    try {
      onstopped(await api.stopRecording());
    } catch (e) {
      error = describeError(e, "saving the recording") + " The recording may still be running; check the state above.";
      stopping = false;
    } finally {
      rs.stopping = false;
    }
  }
</script>

<section class="rec-panel" aria-labelledby="rec-h" aria-busy={stopping}>
  <h1 id="rec-h" class="rec-title">
    <span class="dot" class:paused={st.paused} aria-hidden="true"></span>
    {st.paused ? "Paused" : "Recording"}
  </h1>

  {#if stopping}
    <p class="busy-line" role="status"><span class="spinner" aria-hidden="true"></span> Saving the recording…</p>
  {:else}
    <p>
      Keystroke capture is <strong>{st.keystrokes ? "on" : "off"}</strong>.
      {#if st.keystrokes}Text you type is saved in the steps. Detected password fields are not recorded.{/if}
      Anything visible on screen appears in screenshots either way.
    </p>
    <p class="count">Steps recorded: <strong>{st.steps}</strong></p>
    {#if st.paused}<p class="hint">Paused: nothing is being recorded until you resume.</p>{/if}

    {#if st.withheld > 0}
      <p class="warn-line">
        {st.withheld} {st.withheld === 1 ? "screenshot was" : "screenshots were"} withheld because the password check could not finish.
        The step is kept without its picture.
      </p>
    {/if}
    {#if st.warnings.length > 0}
      <ul class="warn-line" aria-label="Recording warnings">
        {#each st.warnings as w}<li>{w}</li>{/each}
      </ul>
    {/if}
  {/if}

  {#if error}<p class="error" role="alert">{error}</p>{/if}

  <div class="row">
    <button type="button" onclick={togglePause} disabled={stopping}>{st.paused ? "Resume" : "Pause"}</button>
    <button type="button" class="danger" onclick={stop} disabled={stopping}>Stop recording</button>
  </div>

  <form class="note-form" onsubmit={addNote}>
    <label class="grow">Add a note to the steps
      <input type="text" bind:this={noteInput} bind:value={note} maxlength="2000" disabled={stopping} autocomplete="off" />
    </label>
    <button type="submit" disabled={stopping || !note.trim()}>Add</button>
  </form>

  {#if st.hotkeys.length > 0}
    <h2 class="small-h">Shortcuts</h2>
    <ul class="hotkeys">
      {#each st.hotkeys as h}
        <li>
          <strong>{h.action}:</strong> <kbd>{h.keys}</kbd>
          {#if !h.registered}
            <span class="hint"> Another app uses this shortcut; use the buttons instead.</span>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
</section>
