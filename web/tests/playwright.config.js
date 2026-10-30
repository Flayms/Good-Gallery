// End-to-end tests of the built page (spec 019). `cargo xtask web-test` builds it first.
import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: ".",
  testMatch: "*.spec.js",
  timeout: 150_000,
  expect: { timeout: 30_000, toHaveScreenshot: { maxDiffPixels: 50 } },
  fullyParallel: false,
  workers: 1,
  reporter: process.env.CI ? [["list"], ["html", { open: "never" }]] : "list",
  snapshotPathTemplate: "{testDir}/snapshots/{arg}{ext}",
  use: {
    baseURL: "http://localhost:8765/",
    viewport: { width: 1200, height: 800 },
    deviceScaleFactor: 1,
    locale: "en-US",
    timezoneId: "UTC",
    // The app follows the browser's light or dark preference (REQ-020-02), and Chromium's default is light. The
    // screenshots are of the dark appearance; the light one has a test of its own.
    colorScheme: "dark",
    screenshot: "only-on-failure",
  },
  projects: [
    {
      name: "chromium",
      use: {
        browserName: "chromium",
        // No GPU on CI: WebGL 2 through SwiftShader, the path of browsers without WebGPU.
        launchOptions: { args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader"] },
      },
    },
  ],
  webServer: {
    command: "node serve.mjs",
    url: "http://localhost:8765/",
    reuseExistingServer: !process.env.CI,
  },
});
