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
  textOutlineColor,
  textOutlineWidth,
  type TextOutline,
  type TextShape,
  type Tool,
} from "../annotate";
import { settings, showToast, type CaptureDoc } from "../store";

/*
 * One editor per open capture. App.vue keeps every one alive and shows the
 * active tab's, so annotations and undo history survive switching tabs.
 */
const props = defineProps<{ capture: CaptureDoc; active: boolean }>();
import { errorMessage } from "../api";
import { loadBitmap } from "../pixels";
import { modKey } from "../platform";

/* Drawing tools share one toolbar button that shows whichever is picked. */
const SHAPE_TOOLS: { id: Tool; label: string; key: string }[] = [
  { id: "arrow", label: "Arrow", key: "A" },
  { id: "line", label: "Line", key: "L" },
  { id: "rect", label: "Rectangle", key: "R" },
  { id: "ellipse", label: "Ellipse", key: "O" },
  { id: "pen", label: "Pen", key: "P" },
  { id: "highlighter", label: "Highlighter", key: "H" },
  { id: "step", label: "Numbered step", key: "N" },
];
const OTHER_TOOLS: { id: Tool; label: string; key: string }[] = [
  { id: "text", label: "Text", key: "T" },
  { id: "blur", label: "Pixelate (redact)", key: "B" },
  { id: "crop", label: "Crop", key: "C" },
];
const TOOLS = [...SHAPE_TOOLS, ...OTHER_TOOLS];
const COLORS = ["#f5418c", "#e40010", "#fc7502", "#fef236", "#06e617", "#00caeb", "#a233ff", "#ffffff", "#000000"];
const SIZES = [
  { label: "Extra small", stroke: 2, text: 16, step: 11 },
  { label: "Small", stroke: 4, text: 22, step: 14 },
  { label: "Medium", stroke: 7, text: 32, step: 18 },
  { label: "Large", stroke: 11, text: 46, step: 24 },
  { label: "Extra large", stroke: 16, text: 64, step: 32 },
];
const OUTLINES: { id: TextOutline; label: string }[] = [
  { id: "auto", label: "Automatic" },
  { id: "black", label: "Black" },
  { id: "white", label: "White" },
  { id: "none", label: "None" },
];

const tool = ref<Tool>("arrow");
const shapeTool = ref<Tool>("arrow");
const color = ref(COLORS[0]);
/* Medium: bold enough to read at a glance on a full-screen capture. */
const sizeIndex = ref(2);
const textOutline = ref<TextOutline>("auto");
const shapes = shallowRef<Shape[]>([]);
const crop = ref<Rect | null>(null);
/** The crop being drawn while the Crop tool is on; Apply commits it. */
const pendingCrop = ref<Rect | null>(null);
const selectedId = ref<number | null>(null);
const editing = ref<TextShape | null>(null);
const editingText = ref("");
const past = ref<{ shapes: Shape[]; crop: Rect | null }[]>([]);
const future = ref<{ shapes: Shape[]; crop: Rect | null }[]>([]);
const openMenu = ref<"shape" | "color" | "size" | "outline" | null>(null);

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
/** The part of the image on screen: the applied crop, or all of it while cropping. */
const view = computed<Rect>(() =>
  crop.value && tool.value !== "crop" ? crop.value : { x: 0, y: 0, w: width.value, h: height.value },
);
const outputSize = computed(() => {
  const r = tool.value === "crop" && pendingCrop.value ? pendingCrop.value : crop.value;
  return r ? { width: Math.round(r.w), height: Math.round(r.h) } : { width: width.value, height: height.value };
});
const selectedShape = computed(() => shapes.value.find((s) => s.id === selectedId.value) ?? null);
const currentShapeTool = computed(() => SHAPE_TOOLS.find((t) => t.id === shapeTool.value)!);
const showOutline = computed(
  () => tool.value === "text" || !!editing.value || selectedShape.value?.kind === "text",
);

function sizeFor(kind: "stroke" | "text" | "step") {
  return Math.round(SIZES[sizeIndex.value][kind] * unit.value);
}

/* ---- Loading ---- */

watch(
  () => props.capture.id,
  (id) => {
    reset();
    image.value?.close();
    image.value = null;
    loadError.value = "";
    if (!id) return;
    const { width: w, height: h } = props.capture;
    loadBitmap(`image/${id}`, w, h)
      .then(async (bitmap) => {
        if (props.capture.id !== id) {
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
        if (props.capture.id === id) loadError.value = "This capture is no longer available. Take a new one.";
      });
  },
  { immediate: true },
);

function reset() {
  shapes.value = [];
  crop.value = null;
  pendingCrop.value = null;
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
  if (tool.value === "crop") pendingCrop.value = prev.crop && { ...prev.crop };
  draw();
}

function redo() {
  const next = future.value.pop();
  if (!next) return;
  past.value.push({ shapes: shapes.value, crop: crop.value });
  shapes.value = next.shapes;
  crop.value = next.crop;
  selectedId.value = null;
  if (tool.value === "crop") pendingCrop.value = next.crop && { ...next.crop };
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
  pendingCrop.value = null;
}

/* ---- Crop ---- */

let toolBeforeCrop: Tool = "arrow";

function applyCrop() {
  const r = pendingCrop.value;
  if (r && r.w >= 1 && r.h >= 1) {
    const x = Math.max(0, Math.round(r.x));
    const y = Math.max(0, Math.round(r.y));
    const next = {
      x,
      y,
      w: Math.max(1, Math.min(width.value - x, Math.round(r.w))),
      h: Math.max(1, Math.min(height.value - y, Math.round(r.h))),
    };
    const same = crop.value && (["x", "y", "w", "h"] as const).every((k) => crop.value![k] === next[k]);
    if (!same) commit(shapes.value, next);
  }
  tool.value = toolBeforeCrop;
}

function cancelCrop() {
  tool.value = toolBeforeCrop;
}

/* ---- Zoom ---- */

function fit() {
  /* A hidden tab's stage measures 0; it refits when it becomes visible. */
  if (!stage.value || !view.value.w || !stage.value.clientWidth) return;
  const pad = 48;
  const availW = Math.max(50, stage.value.clientWidth - pad) * dpr;
  const availH = Math.max(50, stage.value.clientHeight - pad) * dpr;
  zoom.value = Math.min(1, availW / view.value.w, availH / view.value.h);
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

function draw() {
  cancelAnimationFrame(frame);
  frame = requestAnimationFrame(paint);
}

function paint() {
  const img = image.value;
  const el = canvas.value;
  if (!img || !el) return;
  const v = view.value;
  const w = Math.max(1, Math.round(v.w));
  const h = Math.max(1, Math.round(v.h));
  if (el.width !== w) el.width = w;
  if (el.height !== h) el.height = h;
  const ctx = el.getContext("2d")!;
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  ctx.clearRect(0, 0, el.width, el.height);
  ctx.translate(-v.x, -v.y);
  ctx.drawImage(img, 0, 0);
  for (const shape of shapes.value) {
    if (editing.value?.id === shape.id) continue;
    drawShape(ctx, shape, img);
  }
  if (draft) drawShape(ctx, draft, img);

  const line = Math.max(1, 1.5 / cssScale.value);
  const selected = selectedShape.value;
  if (selected && !editing.value && tool.value !== "crop") {
    const b = bounds(selected);
    const pad = 4 / cssScale.value;
    ctx.save();
    ctx.setLineDash([6 / cssScale.value, 4 / cssScale.value]);
    ctx.lineWidth = line;
    ctx.strokeStyle = "#118777";
    ctx.strokeRect(b.x - pad, b.y - pad, b.w + pad * 2, b.h + pad * 2);
    ctx.restore();
  }

  const shownCrop = tool.value === "crop" ? pendingCrop.value : null;
  if (shownCrop) {
    ctx.save();
    ctx.fillStyle = "rgb(0 0 0 / 0.55)";
    ctx.beginPath();
    ctx.rect(0, 0, img.width, img.height);
    ctx.rect(shownCrop.x, shownCrop.y, shownCrop.w, shownCrop.h);
    ctx.fill("evenodd");
    ctx.lineWidth = line;
    ctx.strokeStyle = "#fff";
    ctx.setLineDash([6 / cssScale.value, 4 / cssScale.value]);
    ctx.strokeRect(shownCrop.x, shownCrop.y, shownCrop.w, shownCrop.h);
    ctx.restore();
  }
}

watch([shapes, crop, pendingCrop, selectedId, tool, zoom], draw);

watch(
  () => [view.value.x, view.value.y, view.value.w, view.value.h],
  async () => {
    await nextTick();
    if (fitMode.value) fit();
    draw();
  },
);

function toImage(event: PointerEvent | MouseEvent) {
  const rect = canvas.value!.getBoundingClientRect();
  const v = view.value;
  return {
    x: v.x + ((event.clientX - rect.left) / rect.width) * v.w,
    y: v.y + ((event.clientY - rect.top) / rect.height) * v.h,
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

function insideRect(r: Rect | null, p: { x: number; y: number }) {
  return !!r && p.x >= r.x && p.x <= r.x + r.w && p.y >= r.y && p.y <= r.y + r.h;
}

function toolCursor() {
  return tool.value === "text" ? "text" : "crosshair";
}

let drag: {
  start: { x: number; y: number };
  moved: boolean;
  /** Set when the press landed on an existing annotation, which then moves. */
  original?: Shape;
  /** Set while the Crop tool is drawing or moving the pending crop. */
  crop?: { mode: "draw" | "move"; before: Rect | null };
} | null = null;

function onPointerDown(event: PointerEvent) {
  if (event.button !== 0 || !image.value) return;
  /* Keep the browser from moving focus to the canvas, which would blur the
     text box this click may be about to open. */
  event.preventDefault();
  openMenu.value = null;
  if (editing.value) {
    finishText();
    return;
  }
  const p = toImage(event);
  canvas.value!.setPointerCapture(event.pointerId);

  if (tool.value === "crop") {
    const before = pendingCrop.value && { ...pendingCrop.value };
    if (insideRect(pendingCrop.value, p)) {
      drag = { start: p, moved: false, crop: { mode: "move", before } };
    } else {
      const c = clampToImage(p);
      pendingCrop.value = { x: c.x, y: c.y, w: 0, h: 0 };
      drag = { start: p, moved: false, crop: { mode: "draw", before } };
    }
    return;
  }

  /* Any tool grabs an annotation under the pointer: a click selects it,
     a drag moves it. */
  const hit = topShapeAt(p.x, p.y);
  if (hit) {
    selectedId.value = hit.id;
    applyStyleFrom(hit);
    drag = { start: p, moved: false, original: hit };
    draw();
    return;
  }
  selectedId.value = null;

  switch (tool.value) {
    case "text":
      startText({
        id: newId(),
        kind: "text",
        color: color.value,
        size: sizeFor("text"),
        outline: textOutline.value,
        x: p.x,
        y: p.y - sizeFor("text") * 0.6,
        text: "",
      });
      return;
    case "step": {
      const n = Math.max(0, ...shapes.value.map((s) => (s.kind === "step" ? s.n : 0))) + 1;
      commit([...shapes.value, { id: newId(), kind: "step", color: color.value, size: sizeFor("step"), x: p.x, y: p.y, n }]);
      return;
    }
    case "pen":
    case "highlighter":
      draft = { id: newId(), kind: tool.value, color: color.value, width: sizeFor("stroke"), points: [p.x, p.y] };
      break;
    default:
      draft = { id: newId(), kind: tool.value as "arrow", color: color.value, width: sizeFor("stroke"), x1: p.x, y1: p.y, x2: p.x, y2: p.y };
  }
  drag = { start: p, moved: false };
  draw();
}

function onPointerMove(event: PointerEvent) {
  if (!drag) {
    if (canvas.value && image.value) {
      const p = toImage(event);
      const grab = tool.value === "crop" ? insideRect(pendingCrop.value, p) : !!topShapeAt(p.x, p.y);
      canvas.value.style.cursor = grab ? "move" : toolCursor();
    }
    return;
  }
  let p = toImage(event);
  drag.moved ||= Math.hypot(p.x - drag.start.x, p.y - drag.start.y) * cssScale.value > 2;

  if (drag.crop) {
    if (drag.crop.mode === "move" && drag.crop.before) {
      const b = drag.crop.before;
      pendingCrop.value = {
        ...b,
        x: Math.min(width.value - b.w, Math.max(0, b.x + p.x - drag.start.x)),
        y: Math.min(height.value - b.h, Math.max(0, b.y + p.y - drag.start.y)),
      };
    } else {
      const c = clampToImage(p);
      const s = clampToImage(drag.start);
      pendingCrop.value = normalize(s.x, s.y, c.x, c.y);
    }
    return;
  }
  if (drag.original) {
    if (!drag.moved) return;
    const moved = translate(drag.original, p.x - drag.start.x, p.y - drag.start.y);
    shapes.value = shapes.value.map((s) => (s.id === moved.id ? moved : s));
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
  const { moved, original, crop: cropDrag } = drag;
  drag = null;

  if (cropDrag) {
    const r = pendingCrop.value;
    /* A click, or a sliver too small to see, keeps the crop that was there. */
    if (cropDrag.mode === "draw" && !(r && r.w * cssScale.value > 4 && r.h * cssScale.value > 4)) {
      pendingCrop.value = cropDrag.before;
    }
    return;
  }
  if (original) {
    if (moved) {
      /* The drag moved the shape live; put the original back so the
         history entry records where it started. */
      const now = shapes.value;
      shapes.value = now.map((s) => (s.id === original.id ? original : s));
      commit(now);
    } else if (tool.value === "text" && original.kind === "text") {
      startText(original);
    }
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

/* ---- Style ---- */

/* Selecting a shape loads its color into the toolbar, and changing the
   color, size or outline restyles the selection (or the text being typed). */
function applyStyleFrom(shape: Shape) {
  if (shape.kind !== "blur") color.value = shape.color;
  if (shape.kind === "text") textOutline.value = shape.outline ?? "auto";
}

function restyleSelected(change: { color?: string; size?: number; outline?: TextOutline }) {
  const t = editing.value;
  if (t) {
    if (change.color) t.color = change.color;
    if (change.size != null) t.size = Math.round(SIZES[change.size].text * unit.value);
    if (change.outline) t.outline = change.outline;
    return;
  }
  if (selectedId.value == null) return;
  commit(
    shapes.value.map((s) => {
      if (s.id !== selectedId.value) return s;
      const next = { ...s } as Shape;
      if (change.color && next.kind !== "blur") next.color = change.color;
      if (change.outline && next.kind === "text") next.outline = change.outline;
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

function pickTool(next: Tool) {
  tool.value = next;
  openMenu.value = null;
}

function pickColor(next: string, close = true) {
  color.value = next;
  restyleSelected({ color: next });
  if (close) openMenu.value = null;
}

function pickSize(index: number) {
  sizeIndex.value = index;
  restyleSelected({ size: index });
  openMenu.value = null;
}

function pickOutline(next: TextOutline) {
  textOutline.value = next;
  restyleSelected({ outline: next });
  openMenu.value = null;
}

function toggleMenu(menu: NonNullable<typeof openMenu.value>) {
  openMenu.value = openMenu.value === menu ? null : menu;
}

function onShapeButton() {
  if (!SHAPE_TOOLS.some((t) => t.id === tool.value)) tool.value = shapeTool.value;
  toggleMenu("shape");
}

/* Clicking the toolbar mustn't take focus from the text box being typed in,
   or it would close before the new color or size reaches it. */
function onToolbarMouseDown(event: MouseEvent) {
  if ((event.target as HTMLElement).tagName !== "INPUT") event.preventDefault();
}

function onWindowPointerDown(event: PointerEvent) {
  if (!openMenu.value) return;
  const target = event.target as HTMLElement | null;
  if (!target?.closest(`[data-menu="${openMenu.value}"]`)) openMenu.value = null;
}

/* ---- Text entry ---- */

const textBox = computed(() => {
  const t = editing.value;
  if (!t) return null;
  const fontSize = t.size * cssScale.value;
  const lines = editingText.value.split("\n");
  const longest = Math.max(4, ...lines.map((l) => l.length));
  return {
    left: (t.x - view.value.x) * cssScale.value,
    top: (t.y - view.value.y) * cssScale.value,
    width: longest * fontSize * 0.62 + 12,
    height: lines.length * fontSize * TEXT_LINE_HEIGHT + 6,
    fontSize,
  };
});

const textStyle = computed(() => {
  const t = editing.value;
  const box = textBox.value;
  if (!t || !box) return {};
  const outline = textOutlineColor(t);
  return {
    left: `${box.left}px`,
    top: `${box.top}px`,
    fontSize: `${box.fontSize}px`,
    color: t.color,
    fontFamily: TEXT_FONT,
    lineHeight: String(TEXT_LINE_HEIGHT),
    width: `${box.width}px`,
    height: `${box.height}px`,
    WebkitTextStroke: outline ? `${textOutlineWidth(t.size) * cssScale.value}px ${outline}` : "0",
  };
});

const resizeHandleStyle = computed(() => {
  const box = textBox.value;
  if (!box) return {};
  return { left: `${box.left + box.width}px`, top: `${box.top + box.height}px` };
});

function startText(shape: TextShape) {
  /* A copy, so resizing and restyling never touch the shape in history. */
  editing.value = { ...shape };
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
  resize = null;
  const text = editingText.value.replace(/\s+$/, "");
  const exists = shapes.value.some((s) => s.id === t.id);
  if (!text) {
    if (exists) commit(shapes.value.filter((s) => s.id !== t.id));
    else draw();
    return;
  }
  const next = { ...t, text };
  commit(exists ? shapes.value.map((s) => (s.id === t.id ? next : s)) : [...shapes.value, next]);
  /* Leave it selected so the toolbar can still restyle it. */
  selectedId.value = t.id;
}

function onTextKey(event: KeyboardEvent) {
  if (event.key === "Escape" || (event.key === "Enter" && !event.shiftKey)) {
    event.preventDefault();
    finishText();
  }
  event.stopPropagation();
}

/* The dot on the text box's corner scales the text: dragging it down makes
   the text bigger, and the corner follows the pointer. */
let resize: { startY: number; startSize: number; startHeight: number } | null = null;

function onResizeDown(event: PointerEvent) {
  const t = editing.value;
  if (event.button !== 0 || !t) return;
  event.preventDefault();
  event.stopPropagation();
  (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
  const lines = editingText.value.split("\n").length;
  resize = { startY: event.clientY, startSize: t.size, startHeight: lines * t.size * TEXT_LINE_HEIGHT * cssScale.value };
}

function onResizeMove(event: PointerEvent) {
  const t = editing.value;
  if (!resize || !t) return;
  const h = Math.max(4, resize.startHeight + event.clientY - resize.startY);
  t.size = Math.max(6, Math.min(2000, Math.round((resize.startSize * h) / resize.startHeight)));
}

function onResizeUp() {
  resize = null;
  textArea.value?.focus();
}

function onDoubleClick(event: MouseEvent) {
  if (tool.value === "crop") return;
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
  /* Every open editor listens; only the visible one acts. */
  if (!props.active) return;
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
  } else if (key === "escape") {
    if (openMenu.value) openMenu.value = null;
    else if (tool.value === "crop") cancelCrop();
    else selectedId.value = null;
  } else if (key === "enter" && tool.value === "crop") {
    event.preventDefault();
    applyCrop();
  } else if ((key === "delete" || key === "backspace") && selectedId.value != null) {
    event.preventDefault();
    deleteSelected();
  } else if (!mod && !event.altKey) {
    const match = TOOLS.find((t) => t.key.toLowerCase() === key);
    if (match) pickTool(match.id);
    const n = Number(key);
    if (n >= 1 && n <= SIZES.length) pickSize(n - 1);
  }
}

watch(tool, (next, prev) => {
  if (editing.value) finishText();
  selectedId.value = null;
  if (SHAPE_TOOLS.some((t) => t.id === next)) shapeTool.value = next;
  if (next === "crop") {
    toolBeforeCrop = prev;
    pendingCrop.value = crop.value && { ...crop.value };
  } else if (prev === "crop") {
    pendingCrop.value = null;
  }
  if (canvas.value) canvas.value.style.cursor = toolCursor();
});

watch(
  () => props.active,
  async (active) => {
    if (!active) return;
    await nextTick();
    if (fitMode.value) fit();
    draw();
  },
);

onMounted(() => {
  window.addEventListener("keydown", onKeyDown);
  window.addEventListener("pointerdown", onWindowPointerDown, true);
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
  window.removeEventListener("pointerdown", onWindowPointerDown, true);
  resizeObserver?.disconnect();
  cancelAnimationFrame(frame);
});
</script>

<template>
  <div class="editor">
    <div class="toolbar" @mousedown="onToolbarMouseDown">
      <div class="tool-group">
        <div class="tool-menu" data-menu="shape">
          <button
            class="tool menu-trigger"
            :class="{ active: tool === shapeTool, open: openMenu === 'shape' }"
            :title="`${currentShapeTool.label} (${currentShapeTool.key}) — more drawing tools`"
            :aria-label="`Drawing tool: ${currentShapeTool.label}`"
            aria-haspopup="menu"
            :aria-expanded="openMenu === 'shape'"
            @click="onShapeButton"
          >
            <Icon :name="shapeTool" />
            <Icon name="chevron" class="chevron" />
          </button>
          <div v-if="openMenu === 'shape'" class="menu-pop" role="menu">
            <button
              v-for="t in SHAPE_TOOLS"
              :key="t.id"
              class="menu-item"
              :class="{ active: tool === t.id }"
              role="menuitemradio"
              :aria-checked="tool === t.id"
              @click="pickTool(t.id)"
            >
              <Icon :name="t.id" />
              <span>{{ t.label }}</span>
              <kbd>{{ t.key }}</kbd>
            </button>
          </div>
        </div>
        <button
          v-for="t in OTHER_TOOLS"
          :key="t.id"
          class="tool"
          :class="{ active: tool === t.id }"
          :title="`${t.label} (${t.key})`"
          :aria-label="t.label"
          @click="pickTool(t.id)"
        >
          <Icon :name="t.id" />
        </button>
      </div>

      <template v-if="tool === 'crop'">
        <span class="divider" />
        <div class="tool-group">
          <button class="primary crop-apply" :disabled="!pendingCrop" title="Apply the crop (Enter)" @click="applyCrop">
            <Icon name="check" />
            Apply
          </button>
          <button class="ghost" title="Leave the crop as it was (Esc)" @click="cancelCrop">Cancel</button>
        </div>
      </template>

      <template v-else>
        <span class="divider" />
        <div class="tool-group">
          <div class="tool-menu" data-menu="color">
            <button
              class="tool menu-trigger"
              :class="{ open: openMenu === 'color' }"
              :title="`Color ${color}`"
              aria-label="Color"
              aria-haspopup="menu"
              :aria-expanded="openMenu === 'color'"
              @click="toggleMenu('color')"
            >
              <span class="swatch-dot" :style="{ background: color }" />
              <Icon name="chevron" class="chevron" />
            </button>
            <div v-if="openMenu === 'color'" class="menu-pop swatch-grid" role="menu">
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
                <input type="color" :value="color" @input="pickColor(($event.target as HTMLInputElement).value, false)" />
              </label>
            </div>
          </div>

          <div class="tool-menu" data-menu="size">
            <button
              class="tool menu-trigger"
              :class="{ open: openMenu === 'size' }"
              :title="`Size: ${SIZES[sizeIndex].label}`"
              aria-label="Size"
              aria-haspopup="menu"
              :aria-expanded="openMenu === 'size'"
              @click="toggleMenu('size')"
            >
              <span class="size-slot">
                <span class="size-dot" :style="{ width: `${4 + sizeIndex * 2.5}px`, height: `${4 + sizeIndex * 2.5}px` }" />
              </span>
              <Icon name="chevron" class="chevron" />
            </button>
            <div v-if="openMenu === 'size'" class="menu-pop" role="menu">
              <button
                v-for="(s, i) in SIZES"
                :key="s.label"
                class="menu-item"
                :class="{ active: sizeIndex === i }"
                role="menuitemradio"
                :aria-checked="sizeIndex === i"
                @click="pickSize(i)"
              >
                <span class="size-slot">
                  <span class="size-dot" :style="{ width: `${4 + i * 2.5}px`, height: `${4 + i * 2.5}px` }" />
                </span>
                <span>{{ s.label }}</span>
                <kbd>{{ i + 1 }}</kbd>
              </button>
            </div>
          </div>

          <div v-if="showOutline" class="tool-menu" data-menu="outline">
            <button
              class="tool menu-trigger"
              :class="{ open: openMenu === 'outline' }"
              :title="`Text outline: ${OUTLINES.find((o) => o.id === textOutline)!.label}`"
              aria-label="Text outline"
              aria-haspopup="menu"
              :aria-expanded="openMenu === 'outline'"
              @click="toggleMenu('outline')"
            >
              <span class="outline-sample" :class="`outline-${textOutline}`" :style="{ color }">A</span>
              <Icon name="chevron" class="chevron" />
            </button>
            <div v-if="openMenu === 'outline'" class="menu-pop" role="menu">
              <span class="menu-label">Text outline</span>
              <button
                v-for="o in OUTLINES"
                :key="o.id"
                class="menu-item"
                :class="{ active: textOutline === o.id }"
                role="menuitemradio"
                :aria-checked="textOutline === o.id"
                @click="pickOutline(o.id)"
              >
                <span class="outline-sample" :class="`outline-${o.id}`" :style="{ color }">A</span>
                <span>{{ o.label }}</span>
              </button>
            </div>
          </div>
        </div>
      </template>

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
        <button v-if="crop" class="ghost" title="Remove the crop" @click="commit(shapes, null); pendingCrop = null">
          Clear crop
        </button>
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
        :style="{ width: `${view.w * cssScale}px`, height: `${view.h * cssScale}px` }"
      >
        <canvas
          ref="canvas"
          :style="{ width: `${view.w * cssScale}px`, height: `${view.h * cssScale}px`, cursor: 'crosshair' }"
          @pointerdown="onPointerDown"
          @pointermove="onPointerMove"
          @pointerup="onPointerUp"
          @pointercancel="onPointerUp"
          @dblclick="onDoubleClick"
        />
        <template v-if="editing">
          <textarea
            ref="textArea"
            v-model="editingText"
            class="text-entry"
            :style="textStyle"
            spellcheck="false"
            placeholder="Text"
            @keydown="onTextKey"
            @blur="finishText"
          />
          <span
            class="text-resize"
            :style="resizeHandleStyle"
            title="Drag up or down to resize the text"
            @pointerdown="onResizeDown"
            @pointermove="onResizeMove"
            @pointerup="onResizeUp"
            @pointercancel="onResizeUp"
          />
        </template>
      </div>
      <div v-if="image" class="zoom-controls">
        <button title="Zoom out" @click="setZoom(zoom / 1.25)">−</button>
        <span>{{ Math.round(zoom * 100) }}%</span>
        <button title="Zoom in" @click="setZoom(zoom * 1.25)">+</button>
        <button :class="{ primary: fitMode }" :title="`Fit (${modKey}0)`" @click="fitMode = true; fit()">Fit</button>
        <button title="Actual pixels" @click="setZoom(1)">1:1</button>
      </div>
    </div>

    <ExportBar ref="exportBar" :capture="capture" :render="renderPng" :ready="!!image" @copy="copy()" />
  </div>
</template>
