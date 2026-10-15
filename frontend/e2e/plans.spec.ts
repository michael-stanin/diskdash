import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";

const PHOTOS = String.raw`D:\Photos`;

test("plans: empty state, then a plan can be created and is a checklist, in both themes", async ({ page }, info) => {
  const errors: string[] = [];
  page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
  page.on("pageerror", (e) => errors.push(e.message));
  await page.goto("/plans");
  await expect(page.getByRole("heading", { name: "Plans", exact: true })).toBeVisible();
  await expect(page.getByText("No plans yet")).toBeVisible();
  await expect(page.getByRole("link", { name: "Open Recommendations" })).toBeVisible();
  const empty = await new AxeBuilder({ page }).withRules(["color-contrast"]).analyze();
  expect(empty.violations.flatMap((v) => v.nodes.map((n) => n.target.join(" ")))).toEqual([]);
  await page.screenshot({ path: info.outputPath(`plans-empty-${info.project.name}.png`), fullPage: true });

  const name = `Test plan ${info.project.name}`;
  await page.getByLabel("Name of a new plan").fill(name);
  await page.getByRole("button", { name: "Create" }).click();
  await expect(page.getByRole("heading", { name })).toBeVisible();
  await expect(page.getByText("This plan is empty")).toBeVisible();
  await expect(page.getByRole("link", { name: "Export the plan as CSV" })).toHaveAttribute("href", /\/export\?format=csv$/);
  const filled = await new AxeBuilder({ page }).withRules(["color-contrast"]).analyze();
  expect(filled.violations.flatMap((v) => v.nodes.map((n) => n.target.join(" ")))).toEqual([]);
  await page.screenshot({ path: info.outputPath(`plans-plan-${info.project.name}.png`), fullPage: true });
  // Leave the data directory as found for the next project.
  await page.getByRole("button", { name: "Delete the plan" }).click();
  await page.getByRole("button", { name: "Delete plan" }).click();
  await expect(page.getByText("No plans yet")).toBeVisible();
  expect(errors).toEqual([]);
});

test("settings: exclusions can be added, saved and removed", async ({ page }) => {
  await page.goto("/settings");
  await expect(page.getByText("No exclusions yet.", { exact: false })).toBeVisible();
  await page.getByLabel("Excluded folders").fill(PHOTOS);
  await page.getByRole("button", { name: "Add" }).first().click();
  await expect(page.getByText(PHOTOS, { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "Save" }).click();
  await expect(page.getByText("Settings saved")).toBeVisible();
  await page.reload();
  await expect(page.getByText(PHOTOS, { exact: true })).toBeVisible();
  // Put it back so other tests see a clean state.
  await page.getByRole("button", { name: `Remove ${PHOTOS}` }).click();
  await page.getByRole("button", { name: "Save" }).click();
  await expect(page.getByText("Settings saved")).toBeVisible();
});
