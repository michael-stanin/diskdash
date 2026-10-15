import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";

test("tiering shows every drive's free space and says what is missing before any scan", async ({ page }, info) => {
  const errors: string[] = [];
  page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
  page.on("pageerror", (e) => errors.push(e.message));
  await page.goto("/tiering");
  await expect(page.getByRole("heading", { name: "Tiering", exact: true })).toBeVisible();
  const bars = page.getByRole("region", { name: "Free space" });
  await expect(bars).toBeVisible();
  await expect(bars.getByText("After planned moves").first()).toBeVisible();
  await expect(page.getByText("No tiering suggestions")).toBeVisible();
  await expect(page.getByText(/Not scanned yet/)).toBeVisible();
  await expect(page.getByRole("link", { name: /Adjust the thresholds/ })).toHaveAttribute("href", "/settings");
  const results = await new AxeBuilder({ page }).withRules(["color-contrast"]).analyze();
  expect(results.violations.flatMap((v) => v.nodes.map((n) => n.target.join(" ")))).toEqual([]);
  await page.screenshot({ path: info.outputPath(`tiering-${info.project.name}.png`), fullPage: true });
  expect(errors).toEqual([]);
});
