<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { cursorPosition, getCurrentWindow } from "@tauri-apps/api/window";
import { emitTo, listen, type UnlistenFn } from "@tauri-apps/api/event";
import { api } from "./api";
import { fetchPixels } from "./pixels";
import type { OverlayStart, WindowTarget } from "./types";

/*
 * One of these covers each display during an area or window capture, showing
 * the frozen screenshot of that display. The window ("overlay-<display id>") is
 * made once, kept hidden between captures, and told about each new capture
 * with an "overlay-start" event, so it can appear without loading a page
 * first. In window mode it highlights the window under the pointer instead
 * of drawing a selection, and a click captures that window. Scroll mode
 * (Scrolling Capture) does both: it highlights the window under the
 * pointer, and a click picks that window and scrolls it at the spot clicked,
 * while a drag selects part of the screen to scroll from the middle.
 * Lockshot then captures just that part of the screen page by page.
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

const picksWindow = computed(() => mode.value === "window");
const highlightsWindows = computed(() => mode.value === "window" || mode.value === "scroll");

/** Scrolling Capture: the press has become a drag that selects an area. */
const dragging = computed(() => {
  const s = selection.value;
  return !!s && s.w >= 4 && s.h >= 4;
});

/** The frontmost window under the pointer, in CSS pixels. */
const hovered = computed(() => {
  const p = pointer.value;
  if (!highlightsWindows.value || !p || dragging.value) return null;
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
  /* Scrolling Capture: near the top, clear of the part being pointed at. */
  const y = mode.value === "scroll" ? Math.min(top + 44, (top + bottom) / 2) : (top + bottom) / 2;
  return { left: `${(left + right) / 2}px`, top: `${y}px` };
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

/** The window is the shape of the display it shows, give or take rounding. */
function fits(next: OverlayStart) {
  const W = window.innerWidth;
  const H = window.innerHeight;
  if (!W || !H) return true;
  return Math.abs(W / H - next.width / next.height) < 0.01;
}

/**
 * The frozen image is stretched to fill the window, so a window that came
 * out the wrong shape would show the display squished (issue #31). Ask for
 * it to be put back over its display, and wait for the resize.
 */
async function ensureFit(next: OverlayStart) {
  if (fits(next)) return;
  console.warn(
    `[overlay ${win.label}] page ${window.innerWidth}x${window.innerHeight} does not match the ${next.width}x${next.height} display; placing it again`,
  );
  await api.overlayRefit().catch(() => {});
  await new Promise<void>((resolve) => {
    const done = () => {
      window.removeEventListener("resize", done);
      clearTimeout(timer);
      resolve();
    };
    const timer = setTimeout(done, 300);
    window.addEventListener("resize", done);
  });
}

/** Wait for the page to render a few frames, or give up after a moment. */
function nextFrames(count: number) {
  return new Promise<void>((resolve) => {
    const timer = setTimeout(resolve, 150);
    const step = (left: number) =>
      requestAnimationFrame(() => {
        if (left <= 1) {
          clearTimeout(timer);
          resolve();
        } else step(left - 1);
      });
    step(count);
  });
}

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
  await ensureFit(next);
  if (session.value !== next.session) return;
  loaded.value = true;
  /*
   * A hidden webview is not repainted, so it still holds the last capture's
   * screen. Show it see-through, let the new frame render, then make it
   * opaque, so the old one never flashes up (issue #40).
   */
  await api.overlayShow();
  await nextFrames(2);
  if (session.value !== next.session) return;
  await api.overlayReveal();
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
    if (inside && highlightsWindows.value && !pointer.value) {
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
  if (!start.value || finishing.value) return;
  /* Read before clearing start, which empties the selection. */
  const s = dragging.value ? selection.value : null;
  const picked = s ? null : hovered.value?.target;
  const p = pointer.value;
  start.value = null;
  finishing.value = true;
  const W = window.innerWidth;
  const H = window.innerHeight;
  /* A click without a drag takes the whole screen, or in scroll mode the
  window clicked, scrolled where it was clicked. */
  let rect = s ? { x: s.x / W, y: s.y / H, width: s.w / W, height: s.h / H } : null;
  let spot: { x: number; y: number } | null = null;
  if (mode.value === "scroll" && picked && p) {
    rect = { x: picked.x, y: picked.y, width: picked.width, height: picked.height };
    spot = { x: p.x / W, y: p.y / H };
  }
  try {
    if (mode.value === "scroll") await api.finishScroll(session.value, rect, spot);
    else await api.finishArea(session.value, rect);
  } catch (err) {
    if (mode.value === "scroll") {
      void emitTo("main", "capture-error", typeof err === "string" ? err : "Lockshot couldn't start the scrolling capture.");
    }
    await api.cancelArea();
  }
}

async function pickWindow() {
  const target = hovered.value?.target;
  if (!target || finishing.value) return;
  finishing.value = true;
  try {
    await api.finishWindow(session.value, target.id);
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
        <span v-if="windows.length">Click a window to capture it</span>
        <span v-else>No windows to capture on this display</span>
        <span><kbd>Esc</kbd> to cancel</span>
      </div>
    </template>
    <template v-else-if="loaded">
      <template v-if="hovered">
        <div
          class="overlay-window"
          :style="{ left: `${hovered.x}px`, top: `${hovered.y}px`, width: `${hovered.w}px`, height: `${hovered.h}px` }"
        />
        <div v-if="pointer" class="overlay-spot" :style="{ left: `${pointer.x}px`, top: `${pointer.y}px` }" />
        <div class="overlay-window-label" :style="hoveredLabelStyle">
          <strong>{{ hovered.target.label || "Window" }}</strong>
          <span>Click to scroll here · {{ hoveredSize }}</span>
        </div>
      </template>
      <div
        v-if="selection"
        class="overlay-selection"
        :style="{ left: `${selection.x}px`, top: `${selection.y}px`, width: `${selection.w}px`, height: `${selection.h}px` }"
      />
      <div v-if="selection && sizeLabel" class="overlay-size" :style="labelStyle">{{ sizeLabel }}</div>
      <div v-if="!start && mode === 'scroll'" class="overlay-hint">
        <span>Click the part of a window that scrolls</span>
        <span>Drag to scroll an area instead</span>
        <span><kbd>Esc</kbd> to cancel</span>
      </div>
      <div v-else-if="!start" class="overlay-hint">
        <span>Drag to capture</span>
        <span>Click for full screen</span>
        <span><kbd>Esc</kbd> to cancel</span>
      </div>
    </template>
  </div>
</template>
