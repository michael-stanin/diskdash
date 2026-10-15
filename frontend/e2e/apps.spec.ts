import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";

test("installed apps lists this PC's applications and only links to Windows Settings", async ({ page }, info) => {
  const errors: string[] = [];
  page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
  page.on("pageerror", (e) => errors.push(e.message));
  await page.goto("/apps");
  await expect(page.getByRole("heading", { name: "Installed apps" })).toBeVisible();
  await expect(page.getByRole("table", { name: "Installed applications" }).or(page.getByRole("grid", { name: "Installed applications" }))).toBeVisible({ timeout: 15_000 });
  await expect(page.getByText(/rows$/)).toBeVisible();
  const link = page.getByRole("link", { name: /Open Apps & features in Windows Settings/ });
  await expect(link).toHaveAttribute("href", "ms-settings:appsfeatures");
  await expect(page.getByRole("button", { name: /uninstall/i })).toHaveCount(0);
  const results = await new AxeBuilder({ page }).withRules(["color-contrast"]).analyze();
  expect(results.violations.flatMap((v) => v.nodes.map((n) => n.target.join(" ")))).toEqual([]);
  await page.screenshot({ path: info.outputPath(`apps-${info.project.name}.png`), fullPage: true });
  expect(errors).toEqual([]);
});
