<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { cursorPosition, getCurrentWindow } from "@tauri-apps/api/window";
import { emitTo, listen, type UnlistenFn } from "@tauri-apps/api/event";
import { api } from "./api";
import { fetchPixels } from "./pixels";

/*
 * One of these covers each display during an area capture, showing the frozen
 * screenshot of that display. The window ("overlay-<index>") is made once,
 * kept hidden between captures, and told about each new capture with an
 * "overlay-start" event, so it can appear without loading a page first.
 */
interface OverlayStart {
  session: string;
  index: number;
  width: number;
  height: number;
}

const win = getCurrentWindow();
const canvas = ref<HTMLCanvasElement | null>(null);
const session = ref("");
const loaded = ref(false);
const pointer = ref<{ x: number; y: number } | null>(null);
const start = ref<{ x: number; y: number } | null>(null);
const naturalWidth = ref(0);
const finishing = ref(false);

const selection = computed(() => {
  if (!start.value || !pointer.value) return null;
  const x = Math.min(start.value.x, pointer.value.x);
  const y = Math.min(start.value.y, pointer.value.y);
  return { x, y, w: Math.abs(pointer.value.x - start.value.x), h: Math.abs(pointer.value.y - start.value.y) };
});

/** Image pixels per CSS pixel, for the size readout. */
const density = computed(() => (naturalWidth.value ? naturalWidth.value / window.innerWidth : window.devicePixelRatio));

const sizeLabel = computed(() => {
  const s = selection.value;
  if (!s) return "";
  return `${Math.round(s.w * density.value)} × ${Math.round(s.h * density.value)}`;
});

const labelStyle = computed(() => {
  const s = selection.value;
  if (!s) return {};
  const below = s.y + s.h + 28 < window.innerHeight;
  return {
    left: `${Math.min(s.x + s.w, window.innerWidth - 8)}px`,
    top: `${below ? s.y + s.h + 6 : Math.max(4, s.y - 24)}px`,
    transform: "translateX(-100%)",
  };
});

/** Show a new capture: draw its frozen display, then appear. */
async function begin(next: OverlayStart) {
  const started = performance.now();
  session.value = next.session;
  loaded.value = false;
  start.value = null;
  pointer.value = null;
  finishing.value = false;
  try {
    const pixels = await fetchPixels(`session/${next.session}/${next.index}`, next.width, next.height);
    if (session.value !== next.session || !canvas.value) return;
    canvas.value.width = next.width;
    canvas.value.height = next.height;
    canvas.value.getContext("2d")!.putImageData(pixels, 0, 0);
  } catch {
    if (session.value !== next.session) return;
    finishing.value = true;
    void emitTo("main", "capture-error", "Lockshot couldn't load the screen image for this capture. Please try again.");
    void api.cancelArea();
    return;
  }
  naturalWidth.value = next.width;
  loaded.value = true;
  await win.show();
  if (import.meta.env.DEV) {
    /* Shows up in the tauri dev terminal, for checking speed and geometry. */
    const [pos, size] = await Promise.all([win.outerPosition(), win.outerSize()]);
    console.info(
      `[overlay ${win.label}] shown ${Math.round(performance.now() - started)} ms after the event;`,
      `window ${size.width}x${size.height} at ${pos.x},${pos.y}, page ${window.innerWidth}x${window.innerHeight}`,
    );
  }
  /* Only the display under the pointer takes keyboard focus, for Esc. */
  try {
    const [cursor, pos, size] = await Promise.all([cursorPosition(), win.outerPosition(), win.outerSize()]);
    const inside = cursor.x >= pos.x && cursor.y >= pos.y && cursor.x < pos.x + size.width && cursor.y < pos.y + size.height;
    if (inside) await win.setFocus();
  } catch {
    await win.setFocus();
  }
}

function cancel() {
  if (finishing.value) return;
  finishing.value = true;
  void api.cancelArea();
}

function onPointerDown(event: PointerEvent) {
  if (event.button === 2) {
    cancel();
    return;
  }
  if (event.button !== 0) return;
  (event.target as HTMLElement).setPointerCapture(event.pointerId);
  start.value = { x: event.clientX, y: event.clientY };
  pointer.value = { ...start.value };
}

function onPointerMove(event: PointerEvent) {
  pointer.value = { x: event.clientX, y: event.clientY };
}

async function onPointerUp() {
  const s = selection.value;
  if (!start.value || finishing.value) return;
  start.value = null;
  finishing.value = true;
  const W = window.innerWidth;
  const H = window.innerHeight;
  /* A click without a drag takes the whole screen. */
  const rect = s && s.w >= 4 && s.h >= 4 ? { x: s.x / W, y: s.y / H, width: s.w / W, height: s.h / H } : null;
  try {
    await api.finishArea(session.value, rect);
  } catch {
    await api.cancelArea();
  }
}

function onKey(event: KeyboardEvent) {
  if (event.key === "Escape") cancel();
}

let unlisten: UnlistenFn | undefined;

onMounted(async () => {
  window.addEventListener("keydown", onKey);
  window.addEventListener("contextmenu", (e) => e.preventDefault());
  unlisten = await listen<OverlayStart>("overlay-start", (event) => void begin(event.payload));
  /* A capture that started while this page was still loading. */
  const pending = await api.overlayPending().catch(() => null);
  if (pending && pending.session !== session.value) void begin(pending);
});
onBeforeUnmount(() => {
  window.removeEventListener("keydown", onKey);
  unlisten?.();
});
</script>

<template>
  <div
    class="overlay"
    @pointerdown="onPointerDown"
    @pointermove="onPointerMove"
    @pointerup="onPointerUp"
    @pointerleave="!start && (pointer = null)"
  >
    <canvas ref="canvas" class="overlay-image" />
    <template v-if="loaded">
      <div
        v-if="selection"
        class="overlay-selection"
        :style="{ left: `${selection.x}px`, top: `${selection.y}px`, width: `${selection.w}px`, height: `${selection.h}px` }"
      />
      <div v-if="selection && sizeLabel" class="overlay-size" :style="labelStyle">{{ sizeLabel }}</div>
      <div v-if="!start" class="overlay-hint">
        <span>Drag to capture</span>
        <span>Click for full screen</span>
        <span><kbd>Esc</kbd> to cancel</span>
      </div>
    </template>
  </div>
</template>
