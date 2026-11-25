<script setup lang="ts">
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { computed, defineAsyncComponent, onBeforeUnmount, onMounted } from "vue";
import { useRoute, useRouter } from "vue-router";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import TabBar from "./components/TabBar.vue";
import { useTabs } from "./tabs";
import { isMac } from "./platform";
import Modal from "./components/Modal.vue";
import { useUpdater } from "./composables/useUpdater";
import { api, errorMessage } from "./api";
import {
  OPENABLE_EXTENSIONS,
  captures,
  dismissToast,
  loadSettings,
  openCapture,
  settings,
  showToast,
  toast,
} from "./store";
import type { CaptureReady, Settings } from "./types";

const router = useRouter();
const route = useRoute();
const EditorView = defineAsyncComponent(() => import("./views/EditorView.vue"));

const { active: activeTab, close: closeTab } = useTabs(router, route);

/**
 * ⌘W / Ctrl+W: close the active tab, working back toward Home; on Home, hide
 * the window (Lockshot keeps running in the menu bar / tray).
 */
function closeActiveTabOrWindow() {
  if (activeTab.value !== "home") closeTab(activeTab.value);
  else void api.hideMain();
}

/* macOS sends ⌘W through the menu; elsewhere the page sees Ctrl+W itself. */
function onWindowKeydown(event: KeyboardEvent) {
  if (isMac || !event.ctrlKey || event.altKey || event.shiftKey || event.metaKey) return;
  if (event.key.toLowerCase() !== "w") return;
  event.preventDefault();
  closeActiveTabOrWindow();
}

/** The capture whose editor tab is showing, if any. */
const activeCapture = computed(() => (route.name === "editor" ? String(route.params.id) : null));
const unlisten: UnlistenFn[] = [];

/* Window opacity and Background color in Appearance settings, for the stage behind the image. */
const shellStyle = computed(() => {
  const style: Record<string, string | number> = { "--editor-opacity": settings.value?.opacity ?? 1 };
  const hex = /^#([0-9a-f]{6})$/i.exec(settings.value?.backgroundColor ?? "")?.[1];
  if (hex) {
    const n = parseInt(hex, 16);
    style["--editor-rgb"] = `${(n >> 16) & 255} ${(n >> 8) & 255} ${n & 255}`;
  }
  return style;
});

const OPENABLE = new RegExp(`\\.(${OPENABLE_EXTENSIONS.join("|")})$`, "i");
const {
  status: updateStatus,
  statusText: updateStatusText,
  currentVersion,
  availableVersion,
  busy: updateBusy,
  promptOpen,
  checkForUpdates,
  installUpdate,
  dismissPrompt,
  startAutoChecks,
} = useUpdater();

/** Check for Updates… from the tray menu or the change log: always report back. */
async function checkForUpdatesNow() {
  await checkForUpdates({ prompt: true });
  if (updateStatus.value === "up-to-date") showToast("You're on the latest version.");
  else if (updateStatus.value === "error") showToast(updateStatusText.value, "error");
}

function onContextMenu(event: MouseEvent) {
  /* The webview's default menu (Reload, Inspect) only gets in the way. */
  const target = event.target as HTMLElement | null;
  if (!target?.closest("input, textarea")) event.preventDefault();
}

onMounted(async () => {
  window.addEventListener("contextmenu", onContextMenu);
  window.addEventListener("keydown", onWindowKeydown);
  unlisten.push(
    await listen<string>("navigate", (event) => {
      void api.takePendingRoute();
      void router.push(event.payload);
    }),
    /* Every capture or opened image gets a new editor tab. */
    await listen<CaptureReady>("capture-ready", (event) => {
      const doc = openCapture(event.payload);
      void router.push(`/editor/${doc.id}`);
    }),
    /* Drop an image file anywhere on the window to open it in the editor. */
    await getCurrentWebview().onDragDropEvent((event) => {
      if (event.payload.type !== "drop") return;
      /* A drag-out from the export bar landing back on Lockshot. */
      if (event.payload.paths.some((p) => /[\\/]lockshot-drag[\\/]/.test(p))) return;
      const file = event.payload.paths.find((p) => OPENABLE.test(p));
      if (file) void api.openImage(file).catch((err) => showToast(errorMessage(err), "error"));
      else if (event.payload.paths.length) showToast("Lockshot opens PNG, JPG, GIF, BMP, TIFF and HEIC images.", "error");
    }),
    /* Save Without Editing toggled from the tray menu. */
    await listen<Settings>("settings-changed", (event) => {
      if (settings.value) settings.value = { ...event.payload, hotkeys: settings.value.hotkeys };
    }),
    await listen("check-for-updates", () => void checkForUpdatesNow()),
    await listen("close-tab-or-window", () => closeActiveTabOrWindow()),
    await listen("open-settings", () => void router.push("/settings/general")),
    await listen<string>("capture-error", (event) => {
      showToast(event.payload, "error");
    }),
  );
  try {
    await loadSettings();
  } catch (err) {
    showToast(`Could not load settings: ${errorMessage(err)}`, "error");
  }
  /* A capture or route requested before this page finished loading. */
  if (!captures.value.length) {
    const latest = await api.latestCapture().catch(() => null);
    if (latest) openCapture(latest);
  }
  const pending = await api.takePendingRoute().catch(() => null);
  if (pending) void router.push(pending);
  /*
   * Look for a newer release now and every few hours. The tab bar shows an
   * icon; the prompt opens when the window is in front, and a notification
   * says so while Lockshot is only in the menu bar.
   */
  unlisten.push(startAutoChecks(() => settings.value?.autoCheckUpdates ?? true));
});

onBeforeUnmount(() => {
  window.removeEventListener("contextmenu", onContextMenu);
  window.removeEventListener("keydown", onWindowKeydown);
  unlisten.forEach((fn) => fn());
});
</script>

<template>
  <div class="shell" :class="{ 'see-through': (settings?.opacity ?? 1) < 1 }" :style="shellStyle">
    <TabBar />

    <main v-if="settings" class="content" :class="{ 'editor-active': activeCapture }">
      <EditorView
        v-for="capture in captures"
        v-show="activeCapture === capture.id"
        :key="capture.id"
        :capture="capture"
        :active="activeCapture === capture.id"
      />
      <RouterView v-if="!activeCapture" />
    </main>

    <Modal v-if="promptOpen" title="Update available" @close="updateBusy ? undefined : dismissPrompt()">
      <p>Lockshot {{ availableVersion }} is available. You have {{ currentVersion }}.</p>
      <p
        v-if="updateStatus === 'downloading' || updateStatus === 'installing' || updateStatus === 'error'"
        class="muted"
        style="font-size: 12px"
      >
        {{ updateStatusText }}
      </p>
      <template #actions>
        <button class="ghost" type="button" :disabled="updateBusy" @click="dismissPrompt">Later</button>
        <button class="primary" type="button" :disabled="updateBusy" @click="installUpdate">Install and restart</button>
      </template>
    </Modal>

    <div class="toast-layer" aria-live="polite">
      <Transition name="toast">
        <div v-if="toast" :key="toast.id" class="toast" :class="{ error: toast.kind === 'error' }">
          <span>{{ toast.message }}</span>
          <button v-if="toast.action" class="toast-action" type="button" @click="toast.action.run(); dismissToast()">
            {{ toast.action.label }}
          </button>
          <button class="toast-close" type="button" aria-label="Dismiss" @click="dismissToast">
            <svg viewBox="0 0 16 16" fill="currentColor" aria-hidden="true">
              <path
                d="M3.72 3.72a.75.75 0 0 1 1.06 0L8 6.94l3.22-3.22a.75.75 0 1 1 1.06 1.06L9.06 8l3.22 3.22a.75.75 0 1 1-1.06 1.06L8 9.06l-3.22 3.22a.75.75 0 0 1-1.06-1.06L6.94 8 3.72 4.78a.75.75 0 0 1 0-1.06Z"
              />
            </svg>
          </button>
        </div>
      </Transition>
    </div>
  </div>
</template>
