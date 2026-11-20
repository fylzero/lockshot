import { convertFileSrc } from "@tauri-apps/api/core";

/**
 * Fetch a capture's raw RGBA pixels from the `capture://` protocol. Raw bytes
 * skip PNG encoding in Rust and decoding here, which made captures slow.
 */
export async function fetchPixels(path: string, width: number, height: number) {
  const response = await fetch(convertFileSrc(path, "capture"));
  if (!response.ok) throw new Error(`Capture image not found (${response.status})`);
  const bytes = new Uint8ClampedArray(await response.arrayBuffer());
  if (bytes.length !== width * height * 4) throw new Error("Capture image has an unexpected size");
  return new ImageData(bytes, width, height);
}

/** The same pixels as an ImageBitmap, ready to draw into canvases. */
export async function loadBitmap(path: string, width: number, height: number) {
  return createImageBitmap(await fetchPixels(path, width, height));
}
