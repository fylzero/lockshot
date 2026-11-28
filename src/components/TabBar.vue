<script setup lang="ts">
import { getVersion } from "@tauri-apps/api/app";
import { openUrl } from "@tauri-apps/plugin-opener";
import { nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useRoute, useRouter } from "vue-router";
import Icon from "./Icon.vue";
import { api, errorMessage } from "../api";
import { env, screenAllowed, settings, showToast } from "../store";
import { formatAccelerator } from "../hotkeys";
import { useTabs } from "../tabs";
import { useUpdater } from "../composables/useUpdater";
import type { CaptureMode } from "../types";
import { isMac } from "../platform";

const route = useRoute();
const router = useRouter();
const { tabs, active, activate, close } = useTabs(router, route);
const { updateReady, availableVersion, showPrompt } = useUpdater();

const GITHUB_URL = "https://github.com/fylzero/lockshot";
const appVersion = ref("");

void getVersion()
  .then((value) => {
    appVersion.value = value;
  })
  .catch(() => {
    /* keep the bundled fallback */
  });

async function openGitHub() {
  try {
    await openUrl(GITHUB_URL);
  } catch {
    /* stay quiet if the OS cannot open the URL */
  }
}

function openChangelog() {
  void router.push("/changelog");
}

const tabList = ref<HTMLElement | null>(null);

/* On macOS the window buttons sit in the tab bar's left corner, so the bar
   leaves room for them, except in full screen, where macOS hides them. */
const fullscreen = ref(false);
let unlistenResize: (() => void) | undefined;

async function syncFullscreen() {
  try {
    fullscreen.value = await getCurrentWindow().isFullscreen();
  } catch {
    fullscreen.value = false;
  }
}

onMounted(async () => {
  if (!isMac) return;
  await syncFullscreen();
  try {
    unlistenResize = await getCurrentWindow().onResized(syncFullscreen);
  } catch {
    /* Not running in Tauri (browser dev mock). */
  }
});

onBeforeUnmount(() => unlistenResize?.());

/* A vertical wheel or swipe scrolls the tabs sideways. */
function onWheel(event: WheelEvent) {
  const el = tabList.value;
  if (!el || el.scrollWidth <= el.clientWidth) return;
  if (Math.abs(event.deltaY) > Math.abs(event.deltaX)) {
    el.scrollLeft += event.deltaY;
    event.preventDefault();
  }
}

/* Keep the active tab visible, so a new capture's tab never opens off screen. */
watch(
  () => [active.value, tabs.value.length],
  async () => {
    await nextTick();
    tabList.value?.querySelector<HTMLElement>(".tab.active")?.scrollIntoView({ block: "nearest", inline: "nearest" });
  },
);

const TAB_ICONS: Record<string, string> = { editor: "image", settings: "settings", changelog: "changelog" };

async function capture(mode: CaptureMode) {
  try {
    await api.capture(mode);
  } catch (err) {
    showToast(errorMessage(err), "error");
  }
}

const CAPTURE_ITEMS: { mode: CaptureMode; label: string }[] = [
  { mode: "area", label: "Capture area" },
  { mode: "window", label: "Capture window" },
  { mode: "scroll", label: "Scrolling capture" },
  { mode: "screen", label: "Capture screen" },
];

function shortcut(mode: CaptureMode) {
  const accelerator = settings.value?.hotkeys[mode];
  return accelerator ? formatAccelerator(accelerator, env.os) : "";
}

/* The capture modes live in a ⋮ dropdown, placed like Dispatch's, so they
   take no room from the tabs. */
const captureMenu = ref<HTMLElement | null>(null);
const menuOpen = ref(false);

function closeOnOutside(event: PointerEvent) {
  if (!captureMenu.value?.contains(event.target as Node)) closeMenu();
}

function closeOnEscape(event: KeyboardEvent) {
  if (event.key !== "Escape") return;
  event.preventDefault();
  event.stopPropagation();
  closeMenu();
}

function openMenu() {
  menuOpen.value = true;
  window.addEventListener("pointerdown", closeOnOutside, true);
  window.addEventListener("keydown", closeOnEscape, true);
  window.addEventListener("blur", closeMenu);
}

function closeMenu() {
  menuOpen.value = false;
  window.removeEventListener("pointerdown", closeOnOutside, true);
  window.removeEventListener("keydown", closeOnEscape, true);
  window.removeEventListener("blur", closeMenu);
}

onBeforeUnmount(closeMenu);

function chooseCapture(mode: CaptureMode) {
  closeMenu();
  void capture(mode);
}
</script>

<template>
  <div
    class="tab-bar"
    :class="{ 'traffic-lights': isMac && !fullscreen }"
    role="tablist"
    aria-label="Open views"
    data-tauri-drag-region
  >
    <div ref="tabList" class="tab-list" data-tauri-drag-region @wheel="onWheel">
      <div
        class="tab pinned"
        :class="{ active: active === 'home' }"
        role="tab"
        :aria-selected="active === 'home'"
        title="Home"
        aria-label="Home"
        @click="activate('home')"
      >
        <!-- The menu bar mark (scripts/icons/tray.svg), drawn in currentColor. -->
        <svg class="tab-icon brand" viewBox="0 0 64 64" aria-hidden="true">
          <g fill="none" stroke="currentColor" stroke-width="6" stroke-linecap="round" stroke-linejoin="round">
            <path d="M8 22V12a4 4 0 0 1 4-4h10" />
            <path d="M42 8h10a4 4 0 0 1 4 4v10" />
            <path d="M56 42v10a4 4 0 0 1-4 4H42" />
            <path d="M22 56H12a4 4 0 0 1-4-4V42" />
          </g>
          <circle cx="32" cy="32" r="7" fill="currentColor" />
        </svg>
      </div>
      <div
        v-for="tab in tabs"
        :key="tab.id"
        class="tab"
        :class="{ active: active === tab.id }"
        role="tab"
        :aria-selected="active === tab.id"
        @click="activate(tab.id)"
        @auxclick.middle="close(tab.id)"
      >
        <Icon :name="TAB_ICONS[tab.kind]" class="tab-icon" />
        <span class="tab-title">{{ tab.title }}</span>
        <button class="tab-close" type="button" :aria-label="`Close ${tab.title}`" @click.stop="close(tab.id)">×</button>
      </div>
    </div>
    <div class="tab-tools">
      <button
        v-if="updateReady"
        class="tab-tool update"
        type="button"
        :title="availableVersion ? `Update to v${availableVersion}` : 'Update available'"
        :aria-label="availableVersion ? `Update to v${availableVersion}` : 'Update available'"
        @click="showPrompt"
      >
        <Icon name="update" />
      </button>
      <div ref="captureMenu" class="tab-menu">
        <button
          class="tab-tool"
          :class="{ active: menuOpen }"
          type="button"
          title="Capture"
          aria-label="Capture"
          aria-haspopup="menu"
          :aria-expanded="menuOpen"
          @click="menuOpen ? closeMenu() : openMenu()"
        >
          <svg class="tab-tool-dots" viewBox="0 0 24 24" aria-hidden="true">
            <circle cx="12" cy="5.5" r="1.6" />
            <circle cx="12" cy="12" r="1.6" />
            <circle cx="12" cy="18.5" r="1.6" />
          </svg>
        </button>
        <div v-if="menuOpen" class="menu-pop tab-menu-pop" role="menu">
          <button
            v-for="item in CAPTURE_ITEMS"
            :key="item.mode"
            class="menu-item"
            type="button"
            role="menuitem"
            :disabled="!screenAllowed"
            :title="screenAllowed ? undefined : 'Allow Screen Recording to capture'"
            @click="chooseCapture(item.mode)"
          >
            <Icon :name="item.mode" />
            <span>{{ item.label }}</span>
            <kbd v-if="shortcut(item.mode)">{{ shortcut(item.mode) }}</kbd>
          </button>
        </div>
      </div>
      <button
        class="tab-version"
        :class="{ active: route.path === '/changelog' }"
        type="button"
        title="Open the change log"
        @click="openChangelog"
      >
        <span class="tab-version-prefix">v</span>{{ appVersion }}
      </button>
      <button
        class="tab-tool"
        type="button"
        title="Open Lockshot on GitHub"
        aria-label="Open Lockshot on GitHub"
        @click="openGitHub"
      >
        <svg class="tab-tool-github" viewBox="0 0 16 16" aria-hidden="true">
          <path
            d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27s1.36.09 2 .27c1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0 0 16 8c0-4.42-3.58-8-8-8"
          />
        </svg>
      </button>
      <button
        class="tab-tool"
        :class="{ active: active === 'settings' }"
        type="button"
        title="Settings"
        aria-label="Settings"
        @click="activate('settings')"
      >
        <Icon name="settings" />
      </button>
    </div>
  </div>
</template>
