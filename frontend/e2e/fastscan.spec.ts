import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";

test("the scan dialog says how each drive will be scanned and never starts the helper by itself", async ({ page }, info) => {
  const errors: string[] = [];
  page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
  page.on("pageerror", (e) => errors.push(e.message));
  const calls: string[] = [];
  page.on("request", (r) => calls.push(`${r.method()} ${new URL(r.url()).pathname}`));

  await page.goto("/");
  await page.getByRole("button", { name: "Scan drives" }).click();
  const dialog = page.getByRole("dialog", { name: "Scan drives" });
  await expect(dialog.getByRole("checkbox").first()).toBeVisible({ timeout: 20_000 });

  // Without the helper every drive is a directory walk, with the reason.
  const helper = dialog.getByRole("region", { name: "Elevated features" });
  await expect(helper).toBeVisible();
  await expect(helper.getByText(/Fast scan needs administrator rights|Fast scan is not installed|Elevated helper running/)).toBeVisible();
  await expect(dialog.getByText(/directory walk|fast scan \(MFT\)/).first()).toBeVisible();
  await expect(dialog.getByText(/A single folder is always scanned with the directory walk/)).toBeVisible();

  const results = await new AxeBuilder({ page }).withRules(["color-contrast"]).analyze();
  expect(results.violations.flatMap((v) => v.nodes.map((n) => n.target.join(" ")))).toEqual([]);
  await page.screenshot({ path: info.outputPath(`fast-scan-dialog-${info.project.name}.png`) });

  // Opening the dialog must not ask Windows for administrator rights: only the button does that.
  expect(calls.filter((c) => c.includes("/helper/enable"))).toEqual([]);
  await dialog.getByRole("button", { name: "Cancel" }).click();
  expect(errors).toEqual([]);
});
