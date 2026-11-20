<script setup lang="ts">
import { onMounted } from "vue";
import { CHANGELOG } from "../changelog";
import { useUpdater } from "../composables/useUpdater";
import { showToast } from "../store";

const { status, statusText, currentVersion, availableVersion, busy, updateReady, ensureCurrentVersion, checkForUpdates, showPrompt } =
  useUpdater();

onMounted(() => {
  void ensureCurrentVersion();
});

async function onCheckForUpdates() {
  if (updateReady.value) {
    showPrompt();
    return;
  }
  await checkForUpdates({ prompt: true });
  if (status.value === "up-to-date") showToast("You're on the latest version.");
  else if (status.value === "error") showToast(statusText.value, "error");
}
</script>

<template>
  <div class="changelog-page">
    <div class="changelog-inner">
      <div class="changelog-header">
        <div>
          <h2>Change Log</h2>
          <p class="muted">What shipped in each Lockshot release.</p>
        </div>
        <button class="ghost" type="button" :disabled="busy" @click="onCheckForUpdates">
          <template v-if="status === 'checking'">Checking…</template>
          <template v-else-if="updateReady && availableVersion">Update to v{{ availableVersion }}</template>
          <template v-else>Check for Updates</template>
        </button>
      </div>
      <article v-for="release in CHANGELOG" :key="release.version" class="changelog-release">
        <header class="changelog-release-head">
          <h3 class="changelog-version">{{ release.version }}</h3>
          <span v-if="release.version === currentVersion" class="changelog-current">Current</span>
          <span class="muted changelog-date">{{ release.date }}</span>
        </header>
        <ul class="changelog-notes">
          <li v-for="note in release.notes" :key="note">{{ note }}</li>
        </ul>
      </article>
    </div>
  </div>
</template>
