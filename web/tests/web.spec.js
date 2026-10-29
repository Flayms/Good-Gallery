// End to end in a browser (spec 019): the built page, a fixture folder picked through the folder upload input (the
// path of Firefox and Safari; Chromium's directory picker can't be driven headless), the scan, the gallery, search,
// the viewer and a reload. egui draws into a canvas, so the test clicks by position in a fixed viewport, waits for the
// app's log lines and compares screenshots.
import { expect, test } from "@playwright/test";
import { fileURLToPath } from "node:url";

const LIBRARY = fileURLToPath(new URL("./fixtures/Holiday", import.meta.url));
const PICK_A_FOLDER = { x: 727, y: 456 };
const SEARCH = { x: 555, y: 24 };
// The portrait photo, first in the gallery filtered to Beach.
const PORTRAIT_TILE = { x: 356, y: 218 };

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

test("REQ-019-01 to REQ-019-08, REQ-019-12: pick, scan, browse, search, view and reload", async ({ page, context }) => {
  await context.addInitScript(() => {
    delete window.showDirectoryPicker;
  });
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
    await page.mouse.click(PICK_A_FOLDER.x, PICK_A_FOLDER.y);
    await (await chooser).setFiles(LIBRARY);
    await log.waitFor("scan of library 1 ended: Completed");
    await log.waitFor("thumbnails are up to date", log.mark() - 1);
    await settle();
    // The sidebar has the folder, Anna, Italy, a five-star photo and the Beach tag; the gallery the four media by
    // date, the video with its duration; `@eaDir` is skipped.
    await expect(page).toHaveScreenshot("gallery.png");
  });

  await test.step("REQ-019-06: search by tag", async () => {
    await page.mouse.click(SEARCH.x, SEARCH.y);
    await page.keyboard.type("Beach");
    await page.keyboard.press("Enter");
    await settle();
    await expect(page).toHaveScreenshot("search.png");
    await page.keyboard.press("Escape");
  });

  await test.step("REQ-019-07: the viewer shows a photo in original quality", async () => {
    await page.mouse.click(PORTRAIT_TILE.x, PORTRAIT_TILE.y);
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
