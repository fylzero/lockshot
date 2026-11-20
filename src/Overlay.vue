<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { convertFileSrc } from "@tauri-apps/api/core";
import { cursorPosition, getCurrentWindow } from "@tauri-apps/api/window";
import { api } from "./api";

/*
 * One of these covers each monitor during an area capture, showing the frozen
 * screenshot of that monitor. Its window label is "overlay-<session>-<index>".
 */
const win = getCurrentWindow();
const match = /^overlay-(.+)-(\d+)$/.exec(win.label);
const src = match ? convertFileSrc(`session/${match[1]}/${match[2]}`, "capture") : "";

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

async function onLoad(event: Event) {
  naturalWidth.value = (event.target as HTMLImageElement).naturalWidth;
  loaded.value = true;
  await win.show();
  /* Only the monitor under the pointer takes keyboard focus, for Esc. */
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
    await api.finishArea(rect);
  } catch {
    await api.cancelArea();
  }
}

function onKey(event: KeyboardEvent) {
  if (event.key === "Escape") cancel();
}

onMounted(() => {
  window.addEventListener("keydown", onKey);
  window.addEventListener("contextmenu", (e) => e.preventDefault());
});
onBeforeUnmount(() => window.removeEventListener("keydown", onKey));
</script>

<template>
  <div
    class="overlay"
    @pointerdown="onPointerDown"
    @pointermove="onPointerMove"
    @pointerup="onPointerUp"
    @pointerleave="!start && (pointer = null)"
  >
    <img v-if="src" :src="src" alt="" draggable="false" @load="onLoad" />
    <template v-if="loaded">
      <div v-if="!selection" class="overlay-shade" />
      <div
        v-if="selection"
        class="overlay-selection"
        :style="{ left: `${selection.x}px`, top: `${selection.y}px`, width: `${selection.w}px`, height: `${selection.h}px` }"
      />
      <template v-if="pointer && !selection">
        <div class="overlay-crosshair-x" :style="{ top: `${pointer.y}px` }" />
        <div class="overlay-crosshair-y" :style="{ left: `${pointer.x}px` }" />
      </template>
      <div v-if="selection && sizeLabel" class="overlay-size" :style="labelStyle">{{ sizeLabel }}</div>
      <div v-if="!start" class="overlay-hint">
        <span>Drag to capture</span>
        <span>Click for full screen</span>
        <span><kbd>Esc</kbd> to cancel</span>
      </div>
    </template>
  </div>
</template>
