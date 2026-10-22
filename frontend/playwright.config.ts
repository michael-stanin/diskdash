import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { defineConfig } from "@playwright/test";

const PORT = 8799;
// Isolated data dir: the e2e run never touches the real %LOCALAPPDATA%\diskdash.
const dataDir = mkdtempSync(join(tmpdir(), "diskdash-e2e-"));

export default defineConfig({
  testDir: "./e2e",
  // The light and dark projects share one server and data directory, so tests run one at a time.
  workers: 1,
  outputDir: "./test-results",
  reporter: [["list"], ["html", { open: "never", outputFolder: "playwright-report" }], ...(process.env.CI ? [["github"] as const] : [])],
  use: {
    baseURL: `http://127.0.0.1:${PORT}`,
    // Use the Edge that ships with Windows instead of downloading a browser.
    channel: process.env.PW_CHANNEL ?? "msedge",
    viewport: { width: 1280, height: 720 },
  },
  projects: [
    { name: "light", use: { colorScheme: "light" } },
    { name: "dark", use: { colorScheme: "dark" } },
  ],
  webServer: {
    command: `cargo run --quiet -p diskdash -- --port ${PORT}`,
    cwd: "..",
    url: `http://127.0.0.1:${PORT}/api/v1/health`,
    timeout: 300_000,
    reuseExistingServer: false,
    env: { DISKDASH_DATA_DIR: dataDir },
  },
});
