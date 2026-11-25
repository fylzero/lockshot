<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
import { open, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import Icon from "../components/Icon.vue";
import HotkeyRecorder from "../components/HotkeyRecorder.vue";
import JsonEditor from "../components/JsonEditor.vue";
import PermissionCard from "../components/PermissionCard.vue";
import InputPermissionCard from "../components/InputPermissionCard.vue";
import SystemShortcutNotice from "../components/SystemShortcutNotice.vue";
import { api, errorMessage } from "../api";
import { env, saveSettingsSoon, settings, showToast, systemShortcuts } from "../store";
import { ALL_FORMATS, FORMAT_INFO } from "../formats";
import { DEFAULT_TEMPLATE, FILENAME_TOKENS, formatFilename } from "../filename";
import { HOTKEY_ACTIONS, formatAccelerator, pcKeyboardPreset, systemClashes, systemPreset } from "../hotkeys";
import { THEME_OPTIONS, resolvedTheme } from "../theme";
import { fileManagerName, isMac, isWindows, modKey } from "../platform";
import type { HotkeyAction } from "../types";

/* A custom value from the JSON editor shows up in the list too. */
const scrollPageLimits = computed(() =>
  [...new Set([50, 100, 200, 500, 1000, settings.value?.scrollMaxPages ?? 200])].sort((a, b) => a - b),
);

/* Window opacity and Background blur, as in Dispatch. */
const DEFAULT_OPACITY = 0.3;
const DEFAULT_BLUR = 4;
/* macOS takes a blur radius; Windows only has its on/off Acrylic blur; Linux has none. */
const blurSupported = isMac || isWindows;
const blurHint = isMac
  ? "Softens what shows through the background. Needs opacity below 100%."
  : isWindows
    ? "Turns on the Windows Acrylic blur behind the background. Windows has no strength setting, so any amount is the same."
    : "Linux has no standard way to blur behind a window.";
const blurLabel = computed(() => {
  const blur = settings.value?.blur ?? 0;
  if (!blurSupported || blur === 0) return "Off";
  return isWindows ? "On" : `${Math.round(blur)}px`;
});

/* The theme's own background behind the image, from --editor-rgb in styles.css. */
const themeBackground = computed(() => (resolvedTheme.value === "light" ? "#ffffff" : "#08090c"));
const backgroundColor = computed(() => settings.value?.backgroundColor || themeBackground.value);
function setBackgroundColor(value: string) {
  if (!settings.value) return;
  settings.value.backgroundColor = value;
  saveSettingsSoon();
}

const SECTIONS = [
  { id: "general", label: "General", icon: "settings" },
  { id: "appearance", label: "Appearance", icon: "appearance" },
  { id: "hotkeys", label: "Hotkeys", icon: "keyboard" },
  { id: "permissions", label: "Permissions", icon: "shield" },
  { id: "json", label: "JSON", icon: "document" },
];

/* Screens for "Open the editor on"; a saved one that is unplugged stays listed. */
const screens = ref<string[]>([]);
api
  .screens()
  .then((names) => (screens.value = names))
  .catch(() => {});
const screenOptions = computed(() => {
  const saved = settings.value?.editorScreen ?? "";
  const options = screens.value.map((name) => ({ value: name, label: name }));
  if (saved && !screens.value.includes(saved)) {
    options.push({ value: saved, label: `${saved} (not connected)` });
  }
  return options;
});

const route = useRoute();
const router = useRouter();
/* Unknown sections (like the old About page) fall back to General. */
const section = computed(() => {
  const id = route.params.section as string;
  return SECTIONS.some((s) => s.id === id) ? id : "general";
});
const hotkeyError = ref("");

const preview = computed(() =>
  formatFilename(settings.value?.filenameTemplate ?? DEFAULT_TEMPLATE, new Date(), { width: 1440, height: 900 }),
);

function openFolder(path: string) {
  api.openFolder(path).catch((err) => showToast(errorMessage(err), "error"));
}

async function chooseFolder() {
  const picked = await open({ directory: true, defaultPath: settings.value?.saveDir || env.defaultSaveDir });
  if (typeof picked === "string" && settings.value) {
    settings.value.saveDir = picked;
    saveSettingsSoon();
  }
}

async function chooseQuickSaveFolder() {
  const picked = await open({
    directory: true,
    defaultPath: settings.value?.quickSaveDir || settings.value?.saveDir || env.defaultSaveDir,
  });
  if (typeof picked === "string" && settings.value) {
    settings.value.quickSaveDir = picked;
    saveSettingsSoon();
  }
}

function insertToken(token: string) {
  if (!settings.value) return;
  settings.value.filenameTemplate += token;
  saveSettingsSoon();
}

async function setHotkey(action: HotkeyAction, accelerator: string) {
  hotkeyError.value = "";
  try {
    const saved = await api.setHotkey(action, accelerator);
    if (settings.value) settings.value.hotkeys = saved.hotkeys;
    env.hotkeyErrors = [];
  } catch (err) {
    hotkeyError.value = errorMessage(err);
  }
}

async function resetHotkeys() {
  hotkeyError.value = "";
  try {
    const saved = await api.resetHotkeys();
    if (settings.value) settings.value.hotkeys = saved.hotkeys;
    env.hotkeyErrors = [];
    showToast("Hotkeys reset to the defaults.");
  } catch (err) {
    hotkeyError.value = errorMessage(err);
    const fresh = await api.bootstrap().catch(() => null);
    if (fresh && settings.value) settings.value.hotkeys = fresh.settings.hotkeys;
  }
}

const preset = computed(() => systemPreset(env.os));
const pcPreset = computed(() => pcKeyboardPreset(env.os));
const clashCount = computed(() => systemClashes(settings.value?.hotkeys, systemShortcuts.value).length);

async function applyPreset(which = preset.value) {
  if (!which) return;
  for (const [action, accelerator] of Object.entries(which.hotkeys)) {
    await setHotkey(action as HotkeyAction, accelerator);
    if (hotkeyError.value) return;
  }
  const os = env.os === "macos" ? "macOS" : "Windows";
  showToast(
    clashCount.value
      ? `Almost there: turn off ${os}'s own screenshot shortcut, as shown at the top.`
      : which === pcPreset.value
        ? "Now using PrtSc on your PC keyboard."
        : "Now using the system screenshot keys.",
  );
}

async function openKeyboardSettings() {
  try {
    await api.openSystemSettings("keyboard-shortcuts");
  } catch (err) {
    showToast(errorMessage(err), "error");
  }
}

/* ---- JSON ---- */

const JSON_FILTERS = [{ name: "JSON", extensions: ["json"] }];
const json = ref("");
const savedJson = ref("");
const jsonError = ref("");
const jsonBusy = ref(false);
const jsonDirty = computed(() => json.value !== savedJson.value);
const jsonErrorLine = computed(() => Number(jsonError.value.match(/^Line (\d+),/)?.[1]) || null);

async function loadJson() {
  try {
    json.value = savedJson.value = await api.settingsJson();
    jsonError.value = "";
  } catch (err) {
    jsonError.value = errorMessage(err);
  }
}

/* Show the latest settings, unless there are edits here to keep. */
watch(
  [section, settings],
  () => {
    if (section.value === "json" && !jsonDirty.value) void loadJson();
  },
  { immediate: true, deep: true },
);

async function saveJson() {
  if (jsonBusy.value) return;
  jsonBusy.value = true;
  try {
    settings.value = await api.applySettingsJson(json.value);
    env.hotkeyErrors = (await api.bootstrap()).hotkeyErrors;
    await loadJson();
    showToast("Settings saved.");
  } catch (err) {
    jsonError.value = errorMessage(err);
  } finally {
    jsonBusy.value = false;
  }
}

function revertJson() {
  json.value = savedJson.value;
  jsonError.value = "";
}

async function importJson() {
  const picked = await open({ title: "Import settings", multiple: false, filters: JSON_FILTERS });
  if (typeof picked !== "string") return;
  try {
    json.value = await api.importSettingsJson(picked);
    jsonError.value = "";
    showToast("Imported. Look it over, then Save to use it.");
  } catch (err) {
    jsonError.value = errorMessage(err);
  }
}

async function exportJson() {
  const picked = await saveDialog({
    title: "Export settings",
    defaultPath: "lockshot-settings.json",
    filters: JSON_FILTERS,
  });
  if (!picked) return;
  try {
    await api.exportSettingsJson(picked);
    showToast(
      jsonDirty.value ? "Exported your saved settings, without the unsaved edits." : "Settings exported.",
      "info",
      { label: `Show in ${fileManagerName}`, run: () => void api.revealPath(picked) },
    );
  } catch (err) {
    showToast(errorMessage(err), "error");
  }
}

async function copyJson() {
  try {
    await writeText(json.value);
    showToast("JSON copied.");
  } catch (err) {
    showToast(errorMessage(err), "error");
  }
}

function onJsonKeydown(event: KeyboardEvent) {
  if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "s") {
    event.preventDefault();
    void saveJson();
  }
}

const EDITOR_KEYS = [
  ["V", "Select and move"], ["A", "Arrow"], ["L", "Line"], ["R", "Rectangle"], ["O", "Ellipse"],
  ["P", "Pen"], ["H", "Highlighter"], ["T", "Text"], ["N", "Numbered step"], ["B", "Pixelate"],
  ["C", "Crop"], ["1–4", "Stroke size"], ["⇧ drag", "Straight lines and strokes, squares"],
  [`${modKey}Z`, "Undo"], [`${modKey}⇧Z`, "Redo"], [`${modKey}C`, "Copy image"],
  [`${modKey}S`, "Save"], [`${modKey}⇧S`, "Save As"], [`${modKey}0`, "Zoom to fit"],
];
</script>

<template>
  <div v-if="settings" class="settings">
    <nav class="settings-nav">
      <RouterLink
        v-for="s in SECTIONS"
        :key="s.id"
        :to="`/settings/${s.id}`"
        :class="{ active: section === s.id }"
      >
        <Icon :name="s.icon" /> {{ s.label }}
        <span v-if="s.id === 'hotkeys' && (env.hotkeyErrors.length || clashCount)" class="dot" />
      </RouterLink>
    </nav>

    <div class="settings-body" :class="{ fill: section === 'json' }">
      <!-- General -->
      <template v-if="section === 'general'">
        <h2>General</h2>
        <p class="muted">Where captures go and what they're called.</p>

        <div class="card">
          <div class="row">
            <div class="row-label">
              Save folder
              <small>{{ settings.saveDir || `${env.defaultSaveDir} (default)` }}</small>
            </div>
            <div class="row-control">
              <button v-if="settings.saveDir" class="ghost" @click="settings.saveDir = ''; saveSettingsSoon()">Use default</button>
              <button class="ghost" :title="`Open in ${fileManagerName}`" @click="openFolder(settings.saveDir || env.defaultSaveDir)">Open</button>
              <button @click="chooseFolder"><Icon name="folder" /> Choose…</button>
            </div>
          </div>
          <div class="row">
            <div class="row-label">
              Default format
              <small>{{ FORMAT_INFO[settings.format].note }}. You can still pick another format for each capture.</small>
            </div>
            <div class="row-control">
              <select v-model="settings.format" @change="saveSettingsSoon">
                <option v-for="f in ALL_FORMATS" :key="f" :value="f" :disabled="!env.formats.includes(f)">
                  {{ FORMAT_INFO[f].label }}{{ env.formats.includes(f) ? "" : " (unavailable)" }}
                </option>
              </select>
            </div>
          </div>
          <div v-if="settings.format === 'jpg'" class="row">
            <div class="row-label">
              JPG quality
              <small>Higher keeps more detail and makes bigger files.</small>
            </div>
            <div class="row-control">
              <input v-model.number="settings.jpegQuality" type="range" min="40" max="100" step="5" @change="saveSettingsSoon" />
              <span style="width: 2.5rem; text-align: right">{{ settings.jpegQuality }}</span>
            </div>
          </div>
          <div class="row">
            <div class="row-label">
              File name
              <small>Preview: <code>{{ preview }}.{{ settings.format }}</code></small>
            </div>
            <div class="row-control">
              <input v-model="settings.filenameTemplate" type="text" spellcheck="false" :placeholder="DEFAULT_TEMPLATE" @input="saveSettingsSoon" />
              <button class="ghost" title="Reset" @click="settings.filenameTemplate = DEFAULT_TEMPLATE; saveSettingsSoon()">
                <Icon name="reset" />
              </button>
            </div>
          </div>
          <div class="token-list">
            <button v-for="t in FILENAME_TOKENS" :key="t.token" :title="`Inserts ${t.example}`" @click="insertToken(t.token)">
              {{ t.token }}
            </button>
          </div>
        </div>

        <div class="card">
          <div class="row">
            <div class="row-label">
              Save without editing
              <small>
                Captures go straight to a file in the default format instead of opening the editor. A notice shows
                each one, with Edit for the ones that need markup. Also in the {{ env.os === 'macos' ? 'menu bar' : 'tray' }} menu.
              </small>
            </div>
            <label class="switch">
              <input v-model="settings.quickSave" type="checkbox" @change="saveSettingsSoon" />
              <span />
            </label>
          </div>
          <div class="row">
            <div class="row-label">
              Folder for saves without editing
              <small>{{ settings.quickSaveDir || "Same as the save folder" }}</small>
            </div>
            <div class="row-control">
              <button v-if="settings.quickSaveDir" class="ghost" @click="settings.quickSaveDir = ''; saveSettingsSoon()">
                Use save folder
              </button>
              <button class="ghost" :title="`Open in ${fileManagerName}`" @click="openFolder(settings.quickSaveDir || settings.saveDir || env.defaultSaveDir)">
                Open
              </button>
              <button @click="chooseQuickSaveFolder"><Icon name="folder" /> Choose…</button>
            </div>
          </div>
        </div>

        <div class="card">
          <div class="card-title">Scrolling capture</div>
          <div class="row">
            <div class="row-label">
              Direction
              <small>
                Lockshot scrolls the area you select and saves a page at a time to a new folder in the folder for
                saves without editing. Chat apps keep older messages above, so scroll up to save a thread's history.
              </small>
            </div>
            <div class="row-control">
              <select v-model="settings.scrollDirection" @change="saveSettingsSoon">
                <option value="down">Down, to the end</option>
                <option value="up">Up, to the top</option>
              </select>
            </div>
          </div>
          <div class="row">
            <div class="row-label">
              Wait after each scroll
              <small>Time for the app to load and draw the next messages. Raise it if pages come out half drawn.</small>
            </div>
            <div class="row-control">
              <input v-model.number="settings.scrollDelayMs" type="range" min="100" max="3000" step="100" @change="saveSettingsSoon" />
              <span style="width: 4rem; text-align: right">{{ (settings.scrollDelayMs / 1000).toFixed(1) }} s</span>
            </div>
          </div>
          <div class="row">
            <div class="row-label">
              Page limit
              <small>Stops a feed that never ends from filling the disk.</small>
            </div>
            <div class="row-control">
              <select v-model.number="settings.scrollMaxPages" @change="saveSettingsSoon">
                <option v-for="n in scrollPageLimits" :key="n" :value="n">{{ n }} pages</option>
              </select>
            </div>
          </div>
          <div class="row">
            <div class="row-label">
              Save
              <small>
                The long image joins the pages into one tall picture, without the overlap. Very long threads are split
                into parts of about 30,000 pixels. It is a PNG, or a JPEG when that is your default format.
              </small>
            </div>
            <div class="row-control">
              <select v-model="settings.scrollOutput" @change="saveSettingsSoon">
                <option value="both">Pages and a long image</option>
                <option value="image">Just the long image</option>
                <option value="pages">Just the pages</option>
              </select>
            </div>
          </div>
          <div v-if="env.os === 'macos'" class="row">
            <div class="row-label">
              Save the text
              <small>Reads the messages off the pages into Text.txt, on this Mac. Adds a few seconds at the end of a long run.</small>
            </div>
            <label class="switch">
              <input v-model="settings.scrollText" type="checkbox" @change="saveSettingsSoon" />
              <span />
            </label>
          </div>
        </div>

        <div class="card">
          <div class="row">
            <div class="row-label">
              Launch at login
              <small>Start Lockshot in the {{ env.os === 'macos' ? 'menu bar' : 'tray' }} so the hotkeys work right away.</small>
            </div>
            <label class="switch">
              <input v-model="settings.launchAtLogin" type="checkbox" @change="saveSettingsSoon" />
              <span />
            </label>
          </div>
          <div v-if="env.os === 'macos'" class="row">
            <div class="row-label">
              Keep running in the background
              <small>⌘Q closes the window and leaves Lockshot in the menu bar so the hotkeys keep working. Quit from the menu bar icon to stop it.</small>
            </div>
            <label class="switch">
              <input v-model="settings.keepRunningInBackground" type="checkbox" @change="saveSettingsSoon" />
              <span />
            </label>
          </div>
          <div class="row">
            <div class="row-label">
              Open the editor on
              <small>Lockshot always opens on the desktop you're on. Pick a screen to keep it there; if that screen is unplugged it opens where you captured.</small>
            </div>
            <div class="row-control">
              <select v-model="settings.editorScreen" @change="saveSettingsSoon">
                <option value="">The screen you captured on</option>
                <option v-for="screen in screenOptions" :key="screen.value" :value="screen.value">{{ screen.label }}</option>
              </select>
            </div>
          </div>
          <div class="row">
            <div class="row-label">
              Resize the window to each capture
              <small>The window grows or shrinks to show a capture or opened image at 1:1, up to the size of the screen.</small>
            </div>
            <label class="switch">
              <input v-model="settings.resizeWindowToCapture" type="checkbox" @change="saveSettingsSoon" />
              <span />
            </label>
          </div>
          <div class="row">
            <div class="row-label">
              Always open captures at 1:1
              <small>Show every pixel at its real size and scroll to see the rest. Off, a capture opens at 1:1 when it fits and is shrunk to fit when it doesn't.</small>
            </div>
            <label class="switch">
              <input v-model="settings.alwaysActualSize" type="checkbox" @change="saveSettingsSoon" />
              <span />
            </label>
          </div>
          <div class="row">
            <div class="row-label">
              Copy every capture to the clipboard
              <small>Paste straight into chat or docs without saving. Also applies to saves without editing.</small>
            </div>
            <label class="switch">
              <input v-model="settings.copyOnCapture" type="checkbox" @change="saveSettingsSoon" />
              <span />
            </label>
          </div>
          <div class="row">
            <div class="row-label">
              Settings file
              <small>{{ env.settingsPath }}</small>
            </div>
            <div class="row-control">
              <button @click="api.revealSettingsFile().catch((e) => showToast(errorMessage(e), 'error'))">
                Show in {{ fileManagerName }}
              </button>
              <button @click="router.push('/settings/json')">Edit JSON</button>
            </div>
          </div>
        </div>
      </template>

      <!-- Appearance -->
      <template v-else-if="section === 'appearance'">
        <h2>Appearance</h2>
        <p class="muted">How Lockshot looks.</p>

        <div class="card">
          <div class="row">
            <div class="row-label">
              Theme
              <small>System follows your light or dark appearance setting.</small>
            </div>
            <div class="segmented" role="group" aria-label="Theme">
              <button
                v-for="t in THEME_OPTIONS"
                :key="t.id"
                type="button"
                :class="{ active: settings.theme === t.id }"
                :aria-pressed="settings.theme === t.id"
                @click="settings.theme = t.id; saveSettingsSoon()"
              >
                {{ t.label }}
              </button>
            </div>
          </div>
          <div class="row">
            <div class="row-label">
              Background color
              <small>Behind the image in the editor. Reset goes back to the theme's own.</small>
            </div>
            <div class="row-control">
              <label class="color-chip" title="Pick a color">
                <span class="color-chip-dot" :style="{ background: backgroundColor }" />
                <code>{{ settings.backgroundColor || "Theme" }}</code>
                <input
                  type="color"
                  :value="backgroundColor"
                  aria-label="Background color"
                  @input="setBackgroundColor(($event.target as HTMLInputElement).value)"
                />
              </label>
              <button
                class="ghost"
                title="Reset"
                :disabled="!settings.backgroundColor"
                @click="setBackgroundColor('')"
              >
                <Icon name="reset" />
              </button>
            </div>
          </div>
          <div class="row">
            <div class="row-label">
              Window opacity
              <small>How solid the background behind the image is. Lower shows the desktop through it.</small>
            </div>
            <div class="row-control">
              <input
                v-model.number="settings.opacity"
                type="range"
                min="0.05"
                max="1"
                step="0.05"
                aria-label="Window opacity"
                @input="saveSettingsSoon"
              />
              <span style="width: 2.5rem; text-align: right">{{ Math.round(settings.opacity * 100) }}%</span>
              <button
                class="ghost"
                title="Reset"
                :disabled="settings.opacity === DEFAULT_OPACITY"
                @click="settings.opacity = DEFAULT_OPACITY; saveSettingsSoon()"
              >
                <Icon name="reset" />
              </button>
            </div>
          </div>
          <div class="row">
            <div class="row-label">
              Background blur
              <small>{{ blurHint }}</small>
            </div>
            <div class="row-control">
              <input
                v-model.number="settings.blur"
                type="range"
                min="0"
                max="40"
                step="1"
                aria-label="Background blur"
                :disabled="!blurSupported"
                @input="saveSettingsSoon"
              />
              <span style="width: 2.5rem; text-align: right">{{ blurLabel }}</span>
              <button
                class="ghost"
                title="Reset"
                :disabled="!blurSupported || settings.blur === DEFAULT_BLUR"
                @click="settings.blur = DEFAULT_BLUR; saveSettingsSoon()"
              >
                <Icon name="reset" />
              </button>
            </div>
          </div>
        </div>
      </template>

      <!-- Hotkeys -->
      <template v-else-if="section === 'hotkeys'">
        <h2>Hotkeys</h2>
        <p class="muted">Click a shortcut, then press the keys you want. They work from any app. <kbd>Esc</kbd> cancels, <kbd>Delete</kbd> turns one off.</p>

        <div v-if="env.hotkeyErrors.length" class="notice bad">
          <Icon name="info" />
          <div>
            <strong>Some hotkeys could not be registered</strong>
            <p v-for="e in env.hotkeyErrors" :key="e" class="muted">{{ e }}</p>
            <p class="muted">Pick a different combination below, or free it up in the app that owns it.</p>
          </div>
        </div>
        <SystemShortcutNotice />
        <div v-if="hotkeyError" class="notice bad">
          <Icon name="info" />
          <div><strong>{{ hotkeyError }}</strong></div>
        </div>

        <div class="card">
          <div v-for="a in HOTKEY_ACTIONS" :key="a.id" class="row">
            <div class="row-label">
              {{ a.label }}
              <small>{{ a.hint }}</small>
            </div>
            <div class="row-control">
              <HotkeyRecorder
                :value="settings.hotkeys[a.id]"
                :label="a.label"
                @change="setHotkey(a.id, $event)"
                @invalid="hotkeyError = $event"
              />
            </div>
          </div>
          <div class="row">
            <div class="row-label">
              <small>Defaults: area {{ formatAccelerator("Alt+Shift+4", env.os) }}, window {{ formatAccelerator("Alt+Shift+5", env.os) }}, scrolling {{ formatAccelerator("Alt+Shift+6", env.os) }}, screen {{ formatAccelerator("Alt+Shift+3", env.os) }}, open {{ formatAccelerator("Alt+Shift+L", env.os) }}</small>
            </div>
            <div class="row-control">
              <button v-if="env.os === 'macos' && preset" @click="applyPreset(preset)">{{ preset.label }}</button>
              <button @click="resetHotkeys">{{ env.os === "macos" ? "Use ⌥" : "Use Alt" }}</button>
              <button v-if="pcPreset" @click="applyPreset(pcPreset)">{{ pcPreset.label }}</button>
              <button v-else-if="preset" @click="applyPreset(preset)">{{ preset.label }}</button>
            </div>
          </div>
        </div>

        <div class="card">
          <div class="card-title">Use the system screenshot keys for Lockshot</div>
          <div class="row">
            <div class="row-label">
              <template v-if="env.os === 'macos'">
                macOS keeps <kbd>⌘⇧3</kbd>, <kbd>⌘⇧4</kbd> and <kbd>⌘⇧5</kbd> for its own Screenshot app. To hand them to Lockshot:
                <ol class="steps">
                  <li>Click <strong>Open keyboard settings</strong> below. It opens the Keyboard Shortcuts window (or the Keyboard page: click <strong>Keyboard Shortcuts…</strong> there).</li>
                  <li>Click <strong>Screenshots</strong> in the list on the left, untick the shortcuts you want Lockshot to have, and click <strong>Done</strong>.</li>
                  <li>Come back here and click <strong>{{ preset?.label }}</strong>.</li>
                </ol>
                <p class="muted">
                  Using a PC keyboard? macOS sees its <kbd>PrtSc</kbd> key as <kbd>F13</kbd>, which nothing else uses,
                  so click <strong>{{ pcPreset?.label }}</strong> above: <kbd>PrtSc</kbd> captures an area and
                  <kbd>Shift</kbd>+<kbd>PrtSc</kbd> the whole screen. No system settings to change.
                </p>
              </template>
              <template v-else-if="env.os === 'windows'">
                Windows sends <kbd>PrtSc</kbd> to the Snipping Tool by default. To hand it to Lockshot:
                <ol class="steps">
                  <li>Open <strong>Settings › Accessibility › Keyboard</strong>.</li>
                  <li>Turn off <strong>Use the Print screen key to open screen capture</strong> (Snipping Tool).</li>
                  <li>If OneDrive or Dropbox also claims <kbd>PrtSc</kbd>, turn that off in their settings too.</li>
                  <li>Come back here and click <strong>{{ preset?.label }}</strong>.</li>
                </ol>
              </template>
              <template v-else>
                Most desktops bind <kbd>PrtSc</kbd> to their own screenshot tool. To hand it to Lockshot:
                <ol class="steps">
                  <li>Open your desktop's keyboard settings (GNOME: <strong>Settings › Keyboard › View and Customize Shortcuts › Screenshots</strong>; KDE: <strong>System Settings › Shortcuts › Spectacle</strong>).</li>
                  <li>Remove the <kbd>PrtSc</kbd> bindings.</li>
                  <li>On X11, click <strong>{{ preset?.label }}</strong>. On Wayland, add custom shortcuts that run <code>lockshot --area</code> and <code>lockshot --screen</code>.</li>
                </ol>
              </template>
            </div>
          </div>
          <div v-if="env.os !== 'linux'" class="row">
            <div class="row-label" />
            <div class="row-control">
              <button @click="openKeyboardSettings"><Icon name="external" /> Open keyboard settings</button>
            </div>
          </div>
        </div>

        <div class="card">
          <div class="card-title">While capturing</div>
          <div class="row">
            <div class="row-label muted">
              Drag to select an area · click once for the whole screen · in Capture window, click the highlighted window · <kbd>Esc</kbd> or right-click to cancel.
              Captures also start from the Lockshot icon in the {{ env.os === "macos" ? "menu bar" : "system tray" }}.
            </div>
          </div>
        </div>

        <div class="card">
          <div class="card-title">In the editor</div>
          <div class="row" style="display: grid; grid-template-columns: repeat(auto-fill, minmax(13rem, 1fr)); gap: 0.4rem 1rem">
            <div v-for="[k, label] in EDITOR_KEYS" :key="k" class="muted" style="display: flex; gap: 0.5rem; align-items: center">
              <kbd>{{ k }}</kbd> {{ label }}
            </div>
          </div>
        </div>
      </template>

      <!-- Permissions -->
      <template v-else-if="section === 'permissions'">
        <h2>Permissions</h2>
        <p class="muted">What your OS needs to let Lockshot capture the screen.</p>
        <PermissionCard />
        <InputPermissionCard />
      </template>

      <!-- JSON -->
      <template v-else>
        <div class="json-head">
          <div>
            <h2>JSON</h2>
            <p class="muted json-path" :title="env.settingsPath">{{ env.settingsPath }}</p>
          </div>
          <div class="row-control">
            <button @click="api.revealSettingsFile().catch((e) => showToast(errorMessage(e), 'error'))">Reveal</button>
            <button @click="importJson"><Icon name="import" /> Import</button>
            <button @click="exportJson"><Icon name="export" /> Export</button>
            <button v-if="jsonDirty" class="ghost" @click="revertJson">Revert</button>
            <button class="primary" :disabled="!jsonDirty || jsonBusy" @click="saveJson">Save</button>
          </div>
        </div>

        <div v-if="jsonError" class="notice bad">
          <Icon name="info" />
          <div><strong>{{ jsonError }}</strong></div>
        </div>

        <div class="json-wrap" @keydown="onJsonKeydown">
          <button class="json-copy" @click="copyJson"><Icon name="copy" /> Copy</button>
          <JsonEditor v-model="json" :error-line="jsonErrorLine" />
        </div>
      </template>
    </div>
  </div>
</template>
