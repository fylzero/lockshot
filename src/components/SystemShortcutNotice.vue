<script setup lang="ts">
import { computed, ref } from "vue";
import Icon from "./Icon.vue";
import { api, errorMessage } from "../api";
import { env, refreshSystemShortcuts, settings, showToast, systemShortcuts } from "../store";
import { formatAccelerator, HOTKEY_ACTIONS, systemClashes } from "../hotkeys";

/*
 * Shown wherever hotkeys are picked when one of them is still owned by the
 * OS's own screenshot shortcuts: macOS's, or the Snipping Tool's PrtSc on
 * Windows. On macOS: macOS accepts the registration but its
 * own shortcut wins, so without this the key just opens the system tool and
 * Lockshot looks broken. It walks through freeing the key, and disappears
 * on its own once the shortcut is off (the list refreshes when Lockshot comes
 * back to the front).
 */
const clashes = computed(() => systemClashes(settings.value?.hotkeys, systemShortcuts.value));
const keys = computed(() => clashes.value.map((c) => formatAccelerator(c.accelerator, env.os)));
const checking = ref(false);
const confirming = ref(false);
const busy = ref(false);

function actionLabel(id: string) {
  return HOTKEY_ACTIONS.find((a) => a.id === id)?.label ?? id;
}

const windows = computed(() => env.os === "windows");

async function openShortcuts() {
  try {
    await api.openSystemSettings("keyboard-shortcuts");
  } catch (err) {
    showToast(errorMessage(err), "error");
  }
}

/*
 * With the user's go-ahead, switch off just the macOS shortcuts that clash,
 * keeping everything else as it was, and offer Undo straight away.
 */
async function turnOff() {
  const ids = clashes.value.map((c) => c.system.id);
  busy.value = true;
  try {
    systemShortcuts.value = await api.setSystemScreenshotShortcuts(ids, false);
    await refreshSystemShortcuts(true);
    confirming.value = false;
    if (clashes.value.length) {
      showToast("macOS didn't take the change. Use Open Keyboard Shortcuts to turn them off instead.", "error");
    } else {
      showToast("Done: macOS's shortcuts are off and Lockshot has those keys.", "info", {
        label: "Undo",
        run: () => void turnBackOn(ids),
      });
    }
  } catch (err) {
    showToast(errorMessage(err), "error");
  } finally {
    busy.value = false;
  }
}

async function turnBackOn(ids: string[]) {
  try {
    systemShortcuts.value = await api.setSystemScreenshotShortcuts(ids, true);
    showToast("macOS's screenshot shortcuts are back on.");
  } catch (err) {
    showToast(errorMessage(err), "error");
  }
}

async function checkAgain() {
  checking.value = true;
  await refreshSystemShortcuts();
  checking.value = false;
  if (!clashes.value.length) showToast("All set: Lockshot has those keys now.");
}
</script>

<template>
  <div v-if="clashes.length && windows" class="notice system-clash" role="alert">
    <Icon name="info" />
    <div>
      <strong><kbd>{{ keys[0] }}</kbd> may open the Snipping Tool, not Lockshot</strong>
      <p class="muted">Windows hands the Print Screen key to the Snipping Tool while this setting is on. To give it to Lockshot:</p>
      <ol class="steps">
        <li>Click <strong>Open keyboard settings</strong> below (Settings › Accessibility › Keyboard).</li>
        <li>Turn off <strong>{{ clashes[0].system.label }}</strong>.</li>
        <li>If OneDrive or Dropbox also claims <kbd>PrtSc</kbd>, turn that off in their settings too.</li>
      </ol>
      <p class="muted">This message goes away once it's off. If <kbd>PrtSc</kbd> still opens the Snipping Tool, sign out and back in.</p>
      <div class="notice-actions">
        <button class="primary" @click="openShortcuts"><Icon name="external" /> Open keyboard settings</button>
        <button :disabled="checking" @click="checkAgain">Check again</button>
      </div>
    </div>
  </div>
  <div v-else-if="clashes.length" class="notice system-clash" role="alert">
    <Icon name="info" />
    <div>
      <strong>
        <template v-if="keys.length === 1"><kbd>{{ keys[0] }}</kbd> still opens</template>
        <template v-else><kbd v-for="k in keys" :key="k">{{ k }}</kbd> still open</template>
        macOS's screenshot tool, not Lockshot
      </strong>
      <p class="muted">macOS keeps its own screenshot shortcuts on, and they win over Lockshot's. Lockshot can turn them off for you, or you can do it yourself:</p>
      <ol class="steps">
        <li>Click <strong>Open Keyboard Shortcuts</strong> below.</li>
        <li>In the window that opens, click <strong>Screenshots</strong> in the list on the left.</li>
        <li>
          Untick these, then click <strong>Done</strong>:
          <ul class="clash-list">
            <li v-for="(c, i) in clashes" :key="c.action">
              <strong>{{ c.system.label }}</strong> <kbd>{{ keys[i] }}</kbd>
              <span class="muted">· your {{ actionLabel(c.action) }} key</span>
            </li>
          </ul>
        </li>
      </ol>
      <p class="muted">
        Seeing the main Keyboard page instead? Click <strong>Keyboard Shortcuts…</strong> on it first.
        This message goes away as soon as they're off. No restart needed.
      </p>
      <div v-if="confirming" class="notice-confirm">
        <p>
          Lockshot will turn off
          <template v-for="(c, i) in clashes" :key="c.action"><strong>{{ c.system.label }}</strong><template v-if="i < clashes.length - 1"> and </template></template>
          in macOS. Nothing else changes, and you can undo it right after or in System Settings any time.
        </p>
        <div class="notice-actions">
          <button class="primary" :disabled="busy" @click="turnOff">{{ busy ? "Turning off…" : "Turn them off" }}</button>
          <button :disabled="busy" @click="confirming = false">Cancel</button>
        </div>
      </div>
      <div v-else class="notice-actions">
        <button class="primary" @click="confirming = true">Turn them off for me</button>
        <button @click="openShortcuts"><Icon name="external" /> Open Keyboard Shortcuts</button>
        <button :disabled="checking" @click="checkAgain">Check again</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.clash-list {
  margin: 0.25rem 0 0.1rem;
  padding-left: 1.1rem;
  list-style: disc;
}

.clash-list li + li {
  margin-top: 0.15rem;
}

.notice-confirm {
  margin-top: 0.6rem;
  padding: 0.6rem 0.75rem;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--bg-raised);
}

.notice-confirm p {
  margin: 0;
}
</style>
