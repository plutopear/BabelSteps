<script lang="ts">
  import { api, describeError, pickSettingsOpenPath, pickSettingsSavePath, toAppError } from "./api";
  import type { Settings as SettingsData } from "./types";
  import { say } from "./announce.svelte";
  import Modal from "./Modal.svelte";

  let { onclose }: { onclose: () => void } = $props();

  let loaded = $state(false);
  let limit = $state(false);
  let keep = $state(100);
  let sizeMb = $state(500);
  let border = $state(true);
  let toTray = $state(false);
  let windowTitles = $state(false);
  const DEFAULT_KEYS = { startStop: "Ctrl+Alt+Shift+R", pause: "Ctrl+Alt+Shift+P", note: "Ctrl+Alt+Shift+N", stop: "Ctrl+Alt+Shift+X" };
  let keys = $state({ ...DEFAULT_KEYS });
  let warnings = $state<string[]>([]);
  let savedOk = $state(false);
  const ACTIONS: [keyof typeof DEFAULT_KEYS, string][] = [
    ["startStop", "Start or stop recording"],
    ["pause", "Pause or resume"],
    ["note", "Add a note"],
    ["stop", "Stop now (always stops and saves)"],
  ];

  /** Press the shortcut in the box to fill it in. Typing the text by hand also works. */
  function capture(e: KeyboardEvent, k: keyof typeof DEFAULT_KEYS) {
    if (!(e.ctrlKey || e.altKey || e.metaKey)) return;
    let key = "";
    if (/^Key[A-Z]$/.test(e.code)) key = e.code.slice(3);
    else if (/^Digit[0-9]$/.test(e.code)) key = e.code.slice(5);
    else if (/^F([1-9]|1[0-2])$/.test(e.code)) key = e.code;
    if (!key) return;
    e.preventDefault();
    keys[k] = [e.ctrlKey && "Ctrl", e.altKey && "Alt", e.shiftKey && "Shift", e.metaKey && "Win", key].filter(Boolean).join("+");
  }
  let error = $state("");
  let saving = $state(false);

  function fill(s: SettingsData) {
    limit = s.keepScreenshots !== null;
    keep = s.keepScreenshots ?? 100;
    sizeMb = s.sizeWarningMb;
    border = s.showCaptureBorder;
    keys = { ...s.hotkeys };
    toTray = s.closeToTray;
    windowTitles = s.showWindowTitles;
  }

  api.getSettings().then(
    (s) => {
      fill(s);
      loaded = true;
    },
    (e) => (error = describeError(e, "reading your settings")),
  );

  // C7: export and import. One inline message, read out by screen readers
  // (role status or alert inside the dialog) and sent to the live region too.
  let transferring = $state(false);
  let transferMsg = $state<{ ok: boolean; text: string } | null>(null);

  function report(ok: boolean, text: string) {
    transferMsg = { ok, text };
    say(text);
  }

  async function exportSettings() {
    error = "";
    transferMsg = null;
    transferring = true;
    try {
      const path = await pickSettingsSavePath();
      if (!path) return;
      await api.exportSettings(path);
      report(true, "Settings exported. The file leaves out anything specific to this PC.");
    } catch (e) {
      report(false, describeError(e, "exporting your settings"));
    } finally {
      transferring = false;
    }
  }

  async function importSettings() {
    error = "";
    transferMsg = null;
    transferring = true;
    try {
      const path = await pickSettingsOpenPath();
      if (!path) return;
      const warn = await api.importSettings(path);
      fill(await api.getSettings());
      report(
        true,
        "Settings imported and saved." + (warn.length ? " Warnings: " + warn.join(" ") : ""),
      );
    } catch (e) {
      const ae = toAppError(e);
      report(
        false,
        ae.kind === "invalid_argument" && ae.message
          ? `Nothing was imported. ${ae.message}`
          : ae.kind === "busy"
            ? "Stop the recording before importing shortcuts."
            : "Nothing was imported. " + describeError(e, "importing your settings"),
      );
    } finally {
      transferring = false;
    }
  }

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
      warnings = await api.saveSettings({
        keepScreenshots: limit ? keep : null, sizeWarningMb: sizeMb, showCaptureBorder: border, hotkeys: { ...keys }, closeToTray: toTray, showWindowTitles: windowTitles,
      });
      say("Settings saved.");
      if (warnings.length === 0) onclose();
      else savedOk = true;
    } catch (e) {
      const ae = toAppError(e);
      error = ae.kind === "invalid_argument" && ae.message
        ? `${ae.message}.`
        : ae.kind === "busy"
          ? "Stop the recording before changing shortcuts."
          : describeError(e, "saving your settings");
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
    <fieldset>
      <legend>Recording indicator</legend>
      <label class="check">
        <input type="checkbox" bind:checked={border} disabled={!loaded} />
        Show a red frame around each screen while recording
      </label>
      <p class="hint">
        The tray icon always shows when recording is on. Turning the frame off hides only the frame.
        BabelSteps asks Windows to keep the frame out of screenshots.
      </p>
    </fieldset>
    <fieldset>
      <legend>Step names</legend>
      <label class="check">
        <input type="checkbox" bind:checked={windowTitles} disabled={!loaded} />
        Show full window titles in steps
      </label>
      <p class="hint">
        Off: steps name the app, like <q>Left-click "Save" button in notepad</q>. On: steps name the window,
        like <q>Left-click "Save" button in "Budget 2026 - Notepad"</q>, and the title is saved in the recording. Window titles
        often show file names, web page titles and customer names, so check your steps before you share them.
        Applies to the next recording you start.
      </p>
    </fieldset>
    <fieldset>
      <legend>Shortcuts</legend>
      {#each ACTIONS as [k, label]}
        <label>{label}
          <input type="text" bind:value={keys[k]} onkeydown={(e) => capture(e, k)} autocomplete="off" spellcheck="false" />
        </label>
      {/each}
      <p class="hint">
        Click a box and press the new shortcut, or type it like Ctrl+Alt+Shift+R. Use Ctrl, Alt or Windows with
        a letter, number or F1 to F12. Three modifiers avoid clashes with browsers and other apps.
        Shortcuts change between recordings, not during one.
      </p>
      <button type="button" onclick={() => (keys = { ...DEFAULT_KEYS })}>Reset shortcuts to the defaults</button>
    </fieldset>
    <fieldset>
      <legend>Closing the window</legend>
      <label class="check">
        <input type="radio" name="close" value={false} bind:group={toTray} disabled={!loaded} />
        Quit BabelSteps
      </label>
      <label class="check">
        <input type="radio" name="close" value={true} bind:group={toTray} disabled={!loaded} />
        Keep running in the tray, so the shortcuts still work
      </label>
      <p class="hint">
        Either way, closing the window during a recording stops and saves it first. In the tray, BabelSteps
        uses less memory with its window closed. Double-click the tray icon or choose Show BabelSteps to open it
        again, and Quit BabelSteps in the tray menu to quit.
      </p>
    </fieldset>
    <fieldset>
      <legend>Export and import</legend>
      <p class="hint">
        Export saves your saved settings, including the shortcuts, as a file you can keep or copy to another
        PC. It leaves out anything specific to this PC, such as folders. Import replaces your settings with the
        file's and saves them right away, after checking them like a normal save. A file that fails the check
        changes nothing. Changes you have typed in this window and not saved are replaced.
      </p>
      <div class="actions">
        <button type="button" onclick={exportSettings} disabled={!loaded || transferring}>Export settings…</button>
        <button type="button" onclick={importSettings} disabled={!loaded || transferring}>Import settings…</button>
      </div>
      {#if transferMsg}
        {#if transferMsg.ok}
          <p role="status">{transferMsg.text}</p>
        {:else}
          <p class="error" role="alert">{transferMsg.text}</p>
        {/if}
      {/if}
    </fieldset>
    {#if savedOk}
      <p class="warn-line" role="status">Settings saved, with warnings:</p>
      <ul class="warn-line" aria-label="Shortcut warnings">
        {#each warnings as w}<li>{w}</li>{/each}
      </ul>
    {/if}
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    <div class="actions">
      {#if savedOk}
        <button type="button" class="primary" onclick={onclose}>Close</button>
      {:else}
        <button type="button" onclick={onclose}>Cancel</button>
        <button type="submit" class="primary" disabled={!loaded || saving}>Save</button>
      {/if}
    </div>
  </form>
</Modal>
