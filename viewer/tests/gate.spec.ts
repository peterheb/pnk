// Gate: one real fixture per app renders in the served viewer; encrypted and
// legacy files produce the friendly error cards. Screenshots go to /tmp
// (never into the repo).
//
// The crawl only contains modern files, so the two error fixtures are
// synthesized as ZIP containers whose member base names carry the exact
// markers loader.rs rejects (".iwph" → encrypted, "index.apxl" → legacy).

import { execSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { expect, test, type Page } from "@playwright/test";

const HERE = path.dirname(import.meta.url.replace(/^file:\/\//, ""));
const CRAWL = path.join(HERE, "../../fixtures/crawl");
const OUT = "/tmp/pnk-gate";

const FIXTURES = {
  numbers: "eb299192a219f684d2ba56f2c5f7b06e2c055eb9815b73e82cd5ecc2c57b6bf6.numbers",
  keynote: "85c3a6f17ca8e64ae24fb95c64af6c47e87f27237bfc19386bd09088da998007.key",
  pages: "ee7036ce02c55c331b6cbc984e9650977a4fab1e725ea32a89ed39b1f79321c5.pages",
};

function zipWith(memberName: string, content: string): string {
  const dir = fs.mkdtempSync(path.join(OUT, "mkzip-"));
  const member = path.join(dir, memberName);
  fs.writeFileSync(member, content);
  const zip = path.join(OUT, `${memberName}.zip`);
  execSync(`zip -j -q ${JSON.stringify(zip)} ${JSON.stringify(member)}`);
  return zip;
}

const LEGACY_FIXTURE = () => zipWith("index.apxl", "pre-iWork-'13 XML bundle marker");
const ENCRYPTED_FIXTURE = () => zipWith(".iwph", "not really encrypted — the marker is what matters");

const networkRequests: string[] = [];

test.beforeAll(() => {
  fs.mkdirSync(OUT, { recursive: true });
});

test.beforeEach(() => {
  networkRequests.length = 0;
});

function trackRequests(page: Page): void {
  page.on("request", (req) => {
    const url = req.url();
    if (!url.startsWith("blob:") && !url.startsWith("data:")) networkRequests.push(url);
  });
}

function shot(page: Page, name: string, fullPage = true) {
  return page.screenshot({ path: path.join(OUT, name), fullPage });
}


test("landing shows the local-only drop zone", async ({ page }) => {
  await page.goto("/");
  trackRequests(page);
  await expect(page.locator("#drop-target")).toBeVisible();
  await expect(page.locator("#drop-hint")).toContainText("nothing about them leaves the browser");
  await shot(page, "landing.png");
});

function assertNoRuntimeNetwork(page: Page): void {
  expect(networkRequests).toEqual([]);
}

test("keynote fixture renders slides with positioned content + notes", async ({ page }) => {
  await page.goto("/");
  trackRequests(page);
  await page.setInputFiles("#file-input", path.join(CRAWL, FIXTURES.keynote));
  await expect(page.locator("#app-badge")).toContainText(/keynote/i);
  const items = page.locator(".slide-list-item");
  await expect(items.count()).resolves.toBeGreaterThanOrEqual(2);
  await expect(page.locator(".notes-panel")).toBeVisible();
  // slide switching works; walk the deck until a slide with a real photo is
  // up, then confirm it decoded from local blob bytes
  let sawImage = false;
  for (let i = 0; i < Math.min(await items.count(), 8) && !sawImage; i++) {
    await items.nth(i).click();
    const img = page.locator(".slide-stage .canvas-drawable img");
    if ((await img.count()) > 0) {
      await expect(img.first()).toBeVisible();
      await expect.poll(async () => img.first().evaluate((node) => (node as HTMLImageElement).naturalWidth)).toBeGreaterThan(0);
      sawImage = true;
    }
  }
  expect(sawImage).toBe(true);
  await shot(page, "keynote.png");
  assertNoRuntimeNetwork(page);
});

test("numbers fixture renders sheet tables with real cell values", async ({ page }) => {
  await page.goto("/");
  trackRequests(page);
  await page.setInputFiles("#file-input", path.join(CRAWL, FIXTURES.numbers));
  await expect(page.locator("table.sheet-table").first()).toBeVisible();
  // at least one cell carries a real value
  const populated = page.locator("table.sheet-table td", { hasText: /\S/ });
  await expect(populated.first()).toBeVisible();
  // sheet tabs exist and switching re-mounts the sheet area
  const tabs = page.locator(".sheet-tab");
  if ((await tabs.count()) > 1) {
    await tabs.nth(1).click();
    await expect(page.locator(".sheet-area[data-sheet-index='1']")).toBeVisible();
  }
  await shot(page, "numbers.png");
  assertNoRuntimeNetwork(page);
});

test("pages fixture renders flowing paragraphs and headings", async ({ page }) => {
  await page.goto("/");
  trackRequests(page);
  await page.setInputFiles("#file-input", path.join(CRAWL, FIXTURES.pages));
  const flow = page.locator(".pages-flow");
  await expect(flow).toBeVisible();
  await expect(flow.locator("p").first()).toBeVisible();
  // this fixture has 200+ styled headings in the body
  await expect(flow.locator("h1, h2, h3, h4, h5, h6").first()).toBeVisible();
  await shot(page, "pages.png", false);
  assertNoRuntimeNetwork(page);
});

test("legacy fixture gets the legacy explanation", async ({ page }) => {
  await page.goto("/");
  await page.setInputFiles("#file-input", LEGACY_FIXTURE());
  const card = page.locator(".error-card");
  await expect(card).toBeVisible();
  await expect(card.locator(".error-title")).toContainText(/legacy/i);
  await shot(page, "error-legacy.png");
  assertNoRuntimeNetwork(page);
});

test("encrypted fixture gets the password-protected explanation", async ({ page }) => {
  await page.goto("/");
  trackRequests(page);
  await page.setInputFiles("#file-input", ENCRYPTED_FIXTURE());
  const card = page.locator(".error-card");
  await expect(card).toBeVisible();
  await expect(card.locator(".error-title")).toContainText(/password-protected/i);
  await shot(page, "error-encrypted.png");
  assertNoRuntimeNetwork(page);
});