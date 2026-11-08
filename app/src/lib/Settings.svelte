<script lang="ts">
  import { api, describeError } from "./api";
  import { say } from "./announce.svelte";
  import Modal from "./Modal.svelte";

  let { onclose }: { onclose: () => void } = $props();

  let loaded = $state(false);
  let limit = $state(false);
  let keep = $state(100);
  let sizeMb = $state(500);
  let error = $state("");
  let saving = $state(false);

  api.getSettings().then(
    (s) => {
      limit = s.keepScreenshots !== null;
      keep = s.keepScreenshots ?? 100;
      sizeMb = s.sizeWarningMb;
      loaded = true;
    },
    (e) => (error = describeError(e, "reading your settings")),
  );

  async function save(ev: SubmitEvent) {
    ev.preventDefault();
    error = "";
    if (limit && (!Number.isInteger(keep) || keep < 1)) {
      error = "Type a whole number of 1 or more for how many steps keep their screenshots.";
      return;
    }
    if (!Number.isInteger(sizeMb) || sizeMb < 1) {
      error = "Type a whole number of megabytes (1 or more) for the size warning.";
      return;
    }
    saving = true;
    try {
      await api.saveSettings({ keepScreenshots: limit ? keep : null, sizeWarningMb: sizeMb });
      say("Settings saved.");
      onclose();
    } catch (e) {
      error = describeError(e, "saving your settings");
    } finally {
      saving = false;
    }
  }
</script>

<Modal title="Settings" onclose={onclose}>
  <form onsubmit={save}>
    <fieldset>
      <legend>Screenshots</legend>
      <label class="check">
        <input type="checkbox" bind:checked={limit} disabled={!loaded} />
        Keep screenshots only for the newest steps
      </label>
      {#if limit}
        <label>Number of newest steps that keep their screenshot
          <input type="number" min="1" step="1" bind:value={keep} />
        </label>
        <p class="hint">
          Applied each time a recording stops. Older steps keep their text but lose their picture, and the
          picture files are removed from the recording folder. This cannot be undone from BabelSteps.
        </p>
      {:else}
        <p class="hint">Every step keeps its screenshot. Recordings can get large.</p>
        <label>Warn me after a recording when its screenshots take more than (MB)
          <input type="number" min="1" step="1" bind:value={sizeMb} />
        </label>
      {/if}
    </fieldset>
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    <div class="actions">
      <button type="button" onclick={onclose}>Cancel</button>
      <button type="submit" class="primary" disabled={!loaded || saving}>Save</button>
    </div>
  </form>
</Modal>
