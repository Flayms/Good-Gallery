// End to end in a browser (spec 019): the built page, a fixture folder picked through the folder upload input (the
// path of Firefox and Safari; Chromium's directory picker can't be driven headless), the scan, the gallery, search,
// the viewer and a reload. egui draws into a canvas, so the test clicks by position in a fixed viewport, waits for the
// app's log lines and compares screenshots.
import { expect, test } from "@playwright/test";
import { cpSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const LIBRARY = fileURLToPath(new URL("./fixtures/Holiday", import.meta.url));
const PICK_A_FOLDER = { x: 727, y: 456 };
const SEARCH = { x: 555, y: 24 };
// The portrait photo, first in the gallery filtered to Beach.
const PORTRAIT_TILE = { x: 356, y: 218 };
const SETTINGS = { x: 65, y: 774 };
const ALL_LIBRARIES = { x: 75, y: 116 };
const ADD_FOLDER = { x: 428, y: 153 };
const REMOVE_LIBRARY = { x: 1061, y: 228 };
const CONFIRM_REMOVE = { x: 762, y: 449 };

/** Clicks like a person: egui sees the pointer arrive before the press, as it does between frames. */
async function click(page, point) {
  await page.mouse.move(point.x, point.y);
  await page.waitForTimeout(100);
  await page.mouse.down();
  await page.mouse.up();
}

/** The files below `dir` in the page's storage (Origin Private File System). */
function storedFiles(page) {
  return page.evaluate(async () => {
    const files = [];
    async function walk(dir, prefix) {
      for await (const [name, handle] of dir.entries()) {
        if (handle.kind === "directory") await walk(handle, `${prefix}${name}/`);
        else files.push(`${prefix}${name}`);
      }
    }
    await walk(await navigator.storage.getDirectory(), "");
    return files;
  });
}

/** The app's console log, and a way to wait for a line in it after a given point. */
function watchLog(page) {
  const lines = [];
  page.on("console", (message) => lines.push(message.text()));
  page.on("pageerror", (error) => lines.push(`pageerror: ${error.message}`));
  return {
    lines,
    mark: () => lines.length,
    async waitFor(text, from = 0) {
      await expect
        .poll(() => lines.slice(from).some((line) => line.includes(text)), { timeout: 60_000, message: text })
        .toBe(true);
    },
  };
}

test("REQ-019-01 to REQ-019-13: pick, scan, browse, search, view, reload, pick again, rescan and remove", async ({ page, context }) => {
  await context.addInitScript(() => {
    delete window.showDirectoryPicker;
  });
  // Settings show when a library was last indexed: the same time on every run.
  await page.clock.setFixedTime(new Date("2026-01-15T10:00:00Z"));
  const requests = [];
  page.on("request", (request) => requests.push(request));
  const log = watchLog(page);
  const settle = () => page.waitForTimeout(1500);

  await test.step("REQ-019-01: the page starts with the empty state", async () => {
    await page.goto("/");
    await log.waitFor("event handlers installed");
    await settle();
    await expect(page.locator("#problem")).toBeHidden();
    await expect(page).toHaveScreenshot("empty.png");
  });

  await test.step("REQ-019-02, REQ-019-03, REQ-019-04, REQ-019-05: a picked folder is scanned with metadata and thumbnails", async () => {
    const chooser = page.waitForEvent("filechooser");
    await click(page, PICK_A_FOLDER);
    await (await chooser).setFiles(LIBRARY);
    await log.waitFor("scan of library 1 ended: Completed");
    await log.waitFor("thumbnails are up to date", log.mark() - 1);
    await settle();
    // The sidebar has the folder, Anna, Italy, a five-star photo and the Beach tag; the gallery the four media by
    // date, the video with its duration; `@eaDir` is skipped.
    await expect(page).toHaveScreenshot("gallery.png");
  });

  await test.step("REQ-019-06: search by tag", async () => {
    await click(page, SEARCH);
    await page.keyboard.type("Beach");
    // The tag suggestions load in the background; Enter before they're there searches file names instead.
    await settle();
    await page.keyboard.press("Enter");
    await settle();
    await expect(page).toHaveScreenshot("search.png");
    await page.keyboard.press("Escape");
  });

  await test.step("REQ-019-07: the viewer shows a photo in original quality", async () => {
    await click(page, PORTRAIT_TILE);
    await settle();
    await expect(page).toHaveScreenshot("viewer.png");
    await page.keyboard.press("Escape");
  });

  await test.step("REQ-019-08: after a reload the gallery shows from the browser's storage", async () => {
    const from = log.mark();
    await page.reload();
    await log.waitFor("event handlers installed", from);
    await settle();
    await expect(page).toHaveScreenshot("reload.png");
  });

  await test.step("REQ-019-09, REQ-019-13: without access the library needs access; desktop-only settings are hidden", async () => {
    // The upload input's files were for the last visit only.
    await click(page, SETTINGS);
    await settle();
    await expect(page).toHaveScreenshot("settings-after-reload.png");
  });

  await test.step("REQ-019-09, REQ-019-10: picking the folder again gives access back, and the rescan finds changes", async () => {
    // The same folder by name, since changed: the video deleted, a photo added.
    const copy = join(mkdtempSync(join(tmpdir(), "gg-web-")), "Holiday");
    cpSync(LIBRARY, copy, { recursive: true });
    rmSync(join(copy, "walk.mp4"));
    cpSync(join(copy, "garden.png"), join(copy, "Beach", "garden copy.png"));
    try {
      const from = log.mark();
      const chooser = page.waitForEvent("filechooser");
      await click(page, ADD_FOLDER);
      await (await chooser).setFiles(copy);
      // "Holiday is readable again" shows for 4 s (toasts::SHOWN_FOR); on a fast machine the scan ends sooner.
      const toastGone = page.waitForTimeout(4500);
      await log.waitFor("scan of library 1 ended: Completed", from);
      await log.waitFor("thumbnails are up to date", from);
      await toastGone;
      await settle();
      await expect(page).toHaveScreenshot("settings-after-picking-again.png");
      await click(page, ALL_LIBRARIES);
      await settle();
      await expect(page).toHaveScreenshot("gallery-after-rescan.png");
    } finally {
      rmSync(join(copy, ".."), { recursive: true, force: true });
    }
  });

  await test.step("REQ-019-11: removing the library deletes its index entries and thumbnails", async () => {
    expect((await storedFiles(page)).some((file) => file.startsWith("thumbs/1/"))).toBe(true);
    await click(page, SETTINGS);
    await settle();
    await click(page, REMOVE_LIBRARY);
    await settle();
    await click(page, CONFIRM_REMOVE);
    await settle();
    await expect.poll(async () => (await storedFiles(page)).filter((file) => file.startsWith("thumbs/1/"))).toEqual([]);
    await settle();
    await expect(page).toHaveScreenshot("settings-after-removing.png");
  });

  await test.step("REQ-019-12: only the page's own files were requested", async () => {
    const origin = new URL(page.url()).origin;
    const foreign = requests
      .filter((request) => !request.url().startsWith("blob:") && !request.url().startsWith("data:"))
      .filter((request) => request.method() !== "GET" || new URL(request.url()).origin !== origin)
      .map((request) => `${request.method()} ${request.url()}`);
    expect(foreign).toEqual([]);
    expect(log.lines.filter((line) => line.startsWith("pageerror") || line.includes("ERROR"))).toEqual([]);
  });
});

test("REQ-019-01: a browser without what the page needs is told which browsers work", async ({ page, context }) => {
  await context.addInitScript(() => {
    Object.defineProperty(navigator, "storage", { value: undefined });
  });
  await page.goto("/");
  await expect(page.locator("#problem")).toBeVisible();
  await expect(page.locator("#problem")).toContainText("storage for the index");
  await expect(page.locator("#problem")).toContainText("Chrome, Edge, Firefox and Safari");
});

test("REQ-020-02: in a browser that prefers light, the page and the app are light", async ({ page }) => {
  await page.emulateMedia({ colorScheme: "light" });
  const log = watchLog(page);
  await page.goto("/");
  await expect(page.locator("body")).toHaveCSS("background-color", "rgb(244, 244, 244)");
  await log.waitFor("event handlers installed");
  await page.waitForTimeout(1500);
  await expect(page).toHaveScreenshot("empty-light.png");
});
