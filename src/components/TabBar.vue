<script setup lang="ts">
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
    <div class="tab-list">
      <div
        class="tab pinned"
        :class="{ active: active === 'home' }"
        role="tab"
        :aria-selected="active === 'home'"
        @click="activate('home')"
      >
        <Icon name="home" class="tab-icon" />
        <span class="tab-title">Home</span>
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
        <Icon :name="TAB_ICONS[tab.id]" class="tab-icon" />
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
