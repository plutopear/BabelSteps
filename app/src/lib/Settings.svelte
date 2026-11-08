<script lang="ts">
  import { api, describeError, toAppError } from "./api";
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
  // B14: one word or pattern per line.
  let wordsText = $state("");
  let patternsText = $state("");
  let customError = $state("");
  let customErrorIn = $state<"word" | "pattern" | null>(null);
  let wordsEl = $state<HTMLTextAreaElement>();
  let patternsEl = $state<HTMLTextAreaElement>();
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

  api.getSettings().then(
    (s) => {
      limit = s.keepScreenshots !== null;
      keep = s.keepScreenshots ?? 100;
      sizeMb = s.sizeWarningMb;
      border = s.showCaptureBorder;
      keys = { ...s.hotkeys };
      toTray = s.closeToTray;
      windowTitles = s.showWindowTitles;
      wordsText = (s.customWords ?? []).join("\n");
      patternsText = (s.customPatterns ?? []).join("\n");
      loaded = true;
    },
    (e) => (error = describeError(e, "reading your settings")),
  );

  /** Non-blank lines, each with its line number, so an error can point at the line you see. */
  function entries(text: string, trim: boolean): { text: string; line: number }[] {
    return text
      .split(/\r?\n/)
      .map((t, i) => ({ text: trim ? t.trim() : t, line: i + 1 }))
      .filter((e) => e.text.trim() !== "");
  }

  /** Turns the app's "custom word 3 is empty" into a message about the line you typed. */
  function customMessage(message: string, words: { line: number }[], patterns: { line: number }[]): string | null {
    const m = /^custom (word|pattern) (\d+) (.+)$/.exec(message);
    if (m) {
      const list = m[1] === "word" ? words : patterns;
      const line = list[Number(m[2]) - 1]?.line ?? Number(m[2]);
      customErrorIn = m[1] as "word" | "pattern";
      const where = m[1] === "word" ? "Words to hide" : "Patterns to hide";
      return where + ", line " + line + " " + m[3] + ".";
    }
    const many = /^too many custom (word|pattern)s/.exec(message);
    if (many) {
      customErrorIn = many[1] as "word" | "pattern";
      return message[0].toUpperCase() + message.slice(1) + ".";
    }
    return null;
  }

  async function save(ev: SubmitEvent) {
    ev.preventDefault();
    error = "";
    customError = "";
    customErrorIn = null;
    if (limit && (!Number.isInteger(keep) || keep < 1)) {
      error = "Type a whole number of 1 or more for how many steps keep their screenshots.";
      return;
    }
    if (!Number.isInteger(sizeMb) || sizeMb < 1) {
      error = "Type a whole number of megabytes (1 or more) for the size warning.";
      return;
    }
    const words = entries(wordsText, true);
    const patterns = entries(patternsText, false);
    saving = true;
    try {
      warnings = await api.saveSettings({
        keepScreenshots: limit ? keep : null, sizeWarningMb: sizeMb, showCaptureBorder: border, hotkeys: { ...keys }, closeToTray: toTray, showWindowTitles: windowTitles,
        customWords: words.map((e) => e.text), customPatterns: patterns.map((e) => e.text),
      });
      say("Settings saved.");
      if (warnings.length === 0) onclose();
      else savedOk = true;
    } catch (e) {
      const ae = toAppError(e);
      const custom = ae.kind === "invalid_argument" ? customMessage(ae.message, words, patterns) : null;
      if (custom) {
        // Announced by the alert next to the lists, and the cursor goes to the list at fault.
        customError = custom;
        (customErrorIn === "word" ? wordsEl : patternsEl)?.focus();
        return;
      }
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
      <legend>Words and patterns to hide</legend>
      <p class="hint" id="custom-help">
        Typed text is checked against these as it is recorded, and a match is replaced with [custom] before
        anything is saved. This works only when you record typed text. Changes apply to new recordings only,
        not to ones you already made. Detection can miss things, such as a different spelling or a word split
        across keystrokes, so review your steps before you share them.
      </p>
      <label for="custom-words">Words to hide (one per line)</label>
      <p class="hint" id="custom-words-hint">
        Customer names, project names, internal servers. Matches whole words and ignores upper and lower case,
        so Acme also hides ACME but not Acmeville.
      </p>
      <textarea
        id="custom-words" rows="4" bind:value={wordsText} bind:this={wordsEl} disabled={!loaded}
        autocomplete="off" spellcheck="false"
        aria-describedby={"custom-help custom-words-hint" + (customErrorIn === "word" ? " custom-error" : "")}
        aria-invalid={customErrorIn === "word"}
      ></textarea>
      <label for="custom-patterns">Patterns to hide (advanced, one per line)</label>
      <p class="hint" id="custom-patterns-hint">
        Regular expressions, for things like ticket numbers: PROJ-[0-9]+ hides PROJ-42. Upper and lower case
        count as different unless you start with (?i). Up to 100 words and 100 patterns, 200 characters each.
      </p>
      <textarea
        id="custom-patterns" rows="4" bind:value={patternsText} bind:this={patternsEl} disabled={!loaded}
        autocomplete="off" spellcheck="false"
        aria-describedby={"custom-help custom-patterns-hint" + (customErrorIn === "pattern" ? " custom-error" : "")}
        aria-invalid={customErrorIn === "pattern"}
      ></textarea>
      {#if customError}<p class="error" id="custom-error" role="alert">{customError}</p>{/if}
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
