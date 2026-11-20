<script setup lang="ts">
import { computed, ref } from "vue";
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
}>();

const info = computed(() => FORMAT_INFO[props.format]);

const DRAG_THRESHOLD = 4;
let press: { x: number; y: number; png: Promise<Uint8Array> } | null = null;
const dragging = ref(false);

function onBadgeDown(event: PointerEvent) {
  if (event.button !== 0 || !props.ready || dragging.value) return;
  event.preventDefault();
  /* Start rendering now so the drag can begin the moment the mouse moves. */
  const png = props.render();
  png.catch(() => {});
  press = { x: event.clientX, y: event.clientY, png };
  window.addEventListener("pointermove", onBadgeMove);
  window.addEventListener("pointerup", endPress);
  window.addEventListener("blur", endPress);
}

function endPress() {
  press = null;
  window.removeEventListener("pointermove", onBadgeMove);
  window.removeEventListener("pointerup", endPress);
  window.removeEventListener("blur", endPress);
}

async function onBadgeMove(event: PointerEvent) {
  if (!press) return;
  if (!(event.buttons & 1)) return endPress();
  if (Math.hypot(event.clientX - press.x, event.clientY - press.y) < DRAG_THRESHOLD) return;
  const { png } = press;
  endPress();
  dragging.value = true;
  try {
    const bytes = await png;
    await api.dragImage(bytes, { format: props.format, name: props.capture.name });
  } catch (err) {
    showToast(errorMessage(err), "error");
  } finally {
    dragging.value = false;
  }
}

const badgeTitle = computed(() =>
  props.ready
    ? `Drag to drop ${props.capture.name}.${props.format} anywhere: the Desktop, a folder, a browser, a chat…`
    : info.value.note,
);
</script>

<template>
  <div
    class="file-badge"
    :class="{ draggable: ready, dragging }"
    :title="badgeTitle"
    role="button"
    :aria-label="`Drag out as ${capture.name}.${format}`"
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
