import type { ExportFormat } from "./types";

export const FORMAT_INFO: Record<ExportFormat, { label: string; color: string; note: string }> = {
  png: { label: "PNG", color: "#2f86d6", note: "Lossless, best for screenshots" },
  jpg: { label: "JPG", color: "#d9822b", note: "Small files, lossy" },
  heic: { label: "HEIC", color: "#9b59d0", note: "Small files, high quality" },
  tiff: { label: "TIFF", color: "#3c9d7a", note: "Lossless, large files" },
  gif: { label: "GIF", color: "#c94f7c", note: "256 colors" },
  bmp: { label: "BMP", color: "#6b7a90", note: "Uncompressed" },
  pdf: { label: "PDF", color: "#d1434b", note: "Single-page document" },
};

export const ALL_FORMATS: ExportFormat[] = ["png", "jpg", "heic", "tiff", "gif", "bmp", "pdf"];
