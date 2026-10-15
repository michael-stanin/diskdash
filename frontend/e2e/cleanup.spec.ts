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

test("recommendations ask for a scan when nothing was scanned", async ({ page }, info) => {
  const errors = collectErrors(page);
  await page.goto("/recommendations");
  await expect(page.getByRole("heading", { name: "Recommendations" })).toBeVisible();
  await expect(page.getByText("Scan a drive to see suggestions")).toBeVisible();
  await expect(page.getByRole("button", { name: /Scan a drive/ })).toBeVisible();
  // Filters are present and usable even without data.
  await expect(page.getByRole("radiogroup", { name: "Risk" })).toBeVisible();
  await expectNoContrastIssues(page);
  await page.screenshot({ path: info.outputPath(`recommendations-empty-${info.project.name}.png`), fullPage: true });
  expect(errors).toEqual([]);
});

test("rules page lists the built-in rules with details and copyable steps", async ({ page }, info) => {
  const errors = collectErrors(page);
  await page.goto("/rules");
  await expect(page.getByRole("heading", { name: "Cleanup rules" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Your temporary files" })).toBeVisible();
  await expect(page.getByText(/You have no rules of your own yet/)).toBeVisible();
  // No rule problems in a clean install.
  await expect(page.getByRole("alert", { name: "Rule problems" })).toHaveCount(0);

  await page.getByRole("searchbox", { name: "Search rules" }).fill("hibernation");
  await expect(page.getByRole("heading", { name: "Hibernation file (hiberfil.sys)" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Your temporary files" })).toHaveCount(0);

  const row = page.getByRole("listitem", { name: "Hibernation file (hiberfil.sys)" });
  await row.getByRole("button", { name: /Details/ }).click();
  await expect(row.getByText("powercfg /h off", { exact: true })).toBeVisible();
  await expect(row.getByText(/only shows these steps/)).toBeVisible();
  await expect(row.getByText(/hiberfil\.sys/i).first()).toBeVisible();

  await expectNoContrastIssues(page);
  await page.screenshot({ path: info.outputPath(`rules-${info.project.name}.png`), fullPage: true });
  expect(errors).toEqual([]);
});

test("rule details show the folders a rule looks in on this PC", async ({ page }) => {
  await page.goto("/rules");
  const row = page.getByRole("listitem", { name: "Your temporary files" });
  await row.getByRole("button", { name: /Details/ }).click();
  await expect(row.getByText("Where it looks")).toBeVisible();
  await expect(row.getByText(/Temp$/).first()).toBeVisible();
});

test("the recommendations page links to the rules and back", async ({ page }) => {
  await page.goto("/recommendations");
  await page.getByRole("link", { name: /Rules/ }).click();
  await expect(page).toHaveURL(/\/rules$/);
  await page.getByRole("navigation", { name: "Breadcrumb" }).getByRole("link", { name: "Recommendations" }).click();
  await expect(page).toHaveURL(/\/recommendations$/);
});
