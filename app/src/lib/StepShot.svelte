<script lang="ts">
  import { tick } from "svelte";
  import { api, describeError } from "./api";
  import { say } from "./announce.svelte";
  import type { Rect } from "./types";

  // `redactions` is only passed for step screenshots; without it the image is view-only.
  let {
    path,
    withheld = false,
    redactions = $bindable(),
    label,
  }: { path: string | null; withheld?: boolean; redactions?: Rect[]; label: string } = $props();

  const editable = $derived(redactions !== undefined);

  let url = $state<string | null>(null);
  let loadError = $state("");
  let nat = $state({ w: 0, h: 0 });
  let host: HTMLDivElement | undefined = $state();
  let ov: HTMLDivElement | undefined = $state();
  let visible = $state(false);
  let selected = $state<number | null>(null);
  let drag = $state<{ x0: number; y0: number; x1: number; y1: number } | null>(null);

  // Load images only when scrolled near, so long recordings stay light.
  $effect(() => {
    if (!host) return;
    const io = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) visible = true;
      },
      { rootMargin: "400px" },
    );
    io.observe(host);
    return () => io.disconnect();
  });

  // A withheld screenshot is never requested.
  $effect(() => {
    const p = path;
    if (!visible || withheld || !p) {
      url = null;
      return;
    }
    let cancelled = false;
    let made: string | null = null;
    loadError = "";
    api
      .screenshot(p)
      .then((buf) => {
        if (cancelled) return;
        made = URL.createObjectURL(new Blob([buf], { type: /\.jpe?g$/i.test(p) ? "image/jpeg" : "image/png" }));
        url = made;
      })
      .catch((e) => {
        if (!cancelled) loadError = describeError(e, "loading a screenshot");
      });
    return () => {
      cancelled = true;
      if (made) URL.revokeObjectURL(made);
      url = null;
    };
  });

  const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

  function toSrc(e: PointerEvent) {
    const r = ov!.getBoundingClientRect();
    return {
      x: clamp(((e.clientX - r.left) / r.width) * nat.w, 0, nat.w),
      y: clamp(((e.clientY - r.top) / r.height) * nat.h, 0, nat.h),
    };
  }

  function down(e: PointerEvent) {
    if (e.button !== 0 || !editable || !nat.w) return;
    const p = toSrc(e);
    ov!.setPointerCapture(e.pointerId);
    drag = { x0: p.x, y0: p.y, x1: p.x, y1: p.y };
    selected = null;
  }
  function move(e: PointerEvent) {
    if (!drag) return;
    const p = toSrc(e);
    drag = { ...drag, x1: p.x, y1: p.y };
  }
  function up() {
    if (!drag || !redactions) return;
    const x = Math.round(Math.min(drag.x0, drag.x1));
    const y = Math.round(Math.min(drag.y0, drag.y1));
    const w = Math.round(Math.abs(drag.x1 - drag.x0));
    const h = Math.round(Math.abs(drag.y1 - drag.y0));
    drag = null;
    const minSize = Math.max(4, Math.round(nat.w / 150));
    if (w >= minSize && h >= minSize) {
      redactions.push({ x, y, w, h });
      selected = redactions.length - 1;
      say(`Redaction box ${redactions.length} added.`);
    }
  }

  async function focusBox(i: number) {
    await tick();
    ov?.querySelector<HTMLElement>(`[data-idx="${i}"]`)?.focus();
  }

  function addBox() {
    if (!redactions || !nat.w) return;
    const w = Math.round(nat.w * 0.3);
    const h = Math.round(nat.h * 0.1);
    redactions.push({ x: Math.round((nat.w - w) / 2), y: Math.round((nat.h - h) / 2), w, h });
    selected = redactions.length - 1;
    say(`Redaction box ${redactions.length} added in the middle of the screenshot. Use arrow keys to move it.`);
    focusBox(selected);
  }

  function removeBox(i: number) {
    if (!redactions) return;
    redactions.splice(i, 1);
    say(`Redaction box removed. ${redactions.length} remaining.`);
    if (redactions.length === 0) {
      selected = null;
      host?.querySelector<HTMLElement>("[data-add]")?.focus();
    } else {
      selected = Math.min(i, redactions.length - 1);
      focusBox(selected);
    }
  }

  function boxKey(e: KeyboardEvent, i: number) {
    const r = redactions?.[i];
    if (!r) return;
    const unit = Math.max(1, Math.round(nat.w / 200)) * (e.ctrlKey ? 5 : 1);
    const k = e.key;
    if (k === "Delete" || k === "Backspace") {
      e.preventDefault();
      removeBox(i);
      return;
    }
    const dx = k === "ArrowLeft" ? -unit : k === "ArrowRight" ? unit : 0;
    const dy = k === "ArrowUp" ? -unit : k === "ArrowDown" ? unit : 0;
    if (!dx && !dy) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.shiftKey) {
      r.w = clamp(r.w + dx, 1, nat.w - r.x);
      r.h = clamp(r.h + dy, 1, nat.h - r.y);
    } else {
      r.x = clamp(r.x + dx, 0, nat.w - r.w);
      r.y = clamp(r.y + dy, 0, nat.h - r.h);
    }
  }

  const pct = (v: number, total: number) => (total ? (v / total) * 100 : 0) + "%";
</script>

<div class="shot" bind:this={host}>
  {#if withheld}
    <div class="withheld" role="note">
      <strong>Screenshot withheld (possible password field)</strong>
      <span>
        BabelSteps could not confirm this screen was safe, so the image was not saved. There is
        nothing to load or redact for this step.
      </span>
    </div>
  {:else if !path}
    <div class="noshot">No screenshot for this step.</div>
  {:else if loadError}
    <div class="noshot error" role="alert">{loadError}</div>
  {:else if !url}
    <div class="noshot" aria-busy="true">Loading screenshot…</div>
  {:else}
    <div class="frame">
      <img
        src={url}
        alt={label}
        draggable="false"
        onload={(e) => {
          const im = e.currentTarget as HTMLImageElement;
          nat = { w: im.naturalWidth, h: im.naturalHeight };
        }}
      />
      {#if editable && redactions}
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="overlay"
          bind:this={ov}
          onpointerdown={down}
          onpointermove={move}
          onpointerup={up}
          onpointercancel={() => (drag = null)}
        >
          {#each redactions as r, i (i)}
            <div
              class="rbox"
              class:selected={selected === i}
              data-idx={i}
              role="button"
              tabindex="0"
              aria-label={`Redaction box ${i + 1} of ${redactions.length}. Arrow keys move it, Shift plus arrow keys resize it, Delete removes it.`}
              style:left={pct(r.x, nat.w)}
              style:top={pct(r.y, nat.h)}
              style:width={pct(r.w, nat.w)}
              style:height={pct(r.h, nat.h)}
              onpointerdown={(e) => {
                e.stopPropagation();
                selected = i;
              }}
              onfocus={() => (selected = i)}
              onkeydown={(e) => boxKey(e, i)}
            ></div>
          {/each}
          {#if drag}
            <div
              class="rbox drawing"
              style:left={pct(Math.min(drag.x0, drag.x1), nat.w)}
              style:top={pct(Math.min(drag.y0, drag.y1), nat.h)}
              style:width={pct(Math.abs(drag.x1 - drag.x0), nat.w)}
              style:height={pct(Math.abs(drag.y1 - drag.y0), nat.h)}
            ></div>
          {/if}
        </div>
      {/if}
    </div>
    {#if editable && redactions}
      <div class="shot-tools">
        <button type="button" data-add onclick={addBox}>Add redaction box</button>
        <button
          type="button"
          disabled={selected === null}
          onclick={() => selected !== null && removeBox(selected)}
        >
          Remove selected box
        </button>
        <span class="hint">
          {redactions.length === 0
            ? "No redaction boxes."
            : `${redactions.length} redaction box${redactions.length === 1 ? "" : "es"}.`}
          Drag on the image to draw one. Boxes are painted solid black in exports.
        </span>
      </div>
    {/if}
  {/if}
</div>
