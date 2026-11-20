<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import Icon from "../components/Icon.vue";
import PermissionCard from "../components/PermissionCard.vue";
import HotkeyRecorder from "../components/HotkeyRecorder.vue";
import SystemShortcutNotice from "../components/SystemShortcutNotice.vue";
import { api, errorMessage } from "../api";
import { env, openImageFile, saveSettingsSoon, settings, showToast } from "../store";
import { acceleratorKeys, pcKeyboardPreset, systemPreset } from "../hotkeys";
import type { CaptureMode, HotkeyAction, PermissionStatus } from "../types";

/*
 * Home walks through setup one step at a time: Screen Recording, then
 * hotkeys, then where to find the menu bar icon. Only once that's done does
 * it show the capture buttons. Progress lives in settings, so relaunching
 * for the macOS permission picks up at the next step.
 */
type Step = "permission" | "hotkeys" | "tray" | "done";
const STEPS: Exclude<Step, "done">[] = ["permission", "hotkeys", "tray"];

const trayName = env.os === "macos" ? "menu bar" : "system tray";

const step = computed<Step>(() => {
  const s = settings.value;
  if (!s || s.onboarded) return "done";
  return (STEPS as string[]).includes(s.setupStep) ? (s.setupStep as Step) : "permission";
});
const stepNumber = computed(() => STEPS.indexOf(step.value as Exclude<Step, "done">) + 1);

function goTo(next: Step) {
  if (!settings.value) return;
  if (next === "done") {
    settings.value.onboarded = true;
    settings.value.setupStep = "";
  } else {
    settings.value.setupStep = next;
  }
  saveSettingsSoon();
}

/* Screen Recording counts as done once macOS reports it allowed (after the
   relaunch it asks for), or right away where there's nothing to allow. */
function onPermission(status: PermissionStatus) {
  permission.value = status;
  if (step.value === "permission" && status.screen !== "denied") goTo("hotkeys");
}

/* After setup, the permission card comes back only if access is revoked. */
const permission = ref<PermissionStatus | null>(null);
onMounted(async () => {
  permission.value = await api.permissionStatus().catch(() => null);
  if (permission.value) onPermission(permission.value);
});

async function capture(mode: CaptureMode) {
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
    return true;
  } catch (err) {
    showToast(errorMessage(err), "error");
    return false;
  }
}

const preset = computed(() => systemPreset(env.os));
const pcPreset = computed(() => pcKeyboardPreset(env.os));

/* One click to the system screenshot keys; if macOS still owns them, the
   notice below appears with the steps to free them. */
async function usePreset(which = preset.value) {
  if (!which) return;
  for (const [action, accelerator] of Object.entries(which.hotkeys)) {
    if (!(await setHotkey(action as HotkeyAction, accelerator))) return;
  }
}
</script>

<template>
  <div class="home">
    <div v-if="settings" class="home-inner">
      <div class="home-hero">
        <img src="/app-icon.png" alt="Lockshot" />
        <p v-if="step === 'done'" class="muted">
          Capture from anywhere with a hotkey or the Lockshot icon in the {{ trayName }}.
        </p>
        <p v-else class="muted">A few quick things to set up. Step {{ stepNumber }} of {{ STEPS.length }}.</p>
      </div>

      <!-- Setup, one step at a time -->
      <div v-if="step === 'permission'" class="card">
        <div class="welcome-step">
          <span class="step-num">1</span>
          <div style="flex: 1">
            <h3>Allow Screen Recording</h3>
            <PermissionCard embedded @status="onPermission" />
          </div>
        </div>
      </div>

      <div v-else-if="step === 'hotkeys'" class="card">
        <div class="welcome-step">
          <span class="step-num">2</span>
          <div style="flex: 1">
            <h3>Pick your hotkeys</h3>
            <p class="muted" style="margin-bottom: 0.6rem">
              Click one and press a new combination, or keep the defaults. They work from any app.
            </p>
            <div class="home-hotkeys">
              <span>Capture area</span>
              <HotkeyRecorder :value="settings.hotkeys.area" label="Capture area" @change="setHotkey('area', $event)" @invalid="showToast($event, 'error')" />
              <span>Capture window</span>
              <HotkeyRecorder :value="settings.hotkeys.window" label="Capture window" @change="setHotkey('window', $event)" @invalid="showToast($event, 'error')" />
              <span>Capture screen</span>
              <HotkeyRecorder :value="settings.hotkeys.screen" label="Capture screen" @change="setHotkey('screen', $event)" @invalid="showToast($event, 'error')" />
            </div>
            <p class="muted" style="margin: 0.6rem 0 0; font-size: 0.85rem">
              Want {{ env.os === "macos" ? "⌘⇧4" : "PrtSc" }} instead?
              <button v-if="env.os === 'macos' && preset" class="link" @click="usePreset(preset)">Use the ⌘⇧ keys for Lockshot.</button>
              <RouterLink v-else to="/settings/hotkeys">See how to take over the system keys.</RouterLink>
            </p>
            <p v-if="pcPreset" class="muted" style="margin: 0.3rem 0 0; font-size: 0.85rem">
              On a PC keyboard?
              <button class="link" @click="usePreset(pcPreset)">Use PrtSc and Shift+PrtSc.</button>
            </p>
            <SystemShortcutNotice style="margin: 0.8rem 0 0" />
            <p v-if="env.os === 'linux'" class="muted" style="margin: 0.6rem 0 0; font-size: 0.85rem">
              On Wayland, bind <code>lockshot --area</code> and <code>lockshot --screen</code> in your desktop's keyboard settings instead.
            </p>
            <div class="step-actions">
              <button class="primary" @click="goTo('tray')"><Icon name="check" /> Use these hotkeys</button>
            </div>
          </div>
        </div>
      </div>

      <div v-else-if="step === 'tray'" class="card">
        <div class="welcome-step">
          <span class="step-num">3</span>
          <div style="flex: 1">
            <h3>Find Lockshot in the {{ trayName }}</h3>
            <p class="muted">
              Lockshot runs quietly there. Closing this window keeps it running; quit from the icon's menu.
              <template v-if="env.os === 'linux'"> On GNOME, tray icons need the AppIndicator extension.</template>
            </p>
            <div class="step-actions">
              <button class="primary" @click="goTo('done')"><Icon name="check" /> Got it</button>
            </div>
          </div>
          <button class="ghost step-close" title="Dismiss" aria-label="Dismiss" @click="goTo('done')">
            <Icon name="close" />
          </button>
        </div>
      </div>

      <!-- After setup -->
      <template v-else>
        <div v-if="permission?.screen === 'denied'" class="card">
          <div class="welcome-step">
            <div style="flex: 1">
              <h3>Screen Recording is turned off</h3>
              <PermissionCard embedded @status="onPermission" />
            </div>
          </div>
        </div>

        <SystemShortcutNotice />

        <div class="capture-cards">
          <button class="capture-card" @click="capture('area')">
            <Icon name="area" />
            <strong>Capture area</strong>
            <span v-if="settings.hotkeys.area" class="capture-keys">
              <kbd v-for="(k, i) in acceleratorKeys(settings.hotkeys.area, env.os)" :key="i">{{ k }}</kbd>
            </span>
          </button>
          <button class="capture-card" @click="capture('window')">
            <Icon name="window" />
            <strong>Capture window</strong>
            <span v-if="settings.hotkeys.window" class="capture-keys">
              <kbd v-for="(k, i) in acceleratorKeys(settings.hotkeys.window, env.os)" :key="i">{{ k }}</kbd>
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
      </template>
    </div>
  </div>
</template>
