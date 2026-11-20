<script setup lang="ts">
import { onBeforeUnmount, ref } from "vue";
import { api } from "../api";
import { acceleratorFromEvent, acceleratorKeys } from "../hotkeys";
import { env } from "../store";

const props = defineProps<{ value: string; label: string }>();
const emit = defineEmits<{ change: [accelerator: string]; invalid: [message: string] }>();

const recording = ref(false);
const preview = ref("");
const button = ref<HTMLButtonElement | null>(null);

function start() {
  if (recording.value) return;
  recording.value = true;
  preview.value = "";
  /* The current combo must not fire a capture while it is being replaced. */
  void api.pauseHotkeys(true);
  window.addEventListener("keydown", onKey, true);
  window.addEventListener("keyup", onKeyUp, true);
}

function stop() {
  if (!recording.value) return;
  recording.value = false;
  preview.value = "";
  void api.pauseHotkeys(false);
  window.removeEventListener("keydown", onKey, true);
  window.removeEventListener("keyup", onKeyUp, true);
}

function handle(event: KeyboardEvent) {
  event.preventDefault();
  event.stopPropagation();
  const plain = !event.ctrlKey && !event.altKey && !event.metaKey && !event.shiftKey;
  if (plain && event.key === "Escape") {
    stop();
    return;
  }
  if (plain && (event.key === "Backspace" || event.key === "Delete")) {
    stop();
    emit("change", "");
    return;
  }
  const result = acceleratorFromEvent(event);
  if (result.kind === "pending") {
    preview.value = result.preview;
  } else if (result.kind === "invalid") {
    emit("invalid", result.message);
  } else {
    stop();
    emit("change", result.accelerator);
    button.value?.blur();
  }
}

function onKey(event: KeyboardEvent) {
  handle(event);
}

/* Windows delivers PrintScreen only on key up. */
function onKeyUp(event: KeyboardEvent) {
  if (event.code === "PrintScreen") handle(event);
  else if (recording.value) {
    const result = acceleratorFromEvent(event);
    if (result.kind === "pending") preview.value = result.preview;
  }
}

onBeforeUnmount(stop);
</script>

<template>
  <button
    ref="button"
    class="hotkey"
    :class="{ recording, unset: !value && !recording }"
    :aria-label="`${props.label} shortcut`"
    :title="recording ? 'Press the new shortcut · Esc cancels · Delete clears' : 'Click to record a new shortcut'"
    @click="start"
    @blur="stop"
  >
    <template v-if="recording">
      <span>{{ preview || "Press keys…" }}</span>
    </template>
    <template v-else-if="value">
      <kbd v-for="(key, i) in acceleratorKeys(value, env.os)" :key="i">{{ key }}</kbd>
    </template>
    <template v-else>Not set</template>
  </button>
</template>
