<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from "vue";
import Icon from "./Icon.vue";
import { api, errorMessage } from "../api";
import { showToast } from "../store";
import type { PermissionStatus } from "../types";

/*
 * Scrolling Capture moves the pointer and sends scroll-wheel events to the
 * window being captured. macOS only allows that once Lockshot is turned on
 * under Accessibility; Wayland does not allow it at all.
 */
const status = ref<PermissionStatus | null>(null);
let poll: ReturnType<typeof setInterval> | undefined;

async function refresh() {
  try {
    status.value = await api.permissionStatus();
  } catch {
    /* keep the last answer */
  }
}

async function grant() {
  status.value = await api.requestInputPermission();
  if (status.value.input !== "granted") await openSettings();
}

async function openSettings() {
  try {
    await api.openSystemSettings("accessibility");
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
    <div v-if="status.input === 'granted'" class="notice good">
      <Icon name="scroll" />
      <div>
        <strong>Accessibility is allowed.</strong>
        <p class="muted">Scrolling capture can scroll other apps' windows.</p>
      </div>
    </div>

    <div v-else-if="status.input === 'denied'" class="notice">
      <Icon name="scroll" />
      <div>
        <strong>Allow Accessibility for scrolling capture</strong>
        <p class="muted">
          Scrolling capture scrolls the window for you, which macOS only allows for apps turned on under
          Accessibility. The other captures don't need it.
        </p>
        <ol class="steps">
          <li>Click <strong>Allow access</strong>. macOS shows a prompt the first time, or opens System Settings.</li>
          <li>Under <em>Privacy &amp; Security › Accessibility</em>, turn on <strong>Lockshot</strong>.</li>
        </ol>
        <div class="notice-actions">
          <button class="primary" @click="grant"><Icon name="shield" /> Allow access</button>
          <button @click="openSettings"><Icon name="external" /> Open System Settings</button>
        </div>
      </div>
    </div>

    <div v-else-if="status.input === 'unsupported'" class="notice">
      <Icon name="scroll" />
      <div>
        <strong>Scrolling capture needs X11</strong>
        <p class="muted">Wayland doesn't let apps scroll other apps' windows. Log in to an X11 session to use it.</p>
      </div>
    </div>
  </template>
</template>
