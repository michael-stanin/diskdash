import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";

test("history shows free-space trends and explains what a comparison needs", async ({ page }, info) => {
  const errors: string[] = [];
  page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
  page.on("pageerror", (e) => errors.push(e.message));
  await page.goto("/");
  await page.getByRole("link", { name: "History" }).click();
  await expect(page.getByRole("heading", { name: "History", exact: true })).toBeVisible();

  const trends = page.getByRole("region", { name: "Free space over time" });
  await expect(trends).toBeVisible();
  // Each drive gets a card with an honest forecast line: right after start there is no history to extend.
  await expect(trends.getByRole("heading").nth(1)).toBeVisible();
  await expect(trends.getByText(/Not enough history yet|Free space is stable|Low in about|Already below/).first()).toBeVisible();

  const changes = page.getByRole("region", { name: "What changed" });
  await expect(changes).toBeVisible();
  await expect(changes.getByText(/No scans yet|Need two scans of this folder to compare|Grew the most/).first()).toBeVisible();

  const results = await new AxeBuilder({ page }).withRules(["color-contrast"]).analyze();
  expect(results.violations.flatMap((v) => v.nodes.map((n) => n.target.join(" ")))).toEqual([]);
  await page.screenshot({ path: info.outputPath(`history-${info.project.name}.png`), fullPage: true });
  expect(errors).toEqual([]);
});
