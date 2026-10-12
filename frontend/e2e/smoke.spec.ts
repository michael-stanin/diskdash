import { join } from "node:path";
import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";

function collectErrors(page: Page): string[] {
  const errors: string[] = [];
  page.on("console", (m) => {
    if (m.type() === "error") errors.push(m.text());
  });
  page.on("pageerror", (e) => errors.push(e.message));
  return errors;
}

async function expectNoContrastIssues(page: Page) {
  const results = await new AxeBuilder({ page }).withRules(["color-contrast"]).analyze();
  const summary = results.violations.flatMap((v) =>
    v.nodes.map((n) => `${n.target.join(" ")}: ${n.failureSummary?.split("\n")[1] ?? ""}`),
  );
  expect(summary).toEqual([]);
}

test("shell loads, connects and fits 1280x720", async ({ page }, info) => {
  const errors = collectErrors(page);
  await page.goto("/");
  await expect(page.getByRole("heading", { name: "Overview" })).toBeVisible();
  await expect(page.getByRole("status").filter({ hasText: "Connected" })).toBeVisible({ timeout: 15_000 });
  await expect(page.getByRole("navigation", { name: "Main" })).toBeVisible();

  const { scrollWidth, innerWidth } = await page.evaluate(() => ({
    scrollWidth: document.documentElement.scrollWidth,
    innerWidth: window.innerWidth,
  }));
  expect(scrollWidth).toBeLessThanOrEqual(innerWidth);

  await expectNoContrastIssues(page);
  await page.screenshot({ path: info.outputPath(`shell-${info.project.name}.png`), fullPage: true });
  expect(errors).toEqual([]);
});

test("theme is applied before first paint", async ({ page }, info) => {
  await page.goto("/");
  const dark = await page.evaluate(() => document.documentElement.classList.contains("dark"));
  expect(dark).toBe(info.project.name === "dark");
});

test("component gallery renders without errors", async ({ page }, info) => {
  const errors = collectErrors(page);
  await page.goto("/dev/components");
  await expect(page.getByRole("heading", { name: "Component gallery" })).toBeVisible();
  await expect(page.getByText("2,000 rows", { exact: true })).toBeVisible();
  await expectNoContrastIssues(page);
  await page.screenshot({ path: info.outputPath(`gallery-${info.project.name}.png`), fullPage: true });
  expect(errors).toEqual([]);
});

test("keyboard reaches the sidebar first with a visible focus ring", async ({ page }) => {
  await page.goto("/");
  await page.keyboard.press("Tab"); // skip link
  await page.keyboard.press("Tab");
  const focused = page.locator(":focus");
  await expect(focused).toHaveAttribute("href", "/");
  const outline = await focused.evaluate((el) => getComputedStyle(el).outlineStyle);
  expect(outline).not.toBe("none");
});

test("home shows this machine's drives and the detail page opens", async ({ page }, info) => {
  const errors = collectErrors(page);
  await page.goto("/");
  const cards = page.getByRole("link", { name: /^Details for / });
  await expect(cards.first()).toBeVisible({ timeout: 20_000 });
  await expectNoContrastIssues(page);
  await page.screenshot({ path: info.outputPath(`home-${info.project.name}.png`), fullPage: true });

  await cards.first().click();
  await expect(page.getByRole("navigation", { name: "Breadcrumb" })).toBeVisible();
  await expect(page.getByRole("tab", { name: "Usage" })).toHaveAttribute("aria-selected", "true");
  await page.getByRole("tab", { name: "Hardware & health" }).click();
  await expect(page.getByText("Overall health")).toBeVisible();
  await expectNoContrastIssues(page);
  await page.screenshot({ path: info.outputPath(`drive-${info.project.name}.png`), fullPage: true });
  expect(errors).toEqual([]);
});

test("scan dialog lists drives and validates a folder", async ({ page }, info) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Scan drives" }).click();
  const dialog = page.getByRole("dialog", { name: "Scan drives" });
  await expect(dialog).toBeVisible();
  await expect(dialog.getByRole("checkbox").first()).toBeVisible({ timeout: 20_000 });
  await expectNoContrastIssues(page);
  await page.screenshot({ path: info.outputPath(`scan-dialog-${info.project.name}.png`) });

  // An out-of-scope path is rejected with a field message; nothing is scanned.
  await dialog.getByLabel("Or a single folder").fill("Z:\\nowhere");
  await dialog.getByRole("button", { name: "Start scan" }).click();
  await expect(dialog.getByRole("alert")).toContainText("fixed internal");
  await dialog.getByRole("button", { name: "Cancel" }).click();
  await expect(dialog).toBeHidden();
});

test("a folder scan can be explored: chart, breadcrumb, breakdown, tables", async ({ page }, info) => {
  test.setTimeout(90_000);
  const errors = collectErrors(page);
  await page.goto("/");
  // Scan this project's own source folder (read-only) through the API.
  const root = join(process.cwd(), "src");
  const started = await page.evaluate(async (roots) => {
    const token = document.querySelector<HTMLMetaElement>('meta[name="diskdash-token"]')!.content;
    const r = await fetch("/api/v1/scans", {
      method: "POST",
      headers: { "Content-Type": "application/json", "X-Diskdash-Token": token },
      body: JSON.stringify({ roots }),
    });
    return (await r.json()) as { job_id: number; snapshots: { id: number }[] };
  }, [root]);
  await expect
    .poll(async () => (await (await page.request.get(`/api/v1/jobs/${started.job_id}`)).json()).status, { timeout: 60_000 })
    .toBe("succeeded");

  await page.goto(`/snapshots/${started.snapshots[0]!.id}`);
  await expect(page.getByRole("img", { name: /Space used inside/ })).toBeVisible({ timeout: 20_000 });
  await expect(page.getByRole("navigation", { name: "Folder path" })).toBeVisible();
  await expect(page.getByRole("table", { name: "Largest files" })).toBeVisible();
  await expect(page.getByRole("tab", { name: "Types" })).toBeVisible();
  await page.waitForTimeout(400); // let the chart finish its first paint
  await expectNoContrastIssues(page);
  await page.screenshot({ path: info.outputPath(`snapshot-${info.project.name}.png`), fullPage: true });

  // Drill into the largest folder from the table; the breadcrumb grows.
  await page.getByRole("button", { name: /^Show .* in the chart$/ }).first().click();
  await expect(page.getByRole("navigation", { name: "Folder path" }).getByRole("button")).toHaveCount(1);
  expect(errors).toEqual([]);
});

test("settings validation shows field errors", async ({ page }) => {
  await page.goto("/settings");
  const stale = page.getByLabel("Stale after");
  await stale.fill("500");
  await page.getByRole("button", { name: "Save" }).click();
  await expect(page.getByText("must be between 1 and 120 months")).toBeVisible();
});
