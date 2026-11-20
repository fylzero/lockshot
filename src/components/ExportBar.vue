<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import Icon from "./Icon.vue";
import { api, errorMessage } from "../api";
import { current, env, settings, showToast } from "../store";
import { ALL_FORMATS, FORMAT_INFO } from "../formats";
import { fileManagerName, modKey } from "../platform";
import type { ExportFormat } from "../types";

const props = defineProps<{
  render: () => Promise<Uint8Array>;
  ready: boolean;
}>();
const emit = defineEmits<{ copy: [] }>();

const format = ref<ExportFormat>(settings.value?.format ?? "png");
const busy = ref(false);

/* Every new capture starts from the default format in Settings. */
watch(
  () => current.value?.id,
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
  if (!props.ready || !current.value || busy.value) return;
  busy.value = true;
  try {
    const png = await props.render();
    const path = await api.saveImage(png, {
      format: format.value,
      dir: settings.value?.saveDir ?? "",
      name: current.value.name,
    });
    savedToast(path);
  } catch (err) {
    showToast(errorMessage(err), "error");
  } finally {
    busy.value = false;
  }
}

async function saveAs() {
  if (!props.ready || !current.value || busy.value) return;
  const ext = format.value;
  const picked = await saveDialog({
    title: "Save screenshot",
    defaultPath: `${folder.value}${separator.value}${current.value.name}.${ext}`,
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

defineExpose({ save, saveAs });
</script>

<template>
  <div class="export-bar">
    <div class="file-badge" :title="info.note">
      <svg viewBox="0 0 40 48" aria-hidden="true">
        <path d="M6 2h20l10 10v32a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2z" fill="var(--bg-raised)" stroke="var(--border)" stroke-width="1.5" />
        <path d="M26 2v8a2 2 0 0 0 2 2h8" fill="none" stroke="var(--border)" stroke-width="1.5" />
        <path d="M10 28l6-6 5 5 3-3 6 6" fill="none" stroke="var(--text-faint)" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" />
        <circle cx="25" cy="18" r="2" fill="var(--text-faint)" />
      </svg>
      <span :style="{ background: info.color }">{{ info.label }}</span>
    </div>

    <div class="export-fields">
      <div class="export-name">
        <input
          v-if="current"
          v-model="current.name"
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
  </div>
</template>
