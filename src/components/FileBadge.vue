<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { api, errorMessage } from "../api";
import { showToast, type CaptureDoc } from "../store";
import { FORMAT_INFO } from "../formats";
import type { ExportFormat } from "../types";

/*
 * The draggable file icon in the corner of the editor's preview: drag it out
 * to drop the image as a real file (like Skitch's drag handle), named and
 * formatted as set in the toolbar's export controls.
 */
const props = defineProps<{
  capture: CaptureDoc;
  render: () => Promise<Uint8Array>;
  ready: boolean;
  format: ExportFormat;
  /** Bumped on every edit to the image. */
  revision: number;
  /** The editor is the visible tab. */
  active: boolean;
  /** A text box is being typed in; rendering would end that. */
  busy: boolean;
}>();

const info = computed(() => FORMAT_INFO[props.format]);

const DRAG_THRESHOLD = 3;
/**
 * The press under way. The drag is armed the moment the icon is pressed;
 * waiting for the file before the drag could begin lost quick flicks.
 */
type Press = { x: number; y: number; moved: boolean; ready: boolean };
let press: Press | null = null;
let files = 0;
const dragging = ref(false);

/*
 * The file is written ahead of time, once the image has sat unchanged for a
 * moment or the pointer comes over the icon, so it is complete even when a
 * quick flick drops it a fraction of a second after the grab. A big capture
 * takes a second or two to write.
 */
let prepared: { key: string; id: string } | null = null;
const preparedKey = () => `${props.revision}|${props.format}|${props.capture.name}`;

function writeFile(): string {
  const id = `${Date.now()}-${++files}`;
  const options = { id, format: props.format, name: props.capture.name };
  const entry = { key: preparedKey(), id };
  prepared = entry;
  props
    .render()
    .then((png) => api.dragFile(png, options))
    .catch((err) => {
      if (prepared === entry) prepared = null;
      if (press) showToast(errorMessage(err), "error");
    });
  return id;
}

function prepare() {
  if (!props.ready || !props.active || props.busy || prepared?.key === preparedKey()) return;
  writeFile();
}

let idle: ReturnType<typeof setTimeout> | undefined;
watch(
  () => [props.revision, props.format, props.capture.name, props.ready, props.active, props.busy],
  () => {
    clearTimeout(idle);
    idle = setTimeout(prepare, 800);
  },
  { immediate: true },
);
onBeforeUnmount(() => clearTimeout(idle));

async function onBadgeDown(event: PointerEvent) {
  if (event.button !== 0 || !props.ready || press || dragging.value) return;
  event.preventDefault();
  const current: Press = { x: event.clientX, y: event.clientY, moved: false, ready: false };
  press = current;
  window.addEventListener("pointermove", onBadgeMove);
  window.addEventListener("pointerup", endPress);
  window.addEventListener("blur", endPress);
  /* Use the file written ahead if it is still current, else write it now. */
  const id = prepared?.key === preparedKey() ? prepared.id : writeFile();
  try {
    const status = await api.armDrag({ id, format: props.format, name: props.capture.name });
    if (status !== "ready") return endPress();
    /* Windows and Linux: start once the mouse moves, or now if it already has. */
    current.ready = true;
    if (current.moved && press === current) void startDrag();
  } catch (err) {
    endPress();
    showToast(errorMessage(err), "error");
  }
}

function endPress() {
  press = null;
  window.removeEventListener("pointermove", onBadgeMove);
  window.removeEventListener("pointerup", endPress);
  window.removeEventListener("blur", endPress);
}

function onBadgeMove(event: PointerEvent) {
  if (!press) return;
  if (!(event.buttons & 1)) return endPress();
  if (Math.hypot(event.clientX - press.x, event.clientY - press.y) < DRAG_THRESHOLD) return;
  press.moved = true;
  if (press.ready) void startDrag();
}

async function startDrag() {
  endPress();
  dragging.value = true;
  try {
    await api.startDrag();
  } catch (err) {
    showToast(errorMessage(err), "error");
  } finally {
    dragging.value = false;
  }
}

/*
 * macOS lets a press reach a background window only where Rust is told to,
 * so it knows where the icon is: as the window goes to the background, and
 * whenever the icon moves or shows while it is there.
 */
const badge = ref<HTMLElement | null>(null);
let reported = "";
function reportHandle() {
  const el = badge.value;
  let rect: [number, number, number, number] | null = null;
  if (el && props.ready && props.active && !props.busy) {
    const r = el.getBoundingClientRect();
    if (r.width > 0 && r.height > 0) rect = [r.left, r.top, r.width, r.height];
  }
  const key = JSON.stringify(rect);
  if (key === reported) return;
  reported = key;
  api.setDragHandle(rect).catch(() => {});
}
let observer: ResizeObserver | undefined;
onMounted(() => {
  window.addEventListener("blur", reportHandle);
  window.addEventListener("resize", reportHandle);
  observer = new ResizeObserver(reportHandle);
  if (badge.value?.offsetParent) observer.observe(badge.value.offsetParent);
  reportHandle();
});
watch(() => [props.ready, props.active, props.busy], reportHandle, { flush: "post" });
onBeforeUnmount(() => {
  window.removeEventListener("blur", reportHandle);
  window.removeEventListener("resize", reportHandle);
  observer?.disconnect();
  if (reported !== "null") void api.setDragHandle(null).catch(() => {});
});

const badgeTitle = computed(() =>
  props.ready
    ? `Drag to drop ${props.capture.name}.${props.format} anywhere: the Desktop, a folder, a browser, a chat…`
    : info.value.note,
);
</script>

<template>
  <div
    ref="badge"
    class="file-badge"
    :class="{ draggable: ready, dragging }"
    :title="badgeTitle"
    role="button"
    :aria-label="`Drag out as ${capture.name}.${format}`"
    @pointerenter="prepare"
    @pointerdown="onBadgeDown"
    @dragstart.prevent
  >
    <!-- A paper page with a folded corner, a picture in the format's color and its name on a colored band. -->
    <svg viewBox="0 0 40 48" aria-hidden="true">
      <path class="page" d="M8 2h18l10 10v31a3 3 0 0 1-3 3H8a3 3 0 0 1-3-3V5a3 3 0 0 1 3-3z" />
      <path class="fold" d="M26 2v7a3 3 0 0 0 3 3h7z" />
      <path d="M10 30l6.5-7.5 4.5 5 3-3.5 6 6z" :fill="info.color" fill-opacity="0.45" />
      <circle cx="25.5" cy="18" r="2.6" :fill="info.color" fill-opacity="0.7" />
      <path d="M5 34h31v9a3 3 0 0 1-3 3H8a3 3 0 0 1-3-3z" :fill="info.color" />
      <text x="20.5" y="42.7" text-anchor="middle" :font-size="info.label.length > 3 ? 7.4 : 8.6">{{ info.label }}</text>
    </svg>
  </div>
</template>
