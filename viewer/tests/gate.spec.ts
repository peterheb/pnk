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

// Substitute fonts (webfonts.ts) default ON, and the stylesheet they need is
// the only thing this viewer ever fetches. Every test that asserts the
// zero-network guarantee therefore runs with the setting OFF; the last test
// turns it on and checks WHERE the requests go.
test.beforeEach(async ({ page }) => {
  await page.addInitScript(() => window.localStorage.setItem("pnk.googleFonts", "0"));
});

// Same-origin requests are the viewer's own code (the pdf.js chunk and
// worker load on demand); blob:/data: are local. Everything else counts.
const ORIGIN = "http://127.0.0.1:8123/";
function trackRequests(page: Page): void {
  page.on("request", (req) => {
    const url = req.url();
    if (!url.startsWith("blob:") && !url.startsWith("data:") && !url.startsWith(ORIGIN)) networkRequests.push(url);
  });
}

function shot(page: Page, name: string, fullPage = true) {
  return page.screenshot({ path: path.join(OUT, name), fullPage });
}


test("landing shows the local-only drop zone", async ({ page }) => {
  await page.goto("/");
  trackRequests(page);
  await expect(page.locator("#drop-target")).toBeVisible();
  await expect(page.locator("#pick-btn")).toBeVisible();
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
  // Continuous-scroll view: a .notes-panel renders only for slides with
  // VISIBLE notes. This deck's notes storages are all empty paragraphs, so
  // no panel may appear (the old UI showed 11 empty yellow strips).
  await expect(page.locator(".notes-panel")).toHaveCount(0);
  // slide switching works; walk the deck until a slide carries a raster
  // image that decoded from local blob bytes (some master art is vector
  // PDF — those render blank in <img> and are skipped)
  let sawImage = false;
  const total = Math.min(await items.count(), 12);
  for (let i = 0; i < total && !sawImage; i++) {
    await items.nth(i).click();
    const imgs = page.locator(".slide-stage .canvas-drawable img");
    for (let k = 0; k < (await imgs.count()); k++) {
      const w = await imgs.nth(k).evaluate((node) => (node as HTMLImageElement).naturalWidth);
      if (w > 0) {
        await expect(imgs.nth(k)).toBeVisible();
        sawImage = true;
        break;
      }
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

test("pages fixture renders paginated pages with paragraphs and headings", async ({ page }) => {
  await page.goto("/");
  trackRequests(page);
  await page.setInputFiles("#file-input", path.join(CRAWL, FIXTURES.pages));
  // word-processing docs paginate into page frames; the printable area is
  // .pages-print inside each frame (docs without page geometry fall back to
  // one .pages-flow)
  // the first page may be a cover the floating image fills (this fixture's
  // is), so look for the first printable area that holds a paragraph
  const area = page.locator(".pages-wp-page .pages-print, .pages-flow").filter({ has: page.locator("p") }).first();
  await expect(area).toBeVisible();
  await expect(area.locator("p").first()).toBeVisible();
  // this fixture has 200+ styled headings in the body
  await expect(page.locator(".pages-wp-page h1, .pages-wp-page h2, .pages-wp-page h3, .pages-wp-page h4, .pages-wp-page h5, .pages-wp-page h6, .pages-flow h1, .pages-flow h2, .pages-flow h3").first()).toBeVisible();
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

test("substitute fonts are requested from Google Fonts and nowhere else", async ({ page }) => {
  // wins over the beforeEach hook's "0": init scripts run in the order added
  await page.addInitScript(() => window.localStorage.setItem("pnk.googleFonts", "1"));
  await page.goto("/");
  trackRequests(page);
  // this fixture's font list is Calibri / Arial / Verdana / Helvetica Neue —
  // four families, none of them on a stock Linux or Windows machine
  await page.setInputFiles("#file-input", path.join(CRAWL, FIXTURES.numbers));
  await expect(page.locator("table.sheet-table").first()).toBeVisible();

  const href = await page.locator("link#pnk-webfonts").getAttribute("href");
  expect(href).toContain("family=Carlito:"); // metric clone of Calibri
  expect(href).toContain("family=Arimo:"); // metric clone of Arial
  expect(href).toContain("family=Open+Sans:"); // stand-in for Verdana
  expect(href).toContain("family=Inter:"); // stand-in for Helvetica Neue

  // The faces really arrive. document.fonts.check() is no use here — it
  // answers true for a family nobody has, since the system font satisfies
  // it — so look for the FontFace the stylesheet added and its load state.
  await page.waitForFunction(
    () => [...document.fonts].some((f) => f.family === "Carlito" && f.status === "loaded"),
    null,
    { timeout: 20_000 },
  );

  const external = networkRequests.filter((u) => !u.startsWith(ORIGIN));
  expect(external.some((u) => u.startsWith("https://fonts.gstatic.com/"))).toBe(true);
  for (const url of external) {
    expect(url).toMatch(/^https:\/\/fonts\.(googleapis|gstatic)\.com\//);
  }

  // and the nav setting turns it off: the stylesheet goes away with the
  // re-render, and the document is still on screen
  await page.locator("#settings-dd summary").click();
  await page.locator("#gfonts-toggle").uncheck();
  await expect(page.locator("link#pnk-webfonts")).toHaveCount(0);
  await expect(page.locator("table.sheet-table").first()).toBeVisible();
  expect(await page.evaluate(() => window.localStorage.getItem("pnk.googleFonts"))).toBe("0");
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