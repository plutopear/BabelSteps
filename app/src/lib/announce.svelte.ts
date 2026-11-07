// Polite screen-reader announcements, rendered by App.svelte in a live region.
export const live = $state({ text: "" });
let flip = false;
export function say(text: string) {
  // Alternate a trailing space so repeating the same message is re-announced.
  flip = !flip;
  live.text = text + (flip ? "" : " ");
}
