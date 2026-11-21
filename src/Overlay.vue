<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { cursorPosition, getCurrentWindow } from "@tauri-apps/api/window";
import { emitTo, listen, type UnlistenFn } from "@tauri-apps/api/event";
import { api } from "./api";
import { fetchPixels } from "./pixels";
import type { OverlayStart, WindowTarget } from "./types";

/*
 * One of these covers each display during an area or window capture, showing
 * the frozen screenshot of that display. The window ("overlay-<index>") is
 * made once, kept hidden between captures, and told about each new capture
 * with an "overlay-start" event, so it can appear without loading a page
 * first. In window mode it highlights the window under the pointer instead
 * of drawing a selection, and a click captures that window. Scroll mode
 * (Scrolling Capture) picks a window the same way, and the spot clicked is
 * where Lockshot scrolls, so it reaches the right pane.
 */

const win = getCurrentWindow();
const canvas = ref<HTMLCanvasElement | null>(null);
const session = ref("");
const loaded = ref(false);
const pointer = ref<{ x: number; y: number } | null>(null);
const start = ref<{ x: number; y: number } | null>(null);
const naturalWidth = ref(0);
const finishing = ref(false);
const mode = ref<OverlayStart["mode"]>("area");
const windows = ref<WindowTarget[]>([]);

const picksWindow = computed(() => mode.value === "window" || mode.value === "scroll");

/** Capture Window: the frontmost window under the pointer, in CSS pixels. */
const hovered = computed(() => {
  const p = pointer.value;
  if (!picksWindow.value || !p) return null;
  const W = window.innerWidth;
  const H = window.innerHeight;
  const target = windows.value.find(
    (w) => p.x >= w.x * W && p.y >= w.y * H && p.x < (w.x + w.width) * W && p.y < (w.y + w.height) * H,
  );
  if (!target) return null;
  return { target, x: target.x * W, y: target.y * H, w: target.width * W, h: target.height * H };
});

/** Centre the window's name on the part of it this display shows. */
const hoveredLabelStyle = computed(() => {
  const h = hovered.value;
  if (!h) return {};
  const left = Math.max(0, h.x);
  const top = Math.max(0, h.y);
  const right = Math.min(window.innerWidth, h.x + h.w);
  const bottom = Math.min(window.innerHeight, h.y + h.h);
  return { left: `${(left + right) / 2}px`, top: `${(top + bottom) / 2}px` };
});

const hoveredSize = computed(() => {
  const h = hovered.value;
  return h ? `${Math.round(h.w * density.value)} × ${Math.round(h.h * density.value)}` : "";
});

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
  mode.value = next.mode;
  windows.value = next.windows;
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
    /* Highlight the window under the pointer before it first moves. */
    if (inside && picksWindow.value && !pointer.value) {
      const scale = size.width / window.innerWidth;
      pointer.value = { x: (cursor.x - pos.x) / scale, y: (cursor.y - pos.y) / scale };
    }
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
  if (picksWindow.value) {
    pointer.value = { x: event.clientX, y: event.clientY };
    void pickWindow();
    return;
  }
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

async function pickWindow() {
  const target = hovered.value?.target;
  if (!target || finishing.value) return;
  finishing.value = true;
  const p = pointer.value!;
  try {
    if (mode.value === "scroll") {
      await api.finishScroll(session.value, target.id, p.x / window.innerWidth, p.y / window.innerHeight);
    } else {
      await api.finishWindow(session.value, target.id);
    }
  } catch (err) {
    void emitTo("main", "capture-error", typeof err === "string" ? err : "Lockshot couldn't capture that window.");
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
    <template v-if="loaded && picksWindow">
      <template v-if="hovered">
        <div
          class="overlay-window"
          :style="{ left: `${hovered.x}px`, top: `${hovered.y}px`, width: `${hovered.w}px`, height: `${hovered.h}px` }"
        />
        <div class="overlay-window-label" :style="hoveredLabelStyle">
          <strong>{{ hovered.target.label || "Window" }}</strong>
          <span>{{ hoveredSize }}</span>
        </div>
      </template>
      <div class="overlay-hint">
        <span v-if="windows.length && mode === 'scroll'">Click inside the part of a window to scroll</span>
        <span v-else-if="windows.length">Click a window to capture it</span>
        <span v-else>No windows to capture on this display</span>
        <span><kbd>Esc</kbd> to cancel</span>
      </div>
    </template>
    <template v-else-if="loaded">
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
