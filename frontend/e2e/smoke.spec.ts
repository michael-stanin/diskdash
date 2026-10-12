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

test("settings validation shows field errors", async ({ page }) => {
  await page.goto("/settings");
  const stale = page.getByLabel("Stale after");
  await stale.fill("500");
  await page.getByRole("button", { name: "Save" }).click();
  await expect(page.getByText("must be between 1 and 120 months")).toBeVisible();
});
