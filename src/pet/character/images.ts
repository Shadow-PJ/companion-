// Your imported characters' pictures, decoded once and kept as ImageBitmaps
// (ready to draw on a canvas). Loaded on first use; forgotten when no longer worn.

import { invoke } from "@tauri-apps/api/core";

const cache = new Map<string, ImageBitmap | "loading" | "missing">();
const listeners = new Set<() => void>();

/** The picture for a character id, or null while it's still loading. */
export function characterImage(id: string): ImageBitmap | null {
  if (!id) return null;
  const hit = cache.get(id);
  if (hit instanceof ImageBitmap) return hit;
  if (!hit) {
    cache.set(id, "loading");
    void load(id);
  }
  return null;
}

async function load(id: string) {
  try {
    // Rust sends the PNG as raw bytes (an ArrayBuffer), no base64 detour.
    const bytes = await invoke<ArrayBuffer>("character_image", { id });
    setCharacterImage(id, await createImageBitmap(new Blob([bytes], { type: "image/png" })));
  } catch {
    cache.set(id, "missing");
  }
}

/** Puts a picture in the cache (also used by the dev gallery and right after an import). */
export function setCharacterImage(id: string, bitmap: ImageBitmap) {
  const old = cache.get(id);
  if (old instanceof ImageBitmap && old !== bitmap) old.close();
  cache.set(id, bitmap);
  listeners.forEach((l) => l());
}

/** Called when a picture finishes loading (still previews redraw then). */
export function onImageLoaded(listener: () => void) {
  listeners.add(listener);
}

/** Frees pictures nobody wears anymore (deleted or swapped characters). */
export function keepOnly(ids: Iterable<string>) {
  const keep = new Set(ids);
  for (const [id, value] of cache) {
    if (keep.has(id)) continue;
    if (value instanceof ImageBitmap) value.close();
    cache.delete(id);
  }
}
