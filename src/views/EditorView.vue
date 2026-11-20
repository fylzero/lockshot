<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, shallowRef, watch } from "vue";
import { writeImage } from "@tauri-apps/plugin-clipboard-manager";
import Icon from "../components/Icon.vue";
import ExportBar from "../components/ExportBar.vue";
import {
  bounds,
  canvasToPng,
  drawShape,
  hitTest,
  newId,
  normalize,
  renderFinal,
  TEXT_FONT,
  TEXT_LINE_HEIGHT,
  translate,
  type Rect,
  type Shape,
  type TextShape,
  type Tool,
} from "../annotate";
import { current, settings, showToast } from "../store";
import { errorMessage } from "../api";
import { loadBitmap } from "../pixels";
import { modKey } from "../platform";

const TOOLS: { id: Tool; label: string; key: string }[] = [
  { id: "select", label: "Select and move", key: "V" },
  { id: "arrow", label: "Arrow", key: "A" },
  { id: "line", label: "Line", key: "L" },
  { id: "rect", label: "Rectangle", key: "R" },
  { id: "ellipse", label: "Ellipse", key: "O" },
  { id: "pen", label: "Pen", key: "P" },
  { id: "highlighter", label: "Highlighter", key: "H" },
  { id: "text", label: "Text", key: "T" },
  { id: "step", label: "Numbered step", key: "N" },
  { id: "blur", label: "Pixelate (redact)", key: "B" },
  { id: "crop", label: "Crop", key: "C" },
];
const COLORS = ["#ff3b30", "#ff9500", "#ffcc00", "#34c759", "#2f86d6", "#af52de", "#ffffff", "#111111"];
const SIZES = [
  { label: "Small", stroke: 2, text: 16, step: 11 },
  { label: "Medium", stroke: 4, text: 22, step: 14 },
  { label: "Large", stroke: 7, text: 32, step: 18 },
  { label: "Extra large", stroke: 11, text: 46, step: 24 },
];

const tool = ref<Tool>("arrow");
const color = ref(COLORS[0]);
const sizeIndex = ref(1);
const shapes = shallowRef<Shape[]>([]);
const crop = ref<Rect | null>(null);
const selectedId = ref<number | null>(null);
const editing = ref<TextShape | null>(null);
const editingText = ref("");
const past = ref<{ shapes: Shape[]; crop: Rect | null }[]>([]);
const future = ref<{ shapes: Shape[]; crop: Rect | null }[]>([]);

const image = shallowRef<ImageBitmap | null>(null);
const loadError = ref("");
const canvas = ref<HTMLCanvasElement | null>(null);
const stage = ref<HTMLDivElement | null>(null);
const textArea = ref<HTMLTextAreaElement | null>(null);

const dpr = window.devicePixelRatio || 1;
/** 1 = one image pixel per device pixel. */
const zoom = ref(1);
const fitMode = ref(true);

const width = computed(() => image.value?.width ?? 0);
const height = computed(() => image.value?.height ?? 0);
/** Annotation sizes scale with the capture so they read the same on Retina and 1x shots. */
const unit = computed(() => Math.max(1, Math.min(width.value, height.value) / 600));
const cssScale = computed(() => zoom.value / dpr);
const outputSize = computed(() =>
  crop.value
    ? { width: Math.round(crop.value.w), height: Math.round(crop.value.h) }
    : { width: width.value, height: height.value },
);

function sizeFor(kind: "stroke" | "text" | "step") {
  return Math.round(SIZES[sizeIndex.value][kind] * unit.value);
}

/* ---- Loading ---- */

watch(
  () => current.value?.id,
  (id) => {
    reset();
    image.value?.close();
    image.value = null;
    loadError.value = "";
    if (!id) return;
    const { width: w, height: h } = current.value!;
    loadBitmap(`image/${id}`, w, h)
      .then(async (bitmap) => {
        if (current.value?.id !== id) {
          bitmap.close();
          return;
        }
        image.value?.close();
        image.value = bitmap;
        await nextTick();
        fitMode.value = true;
        fit();
        draw();
        if (settings.value?.copyOnCapture) void copy(true);
      })
      .catch(() => {
        if (current.value?.id === id) loadError.value = "This capture is no longer available. Take a new one.";
      });
  },
  { immediate: true },
);

function reset() {
  shapes.value = [];
  crop.value = null;
  selectedId.value = null;
  past.value = [];
  future.value = [];
  editing.value = null;
}

/* ---- History ---- */

function commit(next: Shape[], nextCrop: Rect | null = crop.value) {
  past.value.push({ shapes: shapes.value, crop: crop.value });
  if (past.value.length > 200) past.value.shift();
  future.value = [];
  shapes.value = next;
  crop.value = nextCrop;
  draw();
}

function undo() {
  const prev = past.value.pop();
  if (!prev) return;
  future.value.push({ shapes: shapes.value, crop: crop.value });
  shapes.value = prev.shapes;
  crop.value = prev.crop;
  selectedId.value = null;
  draw();
}

function redo() {
  const next = future.value.pop();
  if (!next) return;
  past.value.push({ shapes: shapes.value, crop: crop.value });
  shapes.value = next.shapes;
  crop.value = next.crop;
  draw();
}

function deleteSelected() {
  if (selectedId.value == null) return;
  commit(shapes.value.filter((s) => s.id !== selectedId.value));
  selectedId.value = null;
}

function clearAll() {
  if (!shapes.value.length && !crop.value) return;
  commit([], null);
  selectedId.value = null;
}

/* ---- Zoom ---- */

function fit() {
  if (!stage.value || !width.value) return;
  const pad = 48;
  const availW = Math.max(50, stage.value.clientWidth - pad) * dpr;
  const availH = Math.max(50, stage.value.clientHeight - pad) * dpr;
  zoom.value = Math.min(1, availW / width.value, availH / height.value);
}

function setZoom(next: number) {
  fitMode.value = false;
  zoom.value = Math.min(8, Math.max(0.05, next));
}

function onWheel(event: WheelEvent) {
  if (!(event.metaKey || event.ctrlKey)) return;
  event.preventDefault();
  setZoom(zoom.value * (event.deltaY < 0 ? 1.1 : 1 / 1.1));
}

let resizeObserver: ResizeObserver | undefined;

/* ---- Drawing ---- */

let frame = 0;
let draft: Shape | null = null;
let cropDraft: Rect | null = null;

function draw() {
  cancelAnimationFrame(frame);
  frame = requestAnimationFrame(paint);
}

function paint() {
  const img = image.value;
  const el = canvas.value;
  if (!img || !el) return;
  if (el.width !== img.width) el.width = img.width;
  if (el.height !== img.height) el.height = img.height;
  const ctx = el.getContext("2d")!;
  ctx.clearRect(0, 0, el.width, el.height);
  ctx.drawImage(img, 0, 0);
  for (const shape of shapes.value) {
    if (editing.value?.id === shape.id) continue;
    drawShape(ctx, shape, img);
  }
  if (draft) drawShape(ctx, draft, img);

  const line = Math.max(1, 1.5 / cssScale.value);
  const selected = shapes.value.find((s) => s.id === selectedId.value);
  if (selected && tool.value === "select") {
    const b = bounds(selected);
    const pad = 4 / cssScale.value;
    ctx.save();
    ctx.setLineDash([6 / cssScale.value, 4 / cssScale.value]);
    ctx.lineWidth = line;
    ctx.strokeStyle = "#118777";
    ctx.strokeRect(b.x - pad, b.y - pad, b.w + pad * 2, b.h + pad * 2);
    ctx.restore();
  }

  const shownCrop = cropDraft ?? crop.value;
  if (shownCrop) {
    ctx.save();
    ctx.fillStyle = tool.value === "crop" ? "rgb(0 0 0 / 0.55)" : "rgb(0 0 0 / 0.7)";
    ctx.beginPath();
    ctx.rect(0, 0, el.width, el.height);
    ctx.rect(shownCrop.x, shownCrop.y, shownCrop.w, shownCrop.h);
    ctx.fill("evenodd");
    ctx.lineWidth = line;
    ctx.strokeStyle = "#fff";
    ctx.setLineDash([6 / cssScale.value, 4 / cssScale.value]);
    ctx.strokeRect(shownCrop.x, shownCrop.y, shownCrop.w, shownCrop.h);
    ctx.restore();
  }
}

watch([shapes, crop, selectedId, tool, zoom], draw);

function toImage(event: PointerEvent | MouseEvent) {
  const rect = canvas.value!.getBoundingClientRect();
  return {
    x: ((event.clientX - rect.left) / rect.width) * width.value,
    y: ((event.clientY - rect.top) / rect.height) * height.value,
  };
}

function clampToImage(p: { x: number; y: number }) {
  return { x: Math.min(width.value, Math.max(0, p.x)), y: Math.min(height.value, Math.max(0, p.y)) };
}

function topShapeAt(x: number, y: number) {
  const slop = 6 / cssScale.value;
  for (let i = shapes.value.length - 1; i >= 0; i--) {
    if (hitTest(shapes.value[i], x, y, slop)) return shapes.value[i];
  }
  return null;
}

let drag: { start: { x: number; y: number }; moved: boolean; original?: Shape } | null = null;

function onPointerDown(event: PointerEvent) {
  if (event.button !== 0 || !image.value) return;
  /* Keep the browser from moving focus to the canvas, which would blur the
     text box this click may be about to open. */
  event.preventDefault();
  if (editing.value) {
    finishText();
    return;
  }
  const p = toImage(event);
  canvas.value!.setPointerCapture(event.pointerId);

  switch (tool.value) {
    case "select": {
      const hit = topShapeAt(p.x, p.y);
      selectedId.value = hit?.id ?? null;
      drag = hit ? { start: p, moved: false, original: hit } : null;
      if (hit) applyStyleFrom(hit);
      return;
    }
    case "text": {
      const hit = topShapeAt(p.x, p.y);
      if (hit?.kind === "text") {
        startText(hit);
      } else {
        startText({ id: newId(), kind: "text", color: color.value, size: sizeFor("text"), x: p.x, y: p.y - sizeFor("text") * 0.6, text: "" });
      }
      return;
    }
    case "step": {
      const n = Math.max(0, ...shapes.value.map((s) => (s.kind === "step" ? s.n : 0))) + 1;
      commit([...shapes.value, { id: newId(), kind: "step", color: color.value, size: sizeFor("step"), x: p.x, y: p.y, n }]);
      return;
    }
    case "pen":
    case "highlighter":
      draft = { id: newId(), kind: tool.value, color: color.value, width: sizeFor("stroke"), points: [p.x, p.y] };
      break;
    case "crop": {
      const c = clampToImage(p);
      cropDraft = { x: c.x, y: c.y, w: 0, h: 0 };
      break;
    }
    default:
      draft = { id: newId(), kind: tool.value, color: color.value, width: sizeFor("stroke"), x1: p.x, y1: p.y, x2: p.x, y2: p.y };
  }
  drag = { start: p, moved: false };
  draw();
}

function onPointerMove(event: PointerEvent) {
  if (!drag) {
    if (tool.value === "select" && canvas.value) {
      const p = toImage(event);
      canvas.value.style.cursor = topShapeAt(p.x, p.y) ? "move" : "default";
    }
    return;
  }
  let p = toImage(event);
  drag.moved ||= Math.hypot(p.x - drag.start.x, p.y - drag.start.y) * cssScale.value > 2;

  if (tool.value === "select" && drag.original) {
    const moved = translate(drag.original, p.x - drag.start.x, p.y - drag.start.y);
    shapes.value = shapes.value.map((s) => (s.id === moved.id ? moved : s));
    return;
  }
  if (tool.value === "crop" && cropDraft) {
    const c = clampToImage(p);
    const s = clampToImage(drag.start);
    cropDraft = normalize(s.x, s.y, c.x, c.y);
    draw();
    return;
  }
  if (!draft) return;
  if (draft.kind === "pen" || draft.kind === "highlighter") {
    draft.points.push(p.x, p.y);
  } else if ("x2" in draft) {
    /* Shift keeps lines at 45° steps and boxes square. */
    if (event.shiftKey) {
      const dx = p.x - draft.x1;
      const dy = p.y - draft.y1;
      if (draft.kind === "arrow" || draft.kind === "line") {
        const angle = Math.round(Math.atan2(dy, dx) / (Math.PI / 4)) * (Math.PI / 4);
        const len = Math.hypot(dx, dy);
        p = { x: draft.x1 + Math.cos(angle) * len, y: draft.y1 + Math.sin(angle) * len };
      } else {
        const side = Math.max(Math.abs(dx), Math.abs(dy));
        p = { x: draft.x1 + Math.sign(dx || 1) * side, y: draft.y1 + Math.sign(dy || 1) * side };
      }
    }
    draft.x2 = p.x;
    draft.y2 = p.y;
  }
  draw();
}

function onPointerUp() {
  if (!drag) return;
  const { moved, original } = drag;
  drag = null;

  if (tool.value === "select") {
    if (original && moved) {
      /* The drag moved the shape live; put the original back so the
         history entry records where it started. */
      const now = shapes.value;
      shapes.value = now.map((s) => (s.id === original.id ? original : s));
      commit(now);
    }
    return;
  }
  if (tool.value === "crop") {
    const next = cropDraft;
    cropDraft = null;
    if (next && next.w * cssScale.value > 4 && next.h * cssScale.value > 4) {
      commit(shapes.value, next);
      tool.value = "select";
    } else draw();
    return;
  }
  const shape = draft;
  draft = null;
  if (!shape) return;
  const tooSmall =
    "x2" in shape
      ? Math.hypot(shape.x2 - shape.x1, shape.y2 - shape.y1) * cssScale.value < 4
      : shape.kind !== "pen" && shape.kind !== "highlighter";
  if (tooSmall) {
    draw();
    return;
  }
  commit([...shapes.value, shape]);
}

/* Selecting a shape loads its color and size into the toolbar, and changing
   them restyles the selection. */
function applyStyleFrom(shape: Shape) {
  if (shape.kind !== "blur") color.value = shape.color;
}

function restyleSelected(change: { color?: string; size?: number }) {
  if (tool.value !== "select" || selectedId.value == null) return;
  commit(
    shapes.value.map((s) => {
      if (s.id !== selectedId.value) return s;
      const next = { ...s } as Shape;
      if (change.color && next.kind !== "blur") next.color = change.color;
      if (change.size != null) {
        const size = SIZES[change.size];
        if (next.kind === "text") next.size = Math.round(size.text * unit.value);
        else if (next.kind === "step") next.size = Math.round(size.step * unit.value);
        else next.width = Math.round(size.stroke * unit.value);
      }
      return next;
    }),
  );
}

function pickColor(next: string) {
  color.value = next;
  restyleSelected({ color: next });
}

function pickSize(index: number) {
  sizeIndex.value = index;
  restyleSelected({ size: index });
}

/* ---- Text entry ---- */

const textStyle = computed(() => {
  const t = editing.value;
  if (!t) return {};
  const fontSize = t.size * cssScale.value;
  const lines = editingText.value.split("\n");
  const longest = Math.max(4, ...lines.map((l) => l.length));
  return {
    left: `${t.x * cssScale.value}px`,
    top: `${t.y * cssScale.value}px`,
    fontSize: `${fontSize}px`,
    color: t.color,
    fontFamily: TEXT_FONT,
    lineHeight: String(TEXT_LINE_HEIGHT),
    width: `${longest * fontSize * 0.62 + 12}px`,
    height: `${lines.length * fontSize * TEXT_LINE_HEIGHT + 6}px`,
  };
});

function startText(shape: TextShape) {
  editing.value = shape;
  editingText.value = shape.text;
  draw();
  void nextTick(() => {
    textArea.value?.focus();
    textArea.value?.select();
  });
}

function finishText() {
  const t = editing.value;
  if (!t) return;
  editing.value = null;
  const text = editingText.value.replace(/\s+$/, "");
  const exists = shapes.value.some((s) => s.id === t.id);
  if (!text) {
    if (exists) commit(shapes.value.filter((s) => s.id !== t.id));
    else draw();
    return;
  }
  const next = { ...t, text };
  commit(exists ? shapes.value.map((s) => (s.id === t.id ? next : s)) : [...shapes.value, next]);
}

function onTextKey(event: KeyboardEvent) {
  if (event.key === "Escape" || (event.key === "Enter" && !event.shiftKey)) {
    event.preventDefault();
    finishText();
  }
  event.stopPropagation();
}

function onDoubleClick(event: MouseEvent) {
  if (tool.value !== "select") return;
  const p = toImage(event);
  const hit = topShapeAt(p.x, p.y);
  if (hit?.kind === "text") startText(hit);
}

/* ---- Output ---- */

async function renderPng() {
  if (!image.value) throw new Error("There is no capture to export.");
  if (editing.value) finishText();
  return canvasToPng(renderFinal(image.value, shapes.value, crop.value));
}

async function copy(quiet = false) {
  try {
    await writeImage(await renderPng());
    if (!quiet) showToast("Copied to the clipboard.");
  } catch (err) {
    showToast(`Could not copy: ${errorMessage(err)}`, "error");
  }
}

const exportBar = ref<InstanceType<typeof ExportBar> | null>(null);

/* ---- Keyboard ---- */

function onKeyDown(event: KeyboardEvent) {
  const target = event.target as HTMLElement | null;
  if (target && (target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.tagName === "SELECT")) return;
  if (!image.value) return;
  const mod = event.metaKey || event.ctrlKey;
  const key = event.key.toLowerCase();

  if (mod && key === "z") {
    event.preventDefault();
    event.shiftKey ? redo() : undo();
  } else if (mod && key === "y") {
    event.preventDefault();
    redo();
  } else if (mod && key === "c") {
    event.preventDefault();
    void copy();
  } else if (mod && key === "s") {
    event.preventDefault();
    event.shiftKey ? exportBar.value?.saveAs() : exportBar.value?.save();
  } else if (mod && (key === "=" || key === "+")) {
    event.preventDefault();
    setZoom(zoom.value * 1.25);
  } else if (mod && key === "-") {
    event.preventDefault();
    setZoom(zoom.value / 1.25);
  } else if (mod && key === "0") {
    event.preventDefault();
    fitMode.value = true;
    fit();
  } else if ((key === "delete" || key === "backspace") && selectedId.value != null) {
    event.preventDefault();
    deleteSelected();
  } else if (key === "escape") {
    selectedId.value = null;
    cropDraft = null;
    draw();
  } else if (!mod && !event.altKey) {
    const match = TOOLS.find((t) => t.key.toLowerCase() === key);
    if (match) tool.value = match.id;
    const n = Number(key);
    if (n >= 1 && n <= SIZES.length) pickSize(n - 1);
  }
}

watch(tool, (next) => {
  if (next !== "select") selectedId.value = null;
  if (editing.value) finishText();
  if (canvas.value) canvas.value.style.cursor = next === "select" ? "default" : next === "text" ? "text" : "crosshair";
});

onMounted(() => {
  window.addEventListener("keydown", onKeyDown);
  resizeObserver = new ResizeObserver(() => {
    if (fitMode.value) fit();
  });
  if (stage.value) resizeObserver.observe(stage.value);
});

watch(stage, (el, old) => {
  if (old) resizeObserver?.unobserve(old);
  if (el) resizeObserver?.observe(el);
});

onBeforeUnmount(() => {
  window.removeEventListener("keydown", onKeyDown);
  resizeObserver?.disconnect();
  cancelAnimationFrame(frame);
});
</script>

<template>
  <div v-if="current" class="editor">
    <div class="toolbar">
      <div class="tool-group">
        <button
          v-for="t in TOOLS"
          :key="t.id"
          class="tool"
          :class="{ active: tool === t.id }"
          :title="`${t.label} (${t.key})`"
          :aria-label="t.label"
          @click="tool = t.id"
        >
          <Icon :name="t.id" />
        </button>
      </div>
      <span class="divider" />
      <div class="tool-group">
        <button
          v-for="c in COLORS"
          :key="c"
          class="swatch"
          :class="{ active: color === c }"
          :style="{ background: c }"
          :title="c"
          :aria-label="`Color ${c}`"
          @click="pickColor(c)"
        />
        <label class="swatch swatch-custom" :class="{ active: !COLORS.includes(color) }" title="Custom color">
          <input type="color" :value="color" @input="pickColor(($event.target as HTMLInputElement).value)" />
        </label>
      </div>
      <span class="divider" />
      <div class="tool-group">
        <button
          v-for="(s, i) in SIZES"
          :key="s.label"
          class="tool"
          :class="{ active: sizeIndex === i }"
          :title="`${s.label} (${i + 1})`"
          :aria-label="s.label"
          @click="pickSize(i)"
        >
          <span class="size-dot" :style="{ width: `${4 + i * 3}px`, height: `${4 + i * 3}px` }" />
        </button>
      </div>
      <span class="divider" />
      <div class="tool-group">
        <button class="tool" :disabled="!past.length" :title="`Undo (${modKey}Z)`" aria-label="Undo" @click="undo">
          <Icon name="undo" />
        </button>
        <button class="tool" :disabled="!future.length" :title="`Redo (${modKey}⇧Z)`" aria-label="Redo" @click="redo">
          <Icon name="redo" />
        </button>
        <button
          v-if="selectedId != null"
          class="tool"
          title="Delete selected (Delete)"
          aria-label="Delete selected"
          @click="deleteSelected"
        >
          <Icon name="trash" />
        </button>
        <button v-if="crop" class="ghost" title="Remove the crop" @click="commit(shapes, null)">Clear crop</button>
        <button
          v-if="shapes.length || crop"
          class="ghost"
          title="Remove every annotation and the crop"
          @click="clearAll"
        >
          Reset
        </button>
      </div>
      <span class="spacer" />
      <span class="faint" style="font-size: 0.8rem; font-variant-numeric: tabular-nums">
        {{ outputSize.width }} × {{ outputSize.height }}
      </span>
    </div>

    <div ref="stage" class="stage" @wheel="onWheel">
      <p v-if="loadError" class="muted">{{ loadError }}</p>
      <div
        v-else-if="image"
        class="canvas-wrap"
        :style="{ width: `${width * cssScale}px`, height: `${height * cssScale}px` }"
      >
        <canvas
          ref="canvas"
          :style="{ width: `${width * cssScale}px`, height: `${height * cssScale}px`, cursor: 'crosshair' }"
          @pointerdown="onPointerDown"
          @pointermove="onPointerMove"
          @pointerup="onPointerUp"
          @pointercancel="onPointerUp"
          @dblclick="onDoubleClick"
        />
        <textarea
          v-if="editing"
          ref="textArea"
          v-model="editingText"
          class="text-entry"
          :style="textStyle"
          spellcheck="false"
          placeholder="Text"
          @keydown="onTextKey"
          @blur="finishText"
        />
      </div>
      <div v-if="image" class="zoom-controls">
        <button title="Zoom out" @click="setZoom(zoom / 1.25)">−</button>
        <span>{{ Math.round(zoom * 100) }}%</span>
        <button title="Zoom in" @click="setZoom(zoom * 1.25)">+</button>
        <button :class="{ primary: fitMode }" :title="`Fit (${modKey}0)`" @click="fitMode = true; fit()">Fit</button>
        <button title="Actual pixels" @click="setZoom(1)">1:1</button>
      </div>
    </div>

    <ExportBar ref="exportBar" :render="renderPng" :ready="!!image" @copy="copy()" />
  </div>
</template>
