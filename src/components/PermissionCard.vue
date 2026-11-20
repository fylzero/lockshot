<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from "vue";
import { relaunch } from "@tauri-apps/plugin-process";
import Icon from "./Icon.vue";
import { api, errorMessage } from "../api";
import { showToast } from "../store";
import type { PermissionStatus } from "../types";

/* Embedded: the parent card already titles the step, so drop the warning
   box, icon and heading and show only the instructions and buttons. */
const props = defineProps<{ embedded?: boolean }>();
const status = ref<PermissionStatus | null>(null);
const emit = defineEmits<{ status: [status: PermissionStatus] }>();
/* Lets Home move past the permission step once macOS reports it allowed. */
watch(status, (next) => {
  if (next) emit("status", next);
});
const asked = ref(false);
let poll: ReturnType<typeof setInterval> | undefined;

async function refresh() {
  try {
    status.value = await api.permissionStatus();
  } catch {
    /* keep the last answer */
  }
}

async function grant() {
  asked.value = true;
  status.value = await api.requestScreenPermission();
  if (status.value.screen !== "granted") {
    /* macOS only prompts once; after that the switch lives in System Settings. */
    await openSettings();
  }
}

async function openSettings() {
  try {
    await api.openSystemSettings("screen-recording");
  } catch (err) {
    showToast(errorMessage(err), "error");
  }
}

onMounted(() => {
  void refresh();
  /* Pick up the switch being flipped in System Settings without a click. */
  poll = setInterval(refresh, 2000);
});
onBeforeUnmount(() => clearInterval(poll));
</script>

<template>
  <template v-if="status">
    <div v-if="status.os === 'macos' && status.screen === 'granted'" class="notice good">
      <Icon name="shield" />
      <div>
        <strong>Screen Recording is allowed.</strong>
        <p class="muted">Lockshot can see every app's windows when it captures.</p>
      </div>
    </div>

    <div v-else-if="status.os === 'macos' && props.embedded" class="permission-steps">
      <p class="muted">macOS hides other apps' windows from screenshots until you allow it. It's a one-time switch.</p>
      <ol class="steps">
        <li>Click <strong>Allow access</strong>. macOS shows a prompt the first time, or opens System Settings.</li>
        <li>
          Under <em>Privacy &amp; Security › Screen &amp; System Audio Recording</em>, turn on <strong>Lockshot</strong>.
          <button class="link" @click="openSettings">Open System Settings</button>
        </li>
        <li>macOS applies the change after a relaunch, so click <strong>Relaunch Lockshot</strong>.</li>
      </ol>
      <div class="step-actions">
        <button class="primary" @click="grant"><Icon name="shield" /> Allow access</button>
        <button :class="{ primary: asked }" @click="relaunch()"><Icon name="reset" /> Relaunch Lockshot</button>
      </div>
    </div>

    <div v-else-if="status.os === 'macos'" class="notice">
      <Icon name="shield" />
      <div>
        <strong>Allow Screen Recording</strong>
        <p class="muted">
          macOS hides other apps' windows from screenshots until you allow it. It's a one-time switch.
        </p>
        <ol class="steps">
          <li>Click <strong>Allow access</strong>. macOS shows a prompt the first time, or opens System Settings.</li>
          <li>Under <em>Privacy &amp; Security › Screen &amp; System Audio Recording</em>, turn on <strong>Lockshot</strong>.</li>
          <li>macOS applies the change after a relaunch, so click <strong>Relaunch Lockshot</strong>.</li>
        </ol>
        <div class="notice-actions">
          <button class="primary" @click="grant"><Icon name="shield" /> Allow access</button>
          <button @click="openSettings"><Icon name="external" /> Open System Settings</button>
          <button :class="{ primary: asked }" @click="relaunch()"><Icon name="reset" /> Relaunch Lockshot</button>
        </div>
      </div>
    </div>

    <div v-else-if="status.os === 'windows'" class="notice good">
      <Icon name="shield" />
      <div>
        <strong>No permission needed.</strong>
        <p class="muted">Windows lets Lockshot capture any screen right away.</p>
      </div>
    </div>

    <div v-else class="notice" :class="{ good: !status.wayland }">
      <Icon name="shield" />
      <div>
        <strong>{{ status.wayland ? "Wayland session" : "No permission needed" }}</strong>
        <p v-if="status.wayland" class="muted">
          Your desktop asks before an app can capture the screen. Approve the screenshot prompt the first
          time; some desktops let you remember the choice. Wayland also stops apps from grabbing global
          hotkeys, so bind <code>lockshot --area</code> and <code>lockshot --screen</code> to keys in your
          desktop's keyboard settings instead.
        </p>
        <p v-else class="muted">X11 lets Lockshot capture any screen right away.</p>
      </div>
    </div>
  </template>
</template>
