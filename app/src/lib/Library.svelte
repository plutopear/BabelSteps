<script lang="ts">
  import { api, describeError } from "./api";
  import { say } from "./announce.svelte";
  import Modal from "./Modal.svelte";
  import type { LibraryListing, OpenedRecording, RecordingSummary } from "./types";

  let { onopen }: { onopen: (o: OpenedRecording) => void } = $props();

  let listing = $state<LibraryListing | null>(null);
  let loading = $state(true);
  let error = $state("");
  let newTitle = $state("");
  let busy = $state(false);
  let renaming = $state<{ rec: RecordingSummary; title: string } | null>(null);
  let deleting = $state<RecordingSummary | null>(null);

  async function refresh() {
    loading = true;
    try {
      listing = await api.list();
      error = "";
    } catch (e) {
      error = describeError(e, "reading your library");
    } finally {
      loading = false;
    }
  }
  refresh();

  async function run(doing: string, f: () => Promise<void>) {
    busy = true;
    error = "";
    try {
      await f();
    } catch (e) {
      error = describeError(e, doing);
    } finally {
      busy = false;
    }
  }

  const create = (ev: SubmitEvent) => {
    ev.preventDefault();
    if (!newTitle.trim()) {
      error = "Type a name for the new recording first.";
      return;
    }
    run("creating the recording", async () => onopen(await api.create(newTitle.trim())));
  };
  const open = (r: RecordingSummary) =>
    run("opening the recording", async () => onopen(await api.open(r.path)));

  const doRename = () =>
    run("renaming the recording", async () => {
      const r = renaming;
      if (!r) return;
      if (!r.title.trim()) throw { kind: "invalid_argument", message: "" };
      await api.open(r.rec.path);
      try {
        await api.rename(r.title.trim());
      } finally {
        await api.close();
      }
      renaming = null;
      say("Recording renamed.");
      await refresh();
    });

  const doDelete = () =>
    run("deleting the recording", async () => {
      const r = deleting;
      if (!r) return;
      await api.remove(r.path);
      deleting = null;
      say("Recording deleted.");
      await refresh();
    });

  const when = (ms: number) => (ms ? new Date(ms).toLocaleString() : "unknown");
</script>

<section aria-labelledby="lib-h">
  <h1 id="lib-h">Library</h1>
  {#if listing}
    <p class="hint">Recordings are stored on this computer in: <code>{listing.folder}</code></p>
  {/if}

  <form class="new" onsubmit={create}>
    <label>New recording name
      <input type="text" bind:value={newTitle} placeholder="For example: Reset a password" />
    </label>
    <button type="submit" class="primary" disabled={busy}>Create and open</button>
  </form>

  {#if error}
    <p class="error" role="alert">{error}</p>
  {/if}

  {#if loading && !listing}
    <p role="status">Loading your library…</p>
  {:else if listing}
    {#if listing.recordings.length === 0}
      <p class="empty">No recordings yet. Create one above.</p>
    {:else}
      <ul class="rec-list" aria-label="Recordings">
        {#each listing.recordings as r (r.path)}
          <li>
            <div class="rec-info">
              <strong>{r.title}</strong>
              <span class="hint">{r.stepCount} step{r.stepCount === 1 ? "" : "s"} · changed {when(r.updated)}</span>
            </div>
            <button type="button" class="primary" disabled={busy} onclick={() => open(r)} aria-label={`Open ${r.title}`}>Open</button>
            <button type="button" disabled={busy} onclick={() => (renaming = { rec: r, title: r.title })} aria-label={`Rename ${r.title}`}>Rename</button>
            <button type="button" class="danger" disabled={busy} onclick={() => (deleting = r)} aria-label={`Delete ${r.title}`}>Delete</button>
          </li>
        {/each}
      </ul>
    {/if}
    {#if listing.skipped.length > 0}
      <details>
        <summary>{listing.skipped.length} folder{listing.skipped.length === 1 ? "" : "s"} could not be read</summary>
        <ul>
          {#each listing.skipped as s (s.path)}
            <li><code>{s.path}</code>: {s.reason}</li>
          {/each}
        </ul>
        <p class="hint">These are left untouched. A newer version of BabelSteps may have made them.</p>
      </details>
    {/if}
    <button type="button" onclick={refresh} disabled={loading}>Refresh list</button>
  {/if}
</section>

{#if renaming}
  <Modal title="Rename recording" onclose={() => (renaming = null)}>
    <form onsubmit={(e) => { e.preventDefault(); doRename(); }}>
      <label>New name
        <input type="text" bind:value={renaming.title} />
      </label>
      <div class="actions">
        <button type="button" onclick={() => (renaming = null)}>Cancel</button>
        <button type="submit" class="primary" disabled={busy || !renaming.title.trim()}>Rename</button>
      </div>
    </form>
  </Modal>
{/if}

{#if deleting}
  <Modal title="Delete this recording?" onclose={() => (deleting = null)}>
    <p>
      "{deleting.title}" and all of its screenshots will be deleted from this computer.
      <strong>This cannot be undone.</strong>
    </p>
    <div class="actions">
      <button type="button" onclick={() => (deleting = null)}>Keep it</button>
      <button type="button" class="danger" disabled={busy} onclick={doDelete}>Delete permanently</button>
    </div>
  </Modal>
{/if}
