// Browser services for the web build (spec 019), called from `web::js`. Everything here works on files the user picked
// and on the page's own storage; nothing is sent anywhere (REQ-019-12).

// --- Origin Private File System: the index snapshot and the thumbnail cache ---

async function parentOf(path, create) {
  let dir = await navigator.storage.getDirectory();
  const parts = path.split("/");
  const name = parts.pop();
  for (const part of parts) {
    dir = await dir.getDirectoryHandle(part, { create });
  }
  return [dir, name];
}

function isMissing(error) {
  return error && (error.name === "NotFoundError" || error.name === "TypeMismatchError");
}

/** The file at `path` in the page's storage, or null. */
export async function opfsFile(path) {
  try {
    const [dir, name] = await parentOf(path, false);
    return await (await dir.getFileHandle(name)).getFile();
  } catch (error) {
    if (isMissing(error)) return null;
    throw error;
  }
}

/** The bytes at `path`, or null. */
export async function opfsRead(path) {
  const file = await opfsFile(path);
  return file ? new Uint8Array(await file.arrayBuffer()) : null;
}

/** Writes `data` (bytes or a Blob) to `path`, replacing it; through a temporary file, so a reader never sees half. */
export async function opfsWrite(path, data) {
  const [dir, name] = await parentOf(path, true);
  const handle = await dir.getFileHandle(name, { create: true });
  const writable = await handle.createWritable();
  try {
    await writable.write(data);
    await writable.close();
  } catch (error) {
    await writable.abort().catch(() => {});
    throw error;
  }
}

/** Removes the file or folder at `path` with everything in it; a missing one is fine. */
export async function opfsRemove(path) {
  try {
    const [dir, name] = await parentOf(path, false);
    await dir.removeEntry(name, { recursive: true });
  } catch (error) {
    if (!isMissing(error)) throw error;
  }
}

/** Every file below the folder `path`: `[relativePath, size, lastModified]`. */
export async function opfsList(path) {
  const found = [];
  let dir = await navigator.storage.getDirectory();
  try {
    for (const part of path.split("/")) dir = await dir.getDirectoryHandle(part);
  } catch (error) {
    if (isMissing(error)) return found;
    throw error;
  }
  async function walk(dir, prefix) {
    for await (const [name, handle] of dir.entries()) {
      if (handle.kind === "directory") {
        await walk(handle, prefix + name + "/");
      } else {
        const file = await handle.getFile();
        found.push([prefix + name, file.size, file.lastModified]);
      }
    }
  }
  await walk(dir, "");
  return found;
}

/** Asks the browser to keep the page's storage when space runs low (REQ-019-08). */
export async function persistStorage() {
  try {
    if (navigator.storage && navigator.storage.persist) {
      return (await navigator.storage.persisted()) || (await navigator.storage.persist());
    }
  } catch (_) {
    // Not offered: storage stays best-effort.
  }
  return false;
}

// --- Picking folders (REQ-019-02) ---

/** Whether the browser can remember a picked folder between visits (File System Access API). */
export function canKeepFolders() {
  return typeof window.showDirectoryPicker === "function";
}

/** The folder picker with read access: a directory handle, or null when cancelled. */
export async function pickDirectory() {
  try {
    return await window.showDirectoryPicker({ id: "good-gallery", mode: "read" });
  } catch (error) {
    if (error && error.name === "AbortError") return null;
    throw error;
  }
}

/** The folder upload input (Firefox, Safari): `{ name, files }` with paths relative to the folder, or null. */
export function pickFiles() {
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.webkitdirectory = true;
    input.multiple = true;
    input.style.display = "none";
    let settled = false;
    const done = (value) => {
      if (settled) return;
      settled = true;
      input.remove();
      resolve(value);
    };
    input.addEventListener("change", () => {
      const files = Array.from(input.files || []);
      if (files.length === 0) return done(null);
      const name = files[0].webkitRelativePath.split("/")[0];
      const entries = files.map((file) => [file.webkitRelativePath.split("/").slice(1).join("/"), file]);
      done({ name, files: entries });
    });
    input.addEventListener("cancel", () => done(null));
    document.body.appendChild(input);
    input.click();
  });
}

/** A folder's entries: `[name, file]` for files, `[name, null, handle]` for subfolders. */
export async function listDirectory(dir) {
  const entries = [];
  for await (const [name, handle] of dir.entries()) {
    if (handle.kind === "directory") {
      entries.push([name, null, handle]);
    } else {
      try {
        entries.push([name, await handle.getFile(), null]);
      } catch (_) {
        // Vanished or unreadable: skipped like a file that can't be read.
      }
    }
  }
  return entries;
}

/** The file at the `/`-separated `path` below `dir`, or null. */
export async function fileAt(dir, path) {
  try {
    const parts = path.split("/");
    const name = parts.pop();
    for (const part of parts) dir = await dir.getDirectoryHandle(part);
    return await (await dir.getFileHandle(name)).getFile();
  } catch (error) {
    if (isMissing(error)) return null;
    throw error;
  }
}

/** Whether the page may still read `dir`: "granted", "prompt" or "denied". */
export async function folderPermission(dir) {
  return await dir.queryPermission({ mode: "read" });
}

/** Asks for read access to `dir` again (REQ-019-09); needs a click. */
export async function requestFolderPermission(dir) {
  try {
    return (await dir.requestPermission({ mode: "read" })) === "granted";
  } catch (_) {
    return false;
  }
}

// --- Picked folders kept in IndexedDB (directory handles can't go into the index) ---

function folderStore(mode) {
  return new Promise((resolve, reject) => {
    const open = indexedDB.open("good-gallery", 1);
    open.onupgradeneeded = () => open.result.createObjectStore("folders");
    open.onerror = () => reject(open.error);
    open.onsuccess = () => resolve(open.result.transaction("folders", mode).objectStore("folders"));
  });
}

function request(req) {
  return new Promise((resolve, reject) => {
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

export async function saveFolder(key, dir) {
  await request((await folderStore("readwrite")).put(dir, key));
}

export async function loadFolder(key) {
  return (await request((await folderStore("readonly")).get(key))) || null;
}

export async function deleteFolder(key) {
  await request((await folderStore("readwrite")).delete(key));
}

// --- Images ---

/** Decodes an image file or blob, upright by its EXIF orientation. */
export async function decodeImage(blob) {
  return await createImageBitmap(blob, { imageOrientation: "from-image" });
}

function canvasOf(width, height) {
  const canvas = new OffscreenCanvas(width, height);
  const context = canvas.getContext("2d", { willReadFrequently: true });
  context.imageSmoothingEnabled = true;
  context.imageSmoothingQuality = "high";
  return [canvas, context];
}

/** `source` (an ImageBitmap or a video) scaled to `width` × `height`, as RGBA bytes. */
export function rgbaOf(source, width, height) {
  const [, context] = canvasOf(width, height);
  context.drawImage(source, 0, 0, width, height);
  return new Uint8Array(context.getImageData(0, 0, width, height).data.buffer);
}

/** `bitmap` scaled to `width` × `height` and encoded as WebP (JPEG where the browser can't encode WebP). */
export async function encodeImage(bitmap, width, height, quality) {
  const [canvas, context] = canvasOf(width, height);
  context.drawImage(bitmap, 0, 0, width, height);
  let blob = await canvas.convertToBlob({ type: "image/webp", quality });
  if (blob.type !== "image/webp") {
    blob = await canvas.convertToBlob({ type: "image/jpeg", quality });
  }
  return new Uint8Array(await blob.arrayBuffer());
}

/** The stored thumbnail at `path` decoded to RGBA: `[width, height, bytes]`, or null if it isn't stored. */
export async function storedRgba(path) {
  const file = await opfsFile(path);
  if (!file) return null;
  const bitmap = await createImageBitmap(file);
  try {
    return [bitmap.width, bitmap.height, rgbaOf(bitmap, bitmap.width, bitmap.height)];
  } finally {
    bitmap.close();
  }
}

// --- Videos ---

function loadVideo(file) {
  const video = document.createElement("video");
  video.muted = true;
  video.playsInline = true;
  video.preload = "auto";
  const url = URL.createObjectURL(file);
  video.src = url;
  return [video, url];
}

function once(target, ok, timeoutMs) {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("the video took too long")), timeoutMs);
    target.addEventListener(ok, () => { clearTimeout(timer); resolve(); }, { once: true });
    target.addEventListener("error", () => {
      clearTimeout(timer);
      reject(new Error((target.error && target.error.message) || "the browser can't play this video"));
    }, { once: true });
  });
}

/** The frame at `fraction` of the video's duration: `[bitmap, durationSeconds, width, height]`. */
export async function videoFrame(file, fraction) {
  const [video, url] = loadVideo(file);
  try {
    await once(video, "loadedmetadata", 20000);
    const duration = Number.isFinite(video.duration) ? video.duration : 0;
    video.currentTime = duration * fraction;
    await once(video, "seeked", 20000);
    const bitmap = await createImageBitmap(video);
    return [bitmap, duration, video.videoWidth, video.videoHeight];
  } finally {
    video.removeAttribute("src");
    video.load();
    URL.revokeObjectURL(url);
  }
}

/** A `<video>` element playing `file` for the viewer (REQ-019-07); `player.stop()` frees it. */
export class Player {
  constructor() {
    this.video = document.createElement("video");
    this.video.playsInline = true;
    this.video.preload = "auto";
    this.url = null;
    this.error = null;
    this.video.addEventListener("error", () => {
      this.error = (this.video.error && this.video.error.message) || "The browser can't play this video.";
    });
  }
  load(file) {
    this.stop();
    this.error = null;
    this.url = URL.createObjectURL(file);
    this.video.src = this.url;
    this.video.play().catch(() => {});
  }
  fail(message) {
    this.stop();
    this.error = message;
  }
  stop() {
    this.video.pause();
    this.video.removeAttribute("src");
    this.video.load();
    if (this.url) URL.revokeObjectURL(this.url);
    this.url = null;
  }
  setPaused(paused) {
    if (paused) this.video.pause();
    else this.video.play().catch(() => {});
  }
  seek(position, exact) {
    if (!exact && typeof this.video.fastSeek === "function") this.video.fastSeek(position);
    else this.video.currentTime = position;
  }
  setVolume(volume) {
    this.video.volume = Math.min(1, Math.max(0, volume / 100));
  }
  setMuted(muted) {
    this.video.muted = muted;
  }
  /** `[paused, position, duration, width, height, ended, volume, muted, ready, error]` */
  state() {
    const v = this.video;
    return [
      v.paused,
      v.currentTime || 0,
      Number.isFinite(v.duration) ? v.duration : 0,
      v.videoWidth,
      v.videoHeight,
      v.ended,
      v.volume * 100,
      v.muted,
      v.readyState >= 2,
      this.error,
    ];
  }
}

// --- The page ---

/** Holds the page's lock while it runs: false if another tab already has the page open. */
export function holdTabLock() {
  if (!navigator.locks) return Promise.resolve(true);
  return new Promise((resolve) => {
    navigator.locks.request("good-gallery", { ifAvailable: true }, (lock) => {
      resolve(lock !== null);
      return lock ? new Promise(() => {}) : null;
    });
  });
}

export function prefersReducedMotion() {
  return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

export function prefersReducedTransparency() {
  return window.matchMedia("(prefers-reduced-transparency: reduce)").matches;
}

/** Calls `save` when the page is hidden or closed, so the index is kept (REQ-019-08). */
export function onPageHidden(save) {
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "hidden") save();
  });
  window.addEventListener("pagehide", () => save());
}
