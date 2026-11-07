<script lang="ts">
  import "./app.css";
  import { live } from "./lib/announce.svelte";
  import Editor from "./lib/Editor.svelte";
  import Library from "./lib/Library.svelte";
  import type { OpenedRecording } from "./lib/types";

  let opened = $state<OpenedRecording | null>(null);
</script>

<a class="skip" href="#main">Skip to content</a>
<header class="topbar">
  <span class="brand">BabelSteps</span>
  <!-- Capture is not part of this screen yet, so state plainly that nothing is recording. -->
  <span class="rec-state">Not recording · Keystroke capture is off</span>
</header>

<main id="main" tabindex="-1">
  {#if opened}
    {#key opened.path}
      <Editor initial={opened.recording} onback={() => (opened = null)} />
    {/key}
  {:else}
    <Library onopen={(o) => (opened = o)} />
  {/if}
</main>

<div class="sr-only" aria-live="polite" role="status">{live.text}</div>
