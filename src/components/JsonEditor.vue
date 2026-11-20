<script setup lang="ts">
/**
 * A small JSON editor: a transparent textarea laid over a syntax-highlighted
 * copy of the same text, with line numbers. The settings file is a few dozen
 * lines, so this stays dependency-free instead of pulling in a code editor.
 */
import { computed } from "vue";

const model = defineModel<string>({ required: true });
defineProps<{ errorLine?: number | null }>();

const lineCount = computed(() => model.value.split("\n").length);

const TOKEN = /("(?:[^"\\\n]|\\.)*")(\s*:)?|(-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)|\b(true|false|null)\b/g;

function escape(text: string) {
  return text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

const highlighted = computed(() => {
  const text = model.value;
  let html = "";
  let last = 0;
  for (const m of text.matchAll(TOKEN)) {
    html += escape(text.slice(last, m.index));
    const [whole, str, colon, num, word] = m;
    if (str !== undefined) {
      html += `<span class="${colon ? "j-key" : "j-str"}">${escape(str)}</span>${colon ? escape(colon) : ""}`;
    } else if (num !== undefined) {
      html += `<span class="j-num">${num}</span>`;
    } else {
      html += `<span class="j-word">${word}</span>`;
    }
    last = m.index + whole.length;
  }
  /* A trailing newline needs a character after it to take up a line. */
  return html + escape(text.slice(last)) + " ";
});

/** Tab indents with two spaces instead of leaving the editor. */
function onKeydown(event: KeyboardEvent) {
  if (event.key !== "Tab" || event.metaKey || event.ctrlKey || event.altKey) return;
  event.preventDefault();
  const el = event.target as HTMLTextAreaElement;
  const { selectionStart: start, selectionEnd: end } = el;
  if (event.shiftKey) {
    const lineStart = model.value.lastIndexOf("\n", start - 1) + 1;
    const indent = model.value.slice(lineStart, lineStart + 2).match(/^ {1,2}/)?.[0].length ?? 0;
    if (!indent) return;
    model.value = model.value.slice(0, lineStart) + model.value.slice(lineStart + indent);
    requestAnimationFrame(() => el.setSelectionRange(Math.max(lineStart, start - indent), Math.max(lineStart, end - indent)));
    return;
  }
  model.value = model.value.slice(0, start) + "  " + model.value.slice(end);
  requestAnimationFrame(() => el.setSelectionRange(start + 2, start + 2));
}
</script>

<template>
  <div class="json-editor">
    <div class="json-gutter" aria-hidden="true">
      <div v-for="n in lineCount" :key="n" :class="{ bad: n === errorLine }">{{ n }}</div>
    </div>
    <div class="json-code">
      <pre aria-hidden="true" v-html="highlighted" />
      <textarea
        v-model="model"
        spellcheck="false"
        autocapitalize="off"
        autocomplete="off"
        wrap="off"
        aria-label="Settings JSON"
        @keydown="onKeydown"
      />
    </div>
  </div>
</template>
