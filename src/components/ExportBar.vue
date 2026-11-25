<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from "vue";
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

/* Shared with the editor, whose draggable file icon exports in this format. */
const format = defineModel<ExportFormat>("format", { required: true });
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

/* ---- The format menu and the ⋮ menu, styled like the toolbar's menus ---- */

type Menu = "format" | "actions";
const openMenu = ref<Menu | null>(null);
const formatMenu = ref<HTMLElement | null>(null);
const actionsMenu = ref<HTMLElement | null>(null);

function closeOnOutside(event: PointerEvent) {
  const menu = openMenu.value === "format" ? formatMenu.value : actionsMenu.value;
  if (!menu?.contains(event.target as Node)) setMenu(null);
}

function closeOnEscape(event: KeyboardEvent) {
  if (event.key !== "Escape") return;
  /* Don't let the editor also treat this Escape as "deselect" or "cancel crop". */
  event.stopPropagation();
  setMenu(null);
}

function setMenu(menu: Menu | null) {
  openMenu.value = menu;
  if (menu) {
    window.addEventListener("pointerdown", closeOnOutside, true);
    window.addEventListener("keydown", closeOnEscape, true);
  } else {
    window.removeEventListener("pointerdown", closeOnOutside, true);
    window.removeEventListener("keydown", closeOnEscape, true);
  }
}

function toggleMenu(menu: Menu) {
  setMenu(openMenu.value === menu ? null : menu);
}

function pickFormat(next: ExportFormat) {
  if (!supported(next)) return;
  format.value = next;
  setMenu(null);
}

/** Close the ⋮ menu, then do what was picked. */
function runAction(action: () => unknown) {
  setMenu(null);
  void action();
}

onBeforeUnmount(() => setMenu(null));

/*
 * The editor toolbar cancels mouse presses so focus stays in a text
 * annotation being typed. Here that would leave the name field focused (and
 * outlined) after clicking the format menu or a button, so let it go.
 */
const nameInput = ref<HTMLInputElement | null>(null);

function onBarMouseDown(event: MouseEvent) {
  if (event.target !== nameInput.value && document.activeElement === nameInput.value) nameInput.value?.blur();
}

defineExpose({ save, saveAs });
</script>

<template>
  <div class="export-bar" @mousedown="onBarMouseDown">
    <div class="export-fields">
      <div class="export-name">
        <input
          ref="nameInput"
          v-model="capture.name"
          type="text"
          spellcheck="false"
          aria-label="File name"
          @keydown.enter="save"
        />
        <div ref="formatMenu" class="format-menu">
          <button
            class="format-trigger"
            :class="{ open: openMenu === 'format' }"
            :title="info.note"
            aria-label="File format"
            aria-haspopup="menu"
            :aria-expanded="openMenu === 'format'"
            @click="toggleMenu('format')"
          >
            .{{ format }}
            <Icon name="chevron" class="chevron" />
          </button>
          <div v-if="openMenu === 'format'" class="menu-pop format-pop" role="menu">
            <span class="menu-label">Format</span>
            <button
              v-for="f in ALL_FORMATS"
              :key="f"
              class="menu-item"
              :class="{ active: format === f }"
              role="menuitemradio"
              :aria-checked="format === f"
              :disabled="!supported(f)"
              :title="supported(f) ? FORMAT_INFO[f].note : 'Not available on this system'"
              @click="pickFormat(f)"
            >
              <span class="format-chip" :style="{ background: FORMAT_INFO[f].color }">{{ FORMAT_INFO[f].label }}</span>
              <span class="format-note">{{ supported(f) ? FORMAT_INFO[f].note : "Not available" }}</span>
            </button>
          </div>
        </div>
      </div>
    </div>

    <div ref="actionsMenu" class="export-actions">
      <button
        class="icon-only"
        :class="{ open: openMenu === 'actions' }"
        title="Save and copy"
        aria-label="Save and copy"
        aria-haspopup="menu"
        :aria-expanded="openMenu === 'actions'"
        @click="toggleMenu('actions')"
      >
        <Icon name="more" />
      </button>
      <div v-if="openMenu === 'actions'" class="menu-pop actions-pop" role="menu">
        <button class="menu-item" role="menuitem" :disabled="!ready || busy" @click="runAction(save)">
          <Icon name="check" />
          <span>Save</span>
          <kbd>{{ modKey }}S</kbd>
        </button>
        <button class="menu-item" role="menuitem" :disabled="!ready || busy" @click="runAction(saveAs)">
          <Icon name="saveAs" />
          <span>Save As…</span>
          <kbd>{{ modKey }}⇧S</kbd>
        </button>
        <button class="menu-item" role="menuitem" :disabled="!ready" @click="runAction(() => emit('copy'))">
          <Icon name="copy" />
          <span>Copy to clipboard</span>
          <kbd>{{ modKey }}C</kbd>
        </button>
      </div>
    </div>

  </div>
</template>
