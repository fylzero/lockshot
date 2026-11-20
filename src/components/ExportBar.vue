<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import Icon from "./Icon.vue";
import { api, errorMessage } from "../api";
import { env, settings, showToast, type CaptureDoc } from "../store";
import { ALL_FORMATS, FORMAT_INFO } from "../formats";
import { fileManagerName, modKey } from "../platform";
import type { ExportFormat } from "../types";

const props = defineProps<{
  capture: CaptureDoc;
  render: () => Promise<Uint8Array>;
  ready: boolean;
}>();
const emit = defineEmits<{ copy: [] }>();

const format = ref<ExportFormat>(settings.value?.format ?? "png");
const busy = ref(false);

/* Every new capture starts from the default format in Settings. */
watch(
  () => props.capture.id,
  () => {
    format.value = settings.value?.format ?? "png";
  },
);

const info = computed(() => FORMAT_INFO[format.value]);
/** Where Save writes; change it in Settings › General. */
const folder = computed(() => settings.value?.saveDir || env.defaultSaveDir);
const separator = computed(() => (env.os === "windows" ? "\\" : "/"));

function supported(f: ExportFormat) {
  return env.formats.includes(f);
}

function savedToast(path: string) {
  const name = path.split(/[\\/]/).pop() ?? path;
  showToast(`Saved ${name}`, "info", {
    label: `Show in ${fileManagerName}`,
    run: () => void api.revealPath(path).catch((err) => showToast(errorMessage(err), "error")),
  });
}

async function save() {
  if (!props.ready || busy.value) return;
  busy.value = true;
  try {
    const png = await props.render();
    const path = await api.saveImage(png, {
      format: format.value,
      dir: settings.value?.saveDir ?? "",
      name: props.capture.name,
    });
    savedToast(path);
  } catch (err) {
    showToast(errorMessage(err), "error");
  } finally {
    busy.value = false;
  }
}

async function saveAs() {
  if (!props.ready || busy.value) return;
  const ext = format.value;
  const picked = await saveDialog({
    title: "Save screenshot",
    defaultPath: `${folder.value}${separator.value}${props.capture.name}.${ext}`,
    filters: [
      { name: FORMAT_INFO[ext].label, extensions: ext === "jpg" ? ["jpg", "jpeg"] : ext === "tiff" ? ["tiff", "tif"] : [ext] },
      ...ALL_FORMATS.filter((f) => f !== ext && supported(f)).map((f) => ({ name: FORMAT_INFO[f].label, extensions: [f] })),
    ],
  });
  if (!picked) return;

  /* An extension typed in the dialog picks the format. */
  const typed = picked.split(".").pop()?.toLowerCase();
  const alias: Record<string, ExportFormat> = { jpeg: "jpg", tif: "tiff" };
  const chosen = (typed && (alias[typed] ?? (ALL_FORMATS.includes(typed as ExportFormat) ? typed : null))) as ExportFormat | null;
  const finalFormat = chosen && supported(chosen) ? chosen : format.value;
  const path = chosen ? picked : `${picked}.${finalFormat}`;

  busy.value = true;
  try {
    const png = await props.render();
    savedToast(await api.saveImage(png, { format: finalFormat, path }));
    format.value = finalFormat;
  } catch (err) {
    showToast(errorMessage(err), "error");
  } finally {
    busy.value = false;
  }
}

/* ---- Drag the badge out as a file (like Skitch's drag handle) ---- */

const DRAG_THRESHOLD = 4;
let press: { x: number; y: number; png: Promise<Uint8Array> } | null = null;
const dragging = ref(false);

function onBadgeDown(event: PointerEvent) {
  if (event.button !== 0 || !props.ready || busy.value) return;
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
    await api.dragImage(bytes, { format: format.value, name: props.capture.name });
  } catch (err) {
    showToast(errorMessage(err), "error");
  } finally {
    dragging.value = false;
  }
}

const badgeTitle = computed(() =>
  props.ready
    ? `Drag to drop ${props.capture.name}.${format.value} anywhere: the Desktop, a folder, a browser, a chat…`
    : info.value.note,
);

defineExpose({ save, saveAs });
</script>

<template>
  <div class="export-bar">
    <div class="export-fields">
      <div class="export-name">
        <input
          v-model="capture.name"
          type="text"
          spellcheck="false"
          aria-label="File name"
          @keydown.enter="save"
        />
        <select v-model="format" aria-label="File format" :title="info.note">
          <option v-for="f in ALL_FORMATS" :key="f" :value="f" :disabled="!supported(f)">
            .{{ f }}{{ supported(f) ? "" : " (unavailable)" }}
          </option>
        </select>
      </div>
    </div>

    <div class="export-actions">
      <button :disabled="!ready" :title="`Copy to clipboard (${modKey}C)`" @click="emit('copy')">
        <Icon name="copy" /> Copy
      </button>
      <button :disabled="!ready || busy" :title="`Save As (${modKey}⇧S)`" @click="saveAs">
        <Icon name="saveAs" /> Save As…
      </button>
      <button class="primary" :disabled="!ready || busy" :title="`Save (${modKey}S)`" @click="save">
        <Icon name="save" /> Save
      </button>
    </div>

    <div
      class="file-badge"
      :class="{ draggable: ready, dragging }"
      :title="badgeTitle"
      role="button"
      :aria-label="`Drag out as ${capture.name}.${format}`"
      @pointerdown="onBadgeDown"
      @dragstart.prevent
    >
      <svg viewBox="0 0 40 48" aria-hidden="true">
        <path d="M6 2h20l10 10v32a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2z" fill="var(--bg-raised)" stroke="var(--border)" stroke-width="1.5" />
        <path d="M26 2v8a2 2 0 0 0 2 2h8" fill="none" stroke="var(--border)" stroke-width="1.5" />
        <path d="M10 28l6-6 5 5 3-3 6 6" fill="none" stroke="var(--text-faint)" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" />
        <circle cx="25" cy="18" r="2" fill="var(--text-faint)" />
      </svg>
      <span :style="{ background: info.color }">{{ info.label }}</span>
    </div>
  </div>
</template>
