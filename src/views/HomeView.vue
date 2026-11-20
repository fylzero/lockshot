<script setup lang="ts">
import { onMounted } from "vue";
import Icon from "../components/Icon.vue";
import PermissionCard from "../components/PermissionCard.vue";
import HotkeyRecorder from "../components/HotkeyRecorder.vue";
import { api, errorMessage } from "../api";
import { env, openImageFile, saveSettingsSoon, settings, showToast } from "../store";
import { acceleratorKeys } from "../hotkeys";
import type { HotkeyAction } from "../types";

const trayName = env.os === "macos" ? "menu bar" : "system tray";

async function capture(mode: "area" | "screen") {
  try {
    await api.capture(mode);
  } catch (err) {
    showToast(errorMessage(err), "error");
  }
}

async function setHotkey(action: HotkeyAction, accelerator: string) {
  try {
    const saved = await api.setHotkey(action, accelerator);
    if (settings.value) settings.value.hotkeys = saved.hotkeys;
  } catch (err) {
    showToast(errorMessage(err), "error");
  }
}

/* Seeing Home once is the onboarding: later launches start in the tray. */
onMounted(() => {
  if (settings.value && !settings.value.onboarded) {
    settings.value.onboarded = true;
    saveSettingsSoon();
  }
});
</script>

<template>
  <div class="home">
    <div v-if="settings" class="home-inner">
      <div class="home-hero">
        <img src="/app-icon.png" alt="Lockshot" />
        <p class="muted">Capture from anywhere with a hotkey or the Lockshot icon in the {{ trayName }}.</p>
      </div>

      <div class="capture-cards">
        <button class="capture-card" @click="capture('area')">
          <Icon name="area" />
          <strong>Capture area</strong>
          <span v-if="settings.hotkeys.area" class="capture-keys">
            <kbd v-for="(k, i) in acceleratorKeys(settings.hotkeys.area, env.os)" :key="i">{{ k }}</kbd>
          </span>
        </button>
        <button class="capture-card" @click="capture('screen')">
          <Icon name="screen" />
          <strong>Capture screen</strong>
          <span v-if="settings.hotkeys.screen" class="capture-keys">
            <kbd v-for="(k, i) in acceleratorKeys(settings.hotkeys.screen, env.os)" :key="i">{{ k }}</kbd>
          </span>
        </button>
        <button class="capture-card" @click="openImageFile">
          <Icon name="folder" />
          <strong>Open image…</strong>
          <span class="muted capture-keys">or drop one here</span>
        </button>
      </div>

      <div class="card">
        <div class="welcome-step">
          <span class="step-num">1</span>
          <div style="flex: 1">
            <h3>Let Lockshot see your screen</h3>
            <PermissionCard />
          </div>
        </div>
        <div class="welcome-step">
          <span class="step-num">2</span>
          <div style="flex: 1">
            <h3>Pick your hotkeys</h3>
            <p class="muted" style="margin-bottom: 0.6rem">Click one and press a new combination. They work from any app.</p>
            <div class="home-hotkeys">
              <span>Capture area</span>
              <HotkeyRecorder :value="settings.hotkeys.area" label="Capture area" @change="setHotkey('area', $event)" @invalid="showToast($event, 'error')" />
              <span>Capture screen</span>
              <HotkeyRecorder :value="settings.hotkeys.screen" label="Capture screen" @change="setHotkey('screen', $event)" @invalid="showToast($event, 'error')" />
            </div>
            <p class="muted" style="margin: 0.6rem 0 0; font-size: 0.85rem">
              Want {{ env.os === "macos" ? "⌘⇧4" : "PrtSc" }} instead?
              <RouterLink to="/settings/hotkeys">See how to take over the system keys.</RouterLink>
            </p>
          </div>
        </div>
        <div class="welcome-step">
          <span class="step-num">3</span>
          <div>
            <h3>Find Lockshot in the {{ trayName }}</h3>
            <p class="muted">
              Lockshot runs quietly there. Closing this window keeps it running; quit from the icon's menu.
              <template v-if="env.os === 'linux'"> On GNOME, tray icons need the AppIndicator extension.</template>
            </p>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>
