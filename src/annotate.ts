/**
 * The annotation model. Every shape is stored in image pixels so the editor
 * can zoom freely and the export renders at the capture's full resolution.
 */

export type Tool =
  | "select"
  | "arrow"
  | "line"
  | "rect"
  | "ellipse"
  | "pen"
  | "highlighter"
  | "text"
  | "step"
  | "blur"
  | "crop";

interface Base {
  id: number;
}

export interface SegmentShape extends Base {
  kind: "arrow" | "line" | "rect" | "ellipse" | "blur";
  color: string;
  width: number;
  x1: number;
  y1: number;
  x2: number;
  y2: number;
}

export interface StrokeShape extends Base {
  kind: "pen" | "highlighter";
  color: string;
  width: number;
  points: number[];
}

export interface TextShape extends Base {
  kind: "text";
  color: string;
  size: number;
  x: number;
  y: number;
  text: string;
}

export interface StepShape extends Base {
  kind: "step";
  color: string;
  size: number;
  x: number;
  y: number;
  n: number;
}

export type Shape = SegmentShape | StrokeShape | TextShape | StepShape;

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export const TEXT_FONT = 'Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif';
export const TEXT_LINE_HEIGHT = 1.2;

let nextId = 1;
export const newId = () => nextId++;

export function normalize(x1: number, y1: number, x2: number, y2: number): Rect {
  return { x: Math.min(x1, x2), y: Math.min(y1, y2), w: Math.abs(x2 - x1), h: Math.abs(y2 - y1) };
}

/** Black or white, whichever reads better on top of `color`. */
export function contrastColor(color: string) {
  const hex = color.replace("#", "");
  const full = hex.length === 3 ? hex.split("").map((c) => c + c).join("") : hex.slice(0, 6);
  const n = parseInt(full, 16);
  if (Number.isNaN(n)) return "#000";
  const [r, g, b] = [(n >> 16) & 255, (n >> 8) & 255, n & 255];
  return 0.299 * r + 0.587 * g + 0.114 * b > 160 ? "#000" : "#fff";
}

const measureCtx = document.createElement("canvas").getContext("2d")!;

function textLines(shape: TextShape) {
  return shape.text.split("\n");
}

export function textBounds(shape: TextShape): Rect {
  measureCtx.font = `700 ${shape.size}px ${TEXT_FONT}`;
  const lines = textLines(shape);
  const w = Math.max(...lines.map((l) => measureCtx.measureText(l || " ").width));
  return { x: shape.x, y: shape.y, w, h: lines.length * shape.size * TEXT_LINE_HEIGHT };
}

export function bounds(shape: Shape): Rect {
  switch (shape.kind) {
    case "pen":
    case "highlighter": {
      let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
      for (let i = 0; i < shape.points.length; i += 2) {
        minX = Math.min(minX, shape.points[i]);
        maxX = Math.max(maxX, shape.points[i]);
        minY = Math.min(minY, shape.points[i + 1]);
        maxY = Math.max(maxY, shape.points[i + 1]);
      }
      const pad = shape.kind === "highlighter" ? shape.width * 2 : shape.width / 2;
      return { x: minX - pad, y: minY - pad, w: maxX - minX + pad * 2, h: maxY - minY + pad * 2 };
    }
    case "text":
      return textBounds(shape);
    case "step":
      return { x: shape.x - shape.size, y: shape.y - shape.size, w: shape.size * 2, h: shape.size * 2 };
    default: {
      const r = normalize(shape.x1, shape.y1, shape.x2, shape.y2);
      const pad = shape.kind === "blur" ? 0 : shape.width / 2;
      return { x: r.x - pad, y: r.y - pad, w: r.w + pad * 2, h: r.h + pad * 2 };
    }
  }
}

function distToSegment(px: number, py: number, x1: number, y1: number, x2: number, y2: number) {
  const dx = x2 - x1;
  const dy = y2 - y1;
  const len = dx * dx + dy * dy;
  const t = len ? Math.max(0, Math.min(1, ((px - x1) * dx + (py - y1) * dy) / len)) : 0;
  return Math.hypot(px - (x1 + t * dx), py - (y1 + t * dy));
}

/** Whether a click at (x, y) lands on the shape, within `slop` pixels. */
export function hitTest(shape: Shape, x: number, y: number, slop: number) {
  switch (shape.kind) {
    case "arrow":
    case "line":
      return distToSegment(x, y, shape.x1, shape.y1, shape.x2, shape.y2) <= shape.width / 2 + slop;
    case "rect": {
      const r = normalize(shape.x1, shape.y1, shape.x2, shape.y2);
      const edge = shape.width / 2 + slop;
      const inside = x >= r.x - edge && x <= r.x + r.w + edge && y >= r.y - edge && y <= r.y + r.h + edge;
      const deep = x > r.x + edge && x < r.x + r.w - edge && y > r.y + edge && y < r.y + r.h - edge;
      return inside && !deep;
    }
    case "ellipse": {
      const r = normalize(shape.x1, shape.y1, shape.x2, shape.y2);
      const rx = r.w / 2, ry = r.h / 2;
      if (rx < 1 || ry < 1) return false;
      const nx = (x - (r.x + rx)) / rx, ny = (y - (r.y + ry)) / ry;
      const d = Math.sqrt(nx * nx + ny * ny);
      return Math.abs(d - 1) * Math.min(rx, ry) <= shape.width / 2 + slop;
    }
    case "pen":
    case "highlighter": {
      const reach = (shape.kind === "highlighter" ? shape.width * 2 : shape.width / 2) + slop;
      const p = shape.points;
      if (p.length === 2) return Math.hypot(x - p[0], y - p[1]) <= reach;
      for (let i = 2; i < p.length; i += 2) {
        if (distToSegment(x, y, p[i - 2], p[i - 1], p[i], p[i + 1]) <= reach) return true;
      }
      return false;
    }
    default: {
      const b = bounds(shape);
      return x >= b.x - slop && x <= b.x + b.w + slop && y >= b.y - slop && y <= b.y + b.h + slop;
    }
  }
}

export function translate(shape: Shape, dx: number, dy: number): Shape {
  switch (shape.kind) {
    case "pen":
    case "highlighter":
      return { ...shape, points: shape.points.map((v, i) => v + (i % 2 ? dy : dx)) };
    case "text":
    case "step":
      return { ...shape, x: shape.x + dx, y: shape.y + dy };
    default:
      return { ...shape, x1: shape.x1 + dx, y1: shape.y1 + dy, x2: shape.x2 + dx, y2: shape.y2 + dy };
  }
}

function drawArrow(ctx: CanvasRenderingContext2D, s: SegmentShape) {
  const angle = Math.atan2(s.y2 - s.y1, s.x2 - s.x1);
  const length = Math.hypot(s.x2 - s.x1, s.y2 - s.y1);
  const head = Math.min(Math.max(s.width * 4, 12), length * 0.6 || 12);
  const spread = Math.PI / 7;
  /* Stop the shaft inside the head so its square end never pokes out. */
  const shaftEnd = Math.max(0, length - head * 0.8);
  ctx.beginPath();
  ctx.moveTo(s.x1, s.y1);
  ctx.lineTo(s.x1 + Math.cos(angle) * shaftEnd, s.y1 + Math.sin(angle) * shaftEnd);
  ctx.stroke();
  ctx.beginPath();
  ctx.moveTo(s.x2, s.y2);
  ctx.lineTo(s.x2 - head * Math.cos(angle - spread), s.y2 - head * Math.sin(angle - spread));
  ctx.lineTo(s.x2 - head * Math.cos(angle + spread), s.y2 - head * Math.sin(angle + spread));
  ctx.closePath();
  ctx.fill();
}

function strokePoints(ctx: CanvasRenderingContext2D, points: number[]) {
  ctx.beginPath();
  ctx.moveTo(points[0], points[1]);
  if (points.length === 2) {
    ctx.lineTo(points[0] + 0.01, points[1]);
  }
  /* Midpoint quadratic smoothing keeps fast strokes from looking jagged. */
  for (let i = 2; i < points.length - 2; i += 2) {
    const mx = (points[i] + points[i + 2]) / 2;
    const my = (points[i + 1] + points[i + 3]) / 2;
    ctx.quadraticCurveTo(points[i], points[i + 1], mx, my);
  }
  if (points.length > 2) {
    ctx.lineTo(points[points.length - 2], points[points.length - 1]);
  }
  ctx.stroke();
}

const pixelCanvas = document.createElement("canvas");

function drawBlur(ctx: CanvasRenderingContext2D, s: SegmentShape, source: CanvasImageSource) {
  const r = normalize(s.x1, s.y1, s.x2, s.y2);
  if (r.w < 1 || r.h < 1) return;
  /* Blocks need to be at least a line of text tall to actually hide it. */
  const block = Math.max(10, s.width * 4.5);
  const w = Math.max(1, Math.round(r.w / block));
  const h = Math.max(1, Math.round(r.h / block));
  pixelCanvas.width = w;
  pixelCanvas.height = h;
  const pctx = pixelCanvas.getContext("2d")!;
  pctx.imageSmoothingEnabled = true;
  pctx.drawImage(source, r.x, r.y, r.w, r.h, 0, 0, w, h);
  ctx.save();
  ctx.imageSmoothingEnabled = false;
  ctx.drawImage(pixelCanvas, 0, 0, w, h, r.x, r.y, r.w, r.h);
  ctx.restore();
}

/**
 * Draw one shape. `source` is the untouched screenshot, which the pixelate
 * tool samples so redaction never picks up other annotations.
 */
export function drawShape(ctx: CanvasRenderingContext2D, shape: Shape, source: CanvasImageSource) {
  ctx.save();
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  switch (shape.kind) {
    case "arrow":
      ctx.strokeStyle = ctx.fillStyle = shape.color;
      ctx.lineWidth = shape.width;
      ctx.lineCap = "butt";
      drawArrow(ctx, shape);
      break;
    case "line":
      ctx.strokeStyle = shape.color;
      ctx.lineWidth = shape.width;
      ctx.beginPath();
      ctx.moveTo(shape.x1, shape.y1);
      ctx.lineTo(shape.x2, shape.y2);
      ctx.stroke();
      break;
    case "rect": {
      const r = normalize(shape.x1, shape.y1, shape.x2, shape.y2);
      ctx.strokeStyle = shape.color;
      ctx.lineWidth = shape.width;
      ctx.beginPath();
      ctx.roundRect(r.x, r.y, r.w, r.h, Math.min(shape.width, r.w / 2, r.h / 2));
      ctx.stroke();
      break;
    }
    case "ellipse": {
      const r = normalize(shape.x1, shape.y1, shape.x2, shape.y2);
      ctx.strokeStyle = shape.color;
      ctx.lineWidth = shape.width;
      ctx.beginPath();
      ctx.ellipse(r.x + r.w / 2, r.y + r.h / 2, r.w / 2, r.h / 2, 0, 0, Math.PI * 2);
      ctx.stroke();
      break;
    }
    case "pen":
      ctx.strokeStyle = shape.color;
      ctx.lineWidth = shape.width;
      strokePoints(ctx, shape.points);
      break;
    case "highlighter":
      ctx.globalAlpha = 0.38;
      ctx.globalCompositeOperation = "multiply";
      ctx.strokeStyle = shape.color;
      ctx.lineWidth = shape.width * 4;
      ctx.lineCap = "square";
      strokePoints(ctx, shape.points);
      break;
    case "blur":
      drawBlur(ctx, shape, source);
      break;
    case "text": {
      ctx.font = `700 ${shape.size}px ${TEXT_FONT}`;
      ctx.textBaseline = "top";
      ctx.lineWidth = Math.max(2, shape.size / 7);
      ctx.strokeStyle = contrastColor(shape.color);
      ctx.fillStyle = shape.color;
      textLines(shape).forEach((line, i) => {
        const y = shape.y + i * shape.size * TEXT_LINE_HEIGHT + shape.size * 0.1;
        ctx.strokeText(line, shape.x, y);
        ctx.fillText(line, shape.x, y);
      });
      break;
    }
    case "step": {
      ctx.fillStyle = shape.color;
      ctx.shadowColor = "rgb(0 0 0 / 0.35)";
      ctx.shadowBlur = shape.size / 3;
      ctx.shadowOffsetY = shape.size / 10;
      ctx.beginPath();
      ctx.arc(shape.x, shape.y, shape.size, 0, Math.PI * 2);
      ctx.fill();
      ctx.shadowColor = "transparent";
      ctx.lineWidth = Math.max(2, shape.size / 8);
      ctx.strokeStyle = "#fff";
      ctx.stroke();
      ctx.fillStyle = contrastColor(shape.color);
      ctx.font = `800 ${shape.size * 1.05}px ${TEXT_FONT}`;
      ctx.textAlign = "center";
      ctx.textBaseline = "middle";
      ctx.fillText(String(shape.n), shape.x, shape.y + shape.size * 0.05);
      break;
    }
  }
  ctx.restore();
}

/** Render the screenshot with every annotation, cropped, at full resolution. */
export function renderFinal(source: ImageBitmap, shapes: Shape[], crop: Rect | null) {
  const full = document.createElement("canvas");
  full.width = source.width;
  full.height = source.height;
  const ctx = full.getContext("2d")!;
  ctx.drawImage(source, 0, 0);
  for (const shape of shapes) drawShape(ctx, shape, source);
  if (!crop) return full;
  const out = document.createElement("canvas");
  out.width = Math.max(1, Math.round(crop.w));
  out.height = Math.max(1, Math.round(crop.h));
  out.getContext("2d")!.drawImage(full, crop.x, crop.y, crop.w, crop.h, 0, 0, out.width, out.height);
  return out;
}

export async function canvasToPng(canvas: HTMLCanvasElement) {
  const blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, "image/png"));
  if (!blob) throw new Error("Could not render the image.");
  return new Uint8Array(await blob.arrayBuffer());
}
