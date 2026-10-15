import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";

test("duplicates page explains what is read and needs a scan first", async ({ page }, info) => {
  const errors: string[] = [];
  page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
  page.on("pageerror", (e) => errors.push(e.message));
  await page.goto("/duplicates");
  await expect(page.getByRole("heading", { name: "Duplicates" })).toBeVisible();
  await expect(page.getByText("Find duplicate files")).toBeVisible();
  await expect(page.getByText(/It only reads/)).toBeVisible();
  await expect(page.getByRole("button", { name: /Find duplicates/ })).toBeDisabled();
  await expect(page.getByText(/Scan a drive first/)).toBeVisible();
  const results = await new AxeBuilder({ page }).withRules(["color-contrast"]).analyze();
  expect(results.violations.flatMap((v) => v.nodes.map((n) => n.target.join(" ")))).toEqual([]);
  await page.screenshot({ path: info.outputPath(`duplicates-${info.project.name}.png`), fullPage: true });
  expect(errors).toEqual([]);
});
