import { existsSync, mkdtempSync, readFileSync, readdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";

test("the alerts page and the header bell work, and nothing asks for Windows notifications", async ({ page }, info) => {
  const errors: string[] = [];
  page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
  page.on("pageerror", (e) => errors.push(e.message));

  await page.goto("/");
  const bell = page.getByRole("link", { name: /^Alerts:/ });
  await expect(bell).toBeVisible();
  await bell.click();
  await expect(page.getByRole("heading", { name: "Alerts", exact: true })).toBeVisible();
  await expect(page.getByText(/only checked while diskdash is running/)).toBeVisible();
  await expect(page.getByText(/never sends Windows notifications/)).toBeVisible();
  // Either everything is fine, or the machine has a real alert; both are designed states.
  await expect(page.getByText("All drives look healthy").or(page.getByRole("list", { name: "Alerts" }))).toBeVisible();
  await page.getByRole("button", { name: "All" }).click();
  await page.getByRole("button", { name: "Resolved" }).click();
  await page.getByRole("button", { name: "Active" }).click();

  const results = await new AxeBuilder({ page }).withRules(["color-contrast"]).analyze();
  expect(results.violations.flatMap((v) => v.nodes.map((n) => n.target.join(" ")))).toEqual([]);
  await page.screenshot({ path: info.outputPath(`alerts-${info.project.name}.png`), fullPage: true });

  const asked = await page.evaluate(() => (typeof Notification === "undefined" ? "unsupported" : Notification.permission));
  expect(["default", "unsupported"]).toContain(asked);
  expect(errors).toEqual([]);
});

test("export: sections show availability, a report downloads, and one can be saved to a folder", async ({ page }, info) => {
  const errors: string[] = [];
  page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
  page.on("pageerror", (e) => errors.push(e.message));
  await page.goto("/");
  await page.getByRole("button", { name: "Export report" }).click();
  const dialog = page.getByRole("dialog", { name: "Export report" });
  await expect(dialog).toBeVisible();
  const summary = dialog.getByLabel("Drive summary");
  await expect(summary).toBeChecked({ timeout: 20_000 });
  // Sections that need a scan or an analysis say why they are not available.
  await expect(dialog.getByText(/Not available:/).first()).toBeVisible();
  const results = await new AxeBuilder({ page }).include('[role="dialog"]').withRules(["color-contrast"]).analyze();
  expect(results.violations.flatMap((v) => v.nodes.map((n) => n.target.join(" ")))).toEqual([]);
  await page.screenshot({ path: info.outputPath(`export-${info.project.name}.png`) });

  // HTML download.
  const [download] = await Promise.all([page.waitForEvent("download"), dialog.getByRole("button", { name: "Download" }).click()]);
  expect(download.suggestedFilename()).toMatch(/^diskdash-report-\d{8}-\d{6}\.html$/);
  const path = await download.path();
  const html = readFileSync(path, "utf8");
  expect(html).toContain("diskdash report");
  expect(html).not.toMatch(/https?:\/\//);
  expect(html).not.toContain("<script");
  await expect(dialog).toBeHidden();

  // Save a JSON report into a folder we made, then clean up.
  const folder = mkdtempSync(join(tmpdir(), "diskdash-report-e2e-"));
  try {
    await page.getByRole("button", { name: "Export report" }).click();
    await expect(dialog.getByLabel("Drive summary")).toBeChecked({ timeout: 20_000 });
    await dialog.getByLabel(/^JSON/).check();
    await dialog.getByText("Save to a folder instead").click();
    await dialog.getByLabel(/A new file is created there/).fill(folder);
    await dialog.getByRole("button", { name: "Save", exact: true }).click();
    await expect(dialog.getByText(/Saved diskdash-report-.*\.json to/)).toBeVisible();
    const files = readdirSync(folder);
    expect(files).toHaveLength(1);
    expect(files[0]).toMatch(/\.json$/);
    const report = JSON.parse(readFileSync(join(folder, files[0]!), "utf8"));
    expect(report.schema_version).toBe(1);
    expect(report.sections.drives.status).toBe("ok");

    // A folder that does not exist is refused with a message and nothing is created.
    await dialog.getByLabel(/A new file is created there/).fill(join(folder, "missing"));
    await dialog.getByRole("button", { name: "Save", exact: true }).click();
    await expect(dialog.getByRole("alert")).toContainText("not a folder that exists");
    expect(existsSync(join(folder, "missing"))).toBe(false);
  } finally {
    rmSync(folder, { recursive: true, force: true });
  }
  // The refused folder above is an expected 422; the browser logs it as a failed resource.
  expect(errors.filter((e) => !e.includes("422"))).toEqual([]);
});
