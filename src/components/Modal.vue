<script setup lang="ts">
import { onMounted, onUnmounted } from "vue";

defineProps<{ title: string }>();
const emit = defineEmits<{ close: [] }>();

function onKeydown(event: KeyboardEvent) {
  if (event.key === "Escape") {
    event.stopImmediatePropagation();
    emit("close");
  }
}

/* Only a click that starts and ends on the backdrop closes it, so a text
   selection dragged out of the dialog doesn't. */
let downOnBackdrop = false;
function onPointerdown(event: PointerEvent) {
  downOnBackdrop = event.target === event.currentTarget;
}
function onClick(event: MouseEvent) {
  if (downOnBackdrop && event.target === event.currentTarget) emit("close");
  downOnBackdrop = false;
}

onMounted(() => document.addEventListener("keydown", onKeydown, true));
onUnmounted(() => document.removeEventListener("keydown", onKeydown, true));
</script>

<template>
  <Teleport to="body">
    <div class="modal-layer" @pointerdown="onPointerdown" @click="onClick">
      <div class="modal" role="dialog" aria-modal="true">
        <div class="modal-title">{{ title }}</div>
        <div class="modal-body">
          <slot />
        </div>
        <div class="modal-actions">
          <slot name="actions" />
        </div>
      </div>
    </div>
  </Teleport>
</template>
