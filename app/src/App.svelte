<script lang="ts">
  import "./app.css";
  import { api, describeError } from "./lib/api";
  import { live, say } from "./lib/announce.svelte";
  import Editor from "./lib/Editor.svelte";
  import Library from "./lib/Library.svelte";
  import Settings from "./lib/Settings.svelte";
  import RecordingPanel from "./lib/RecordingPanel.svelte";
  import { refreshStatus, rs, startPolling, statusLabel } from "./lib/recording.svelte";
  import type { OpenedRecording } from "./lib/types";

  let opened = $state<OpenedRecording | null>(null);
  // Changes on every load, so the editor starts fresh from the saved document.
  let loadKey = $state(0);
  let focusId = $state<string | null>(null);
  let notice = $state("");
  let showSettings = $state(false);

  const st = $derived(rs.status);

  $effect(() => startPolling());

  let wasActive = false;
  let beforeIds: string[] = [];
  let lastPath: string | null = null;

  function show(o: OpenedRecording) {
    const before = new Set(beforeIds);
    focusId = o.recording.blocks.find((b) => b.type === "step" && !before.has(b.id))?.id
      ?? o.recording.blocks.find((b) => !before.has(b.id))?.id ?? null;
    opened = o;
    loadKey++;
    notice = "";
  }

  // Follow the recorder, whoever started or stopped it (buttons, hotkeys, tray).
  $effect(() => {
    const active = st.active;
    const path = st.path;
    if (active && !wasActive) {
      beforeIds = opened ? [...rs.knownIds] : [];
    }
    if (active && path) lastPath = path;
    if (!active && wasActive && !rs.stopping && lastPath) {
      // Stopped from a hotkey or the tray: load the saved result.
      const p = lastPath;
      api.open(p).then(show, (e) => {
        notice = describeError(e, "opening the finished recording") + " Open it from the library.";
        opened = null;
      });
      say("Recording stopped and saved.");
    }
    wasActive = active;
  });

  function stopped(o: OpenedRecording) {
    rs.status = { ...rs.status, active: false, paused: false };
    show(o);
    say("Recording saved. Showing the new steps.");
  }

  function back() {
    opened = null;
    rs.knownIds = [];
  }
</script>

<a class="skip" href="#main">Skip to content</a>
<header class="topbar">
  <span class="brand">BabelSteps</span>
  <span class="rec-state" class:on={st.active}>
    {#if st.active}<span class="dot" class:paused={st.paused} aria-hidden="true"></span>{/if}{statusLabel(st)}
  </span>
  <button type="button" class="settings-btn" onclick={() => (showSettings = true)} disabled={st.active}>Settings</button>
</header>

<main id="main" tabindex="-1">
  {#if notice}<p class="error" role="alert">{notice}</p>{/if}
  {#if st.notice}
    <p class="warn-line" role="status">
      {st.notice}
      <button type="button" onclick={() => api.clearNotice().then(refreshStatus)}>Dismiss</button>
    </p>
  {/if}
  {#if st.active}
    <RecordingPanel onstopped={stopped} />
  {:else if opened}
    {#key loadKey}
      <Editor initial={opened.recording} {focusId} onback={back} />
    {/key}
  {:else}
    <Library onopen={(o) => { beforeIds = []; opened = o; loadKey++; focusId = null; }} />
  {/if}
</main>

{#if showSettings}<Settings onclose={() => (showSettings = false)} />{/if}

<div class="sr-only" aria-live="polite" role="status">{live.text}</div>
