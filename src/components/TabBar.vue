<script setup lang="ts">
import { nextTick, ref, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
import Icon from "./Icon.vue";
import { api, errorMessage } from "../api";
import { env, settings, showToast } from "../store";
import { formatAccelerator } from "../hotkeys";
import { useTabs } from "../tabs";
import { useUpdater } from "../composables/useUpdater";

const route = useRoute();
const router = useRouter();
const { tabs, active, activate, close } = useTabs(router, route);
const { updateReady, availableVersion, showPrompt } = useUpdater();

const tabList = ref<HTMLElement | null>(null);

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

async function capture(mode: "area" | "screen") {
  try {
    await api.capture(mode);
  } catch (err) {
    showToast(errorMessage(err), "error");
  }
}

function hint(label: string, accelerator?: string) {
  return accelerator ? `${label} (${formatAccelerator(accelerator, env.os)})` : label;
}
</script>

<template>
  <div class="tab-bar" role="tablist" aria-label="Open views">
    <div ref="tabList" class="tab-list" @wheel="onWheel">
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
      <button
        class="tab-tool"
        type="button"
        :title="hint('Capture area', settings?.hotkeys.area)"
        aria-label="Capture area"
        @click="capture('area')"
      >
        <Icon name="area" />
      </button>
      <button
        class="tab-tool"
        type="button"
        :title="hint('Capture screen', settings?.hotkeys.screen)"
        aria-label="Capture screen"
        @click="capture('screen')"
      >
        <Icon name="screen" />
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
