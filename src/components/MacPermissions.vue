<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { relaunch } from "@tauri-apps/plugin-process";
import Icon from "./Icon.vue";
import { api, errorMessage } from "../api";
import { screenAllowed, showToast } from "../store";
import type { PermissionStatus } from "../types";

/*
 * macOS setup: Screen Recording and Accessibility as two stacked blocks
 * in one step, so both switches get flipped in one trip to System
 * Settings. Screen Recording is required and takes a relaunch; Accessibility
 * is only for Scrolling Capture and applies right away.
 *
 * `onlyMissing` limits it to the blocks still turned off (Home after setup, when
 * access was revoked).
 */
const props = defineProps<{ onlyMissing?: boolean; hideInput?: boolean }>();
/* dismissInput: the Accessibility block's Dismiss button, since it's optional. */
const emit = defineEmits<{ status: [status: PermissionStatus]; dismissInput: [] }>();

const status = ref<PermissionStatus | null>(null);
watch(status, (next) => {
  if (!next) return;
  screenAllowed.value = next.screen !== "denied";
  emit("status", next);
});
const askedScreen = ref(false);
let poll: ReturnType<typeof setInterval> | undefined;

async function refresh() {
  try {
    status.value = await api.permissionStatus();
  } catch {
    /* keep the last answer */
  }
}

async function openSettings(target: "screen-recording" | "accessibility") {
  try {
    await api.openSystemSettings(target);
  } catch (err) {
    showToast(errorMessage(err), "error");
  }
}

async function grantScreen() {
  askedScreen.value = true;
  status.value = await api.requestScreenPermission();
  /* macOS only prompts once; after that the switch lives in System Settings. */
  if (status.value.screen !== "granted") await openSettings("screen-recording");
}

async function grantInput() {
  status.value = await api.requestInputPermission();
  if (status.value.input !== "granted") await openSettings("accessibility");
}

const screenOk = computed(() => status.value?.screen !== "denied");
const inputOk = computed(() => status.value?.input !== "denied");
const showScreen = computed(() => !props.onlyMissing || !screenOk.value);
const showInput = computed(() => !props.hideInput && (!props.onlyMissing || !inputOk.value));

onMounted(() => {
  void refresh();
  /* Pick up the switches being flipped in System Settings without a click. */
  poll = setInterval(refresh, 2000);
});
onBeforeUnmount(() => clearInterval(poll));
</script>

<template>
  <div v-if="status" class="perm-blocks">
    <section v-if="showScreen" class="perm-block" :class="{ ok: screenOk }">
      <div class="perm-head">
        <span class="perm-icon"><Icon name="shield" /></span>
        <div class="perm-title">
          <strong>Screen Recording</strong>
          <span v-if="!screenOk" class="perm-tag">Required</span>
        </div>
        <span v-if="screenOk" class="status-pill good"><Icon name="check" /> Allowed</span>
      </div>
      <p class="perm-desc">Lets Lockshot see other apps' windows. Without it, captures show only the desktop.</p>
      <template v-if="!screenOk">
        <ol class="perm-steps">
          <li>Click <strong>Allow access</strong>, then turn on <strong>Lockshot</strong> in the <button class="link" @click="openSettings('screen-recording')">System Settings</button> list.</li>
          <li>Relaunch so macOS applies it.</li>
        </ol>
        <div class="perm-actions">
          <button class="primary" @click="grantScreen"><Icon name="shield" /> Allow access</button>
          <button :class="{ primary: askedScreen }" @click="relaunch()"><Icon name="reset" /> Relaunch</button>
        </div>
      </template>
    </section>

    <section v-if="showInput" class="perm-block" :class="{ ok: inputOk }">
      <div class="perm-head">
        <span class="perm-icon"><Icon name="scroll" /></span>
        <div class="perm-title">
          <strong>Accessibility</strong>
          <span v-if="!inputOk" class="perm-tag">Optional</span>
        </div>
        <span v-if="inputOk" class="status-pill good"><Icon name="check" /> Allowed</span>
      </div>
      <p class="perm-desc">Lets Scrolling Capture scroll the window for you. The other captures don't need it.</p>
      <template v-if="!inputOk">
        <ol class="perm-steps">
          <li>Click <strong>Allow access</strong>, then turn on <strong>Lockshot</strong> in the <button class="link" @click="openSettings('accessibility')">System Settings</button> list.</li>
          <li>That's it: no relaunch needed.</li>
        </ol>
        <div class="perm-actions">
          <button class="primary" @click="grantInput"><Icon name="scroll" /> Allow access</button>
          <button @click="emit('dismissInput')">Dismiss</button>
        </div>
      </template>
    </section>
  </div>
</template>
