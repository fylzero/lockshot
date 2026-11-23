<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import Icon from "./components/Icon.vue";
import { api } from "./api";
import { fileManagerName } from "./platform";
import type { SaveToast } from "./types";

/*
 * The small floating notice ("toast") shown after a capture with Save
 * Without Editing on. Rust makes this window once, keeps it hidden between
 * captures, and tells it about each save with a "toast-show" event. It hides
 * itself after a few seconds, unless the pointer is over it. During a
 * Scrolling Capture it shows the page count and a Stop button, and stays up
 * until the run ends.
 */

const SHOW_MS = 5000;
const ERROR_MS = 8000;

const toast = ref<SaveToast | null>(null);
const thumb = ref<HTMLCanvasElement | null>(null);
const hovering = ref(false);
let timer: ReturnType<typeof setTimeout> | undefined;
let unlisten: UnlistenFn | undefined;

async function present(next: SaveToast) {
  if (!next.skippable) skipping.value = false;
  toast.value = next;
  restartTimer();
  await nextTick();
  const canvas = thumb.value;
  const t = next.thumbnail;
  if (!canvas || !t) return;
  canvas.width = t.width;
  canvas.height = t.height;
  canvas.getContext("2d")?.putImageData(new ImageData(Uint8ClampedArray.from(t.data), t.width, t.height), 0, 0);
}

function restartTimer() {
  clearTimeout(timer);
  if (hovering.value || !toast.value || toast.value.running) return;
  timer = setTimeout(dismiss, toast.value.error ? ERROR_MS : SHOW_MS);
}

function dismiss() {
  clearTimeout(timer);
  if (toast.value) void api.toastDismiss(toast.value.id);
}

function edit() {
  clearTimeout(timer);
  if (toast.value?.capture) void api.toastEdit(toast.value.id);
}

function stop() {
  void api.scrollStop();
}

const skipping = ref(false);

function skipText() {
  skipping.value = true;
  void api.scrollSkipText();
}

function pageCount(n: number | null) {
  return n === 1 ? "1 page" : `${n ?? 0} pages`;
}

function reveal() {
  if (toast.value?.path) void api.revealPath(toast.value.path).catch(() => undefined);
  dismiss();
}

function onEnter() {
  hovering.value = true;
  clearTimeout(timer);
}

function onLeave() {
  hovering.value = false;
  restartTimer();
}

onMounted(async () => {
  unlisten = await listen<SaveToast>("toast-show", (event) => void present(event.payload));
  /* The first save can arrive before this page has loaded. */
  const pending = await api.toastPending().catch(() => null);
  if (pending && pending.id !== toast.value?.id) void present(pending);
});

onBeforeUnmount(() => {
  clearTimeout(timer);
  unlisten?.();
});
</script>

<template>
  <div v-if="toast" class="save-toast" :class="{ error: toast.error }" @mouseenter="onEnter" @mouseleave="onLeave">
    <template v-if="toast.error">
      <div class="body">
        <strong>Could not save the capture</strong>
        <span class="detail">{{ toast.error }}</span>
      </div>
    </template>
    <template v-else-if="toast.saving">
      <div class="thumb"><canvas ref="thumb" /></div>
      <div class="body">
        <strong>{{ toast.note ?? "Stopped" }} · {{ pageCount(toast.pages) }}</strong>
        <span class="detail"><span class="saving-dot" />{{ skipping ? "Skipping the rest of the text…" : (toast.stage ?? "Saving…") }}</span>
      </div>
      <div v-if="toast.skippable" class="actions">
        <button type="button" title="Stop reading the text and keep what was read so far" :disabled="skipping" @click="skipText">Skip</button>
      </div>
    </template>
    <template v-else-if="toast.running">
      <div class="thumb"><canvas ref="thumb" /></div>
      <div class="body">
        <strong>Scrolling capture · {{ pageCount(toast.pages) }}</strong>
        <span v-if="toast.pointerLocked" class="detail running-detail">The mouse is locked while Lockshot scrolls. Press <kbd>Esc</kbd> to stop.</span>
        <span v-else class="detail running-detail">Auto-scrolls with mouse events, so moving the mouse stops it. <kbd>Esc</kbd> stops it too.</span>
      </div>
      <!-- The pointer can't reach a button while it is locked. -->
      <div v-if="!toast.pointerLocked" class="actions">
        <button class="primary" type="button" @click="stop">Stop</button>
      </div>
    </template>
    <template v-else-if="toast.pages !== null">
      <button class="thumb" type="button" :title="`Show in ${fileManagerName}`" @click="reveal">
        <canvas ref="thumb" />
      </button>
      <div class="body">
        <strong>{{ toast.title ?? `Saved ${pageCount(toast.pages)}` }}</strong>
        <span class="detail" :title="toast.path ?? ''">{{ toast.note ? `${toast.note}. ` : "" }}{{ toast.fileName }}</span>
      </div>
      <div class="actions">
        <button class="primary" type="button" :title="`Show in ${fileManagerName}`" @click="reveal"><Icon name="folder" /></button>
      </div>
    </template>
    <template v-else>
      <button class="thumb" type="button" title="Edit this capture" @click="edit">
        <canvas ref="thumb" />
      </button>
      <div class="body">
        <strong>Saved{{ toast.copied ? " and copied" : "" }}</strong>
        <span class="detail" :title="toast.path ?? ''">{{ toast.fileName }}</span>
      </div>
      <div class="actions">
        <button class="primary" type="button" @click="edit">Edit</button>
        <button type="button" :title="`Show in ${fileManagerName}`" @click="reveal"><Icon name="folder" /></button>
      </div>
    </template>
    <button v-if="!toast.running" class="ghost close" type="button" aria-label="Dismiss" @click="dismiss"><Icon name="close" /></button>
  </div>
</template>

<style scoped>
.save-toast {
  position: fixed;
  inset: 0;
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 0 28px 0 14px;
  background: var(--bg-raised);
  border: 1px solid var(--border);
  user-select: none;
  -webkit-user-select: none;
  cursor: default;
}

.save-toast.error {
  border-left: 3px solid var(--bad);
}

.thumb {
  flex: none;
  width: 80px;
  height: 56px;
  padding: 0;
  display: grid;
  place-items: center;
  overflow: hidden;
  background: var(--editor-bg);
  border-radius: 4px;
}

.thumb canvas {
  max-width: 100%;
  max-height: 100%;
}

.body {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.body strong {
  font-size: 13px;
}

.detail {
  font-size: 12px;
  line-height: 1.3;
  color: var(--text-soft);
  overflow: hidden;
  overflow-wrap: anywhere;
  display: -webkit-box;
  -webkit-line-clamp: 2;
  -webkit-box-orient: vertical;
}

.saving-dot {
  display: inline-block;
  width: 7px;
  height: 7px;
  margin-right: 6px;
  border-radius: 50%;
  background: var(--accent);
  animation: saving-pulse 0.9s ease-in-out infinite alternate;
}

@keyframes saving-pulse {
  from {
    opacity: 0.25;
  }
}

/* Room for why moving the mouse stops it. */
.running-detail {
  -webkit-line-clamp: 3;
}

.actions {
  flex: none;
  display: flex;
  gap: 6px;
}

.close {
  position: absolute;
  top: 4px;
  right: 4px;
  padding: 2px;
  color: var(--muted);
}

.close svg {
  width: 14px;
  height: 14px;
}
</style>
