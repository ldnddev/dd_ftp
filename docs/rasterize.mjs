#!/usr/bin/env node
/**
 * Screenshot HTML frames written by `examples/tutorial_shots.rs`
 * into PNGs consumed by `docs/index.html`.
 *
 * Usage: node docs/rasterize.mjs
 */
import { createRequire } from "node:module";
import { readFileSync, mkdirSync, existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { homedir } from "node:os";

function loadPlaywright() {
  const candidates = [
    process.env.PLAYWRIGHT_NODE_MODULES,
    process.env.NODE_PATH?.split(":").shift(),
    join(homedir(), ".local/share/mise/installs/npm-playwright/latest/node_modules"),
  ].filter(Boolean);
  for (const dir of candidates) {
    const pkg = join(dir, "playwright", "package.json");
    if (existsSync(pkg)) {
      return createRequire(pkg)("playwright");
    }
  }
  return createRequire(import.meta.url)("playwright");
}

const { chromium } = loadPlaywright();

const docsDir = dirname(fileURLToPath(import.meta.url));
const framesDir = join(docsDir, "images", "_frames");
const outDir = join(docsDir, "images");
const manifestPath = join(framesDir, "manifest.json");

if (!existsSync(manifestPath)) {
  console.error(`missing ${manifestPath}`);
  console.error("run: cargo run -p dd_ftp_cli --example tutorial_shots --release");
  process.exit(1);
}

const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
mkdirSync(outDir, { recursive: true });

const executablePath =
  process.env.TUTORIAL_CHROMIUM ||
  ["/usr/bin/chromium", "/usr/bin/chromium-browser", "/usr/bin/google-chrome"].find(
    (p) => existsSync(p),
  );

const browser = await chromium.launch({
  executablePath,
  args: ["--no-sandbox", "--disable-dev-shm-usage"],
});

const page = await browser.newPage({
  viewport: { width: 1600, height: 1000 },
  deviceScaleFactor: 2,
});

for (const shot of manifest.shots) {
  const htmlPath = resolve(framesDir, `${shot.id}.html`);
  const pngPath = resolve(outDir, `${shot.id}.png`);
  await page.goto(pathToFileURL(htmlPath).href, { waitUntil: "load" });
  await page.evaluate(() => document.fonts.ready);
  const shotEl = page.locator(".shot");
  await shotEl.waitFor();
  await shotEl.screenshot({ path: pngPath, type: "png" });
  console.log(`wrote ${pngPath}`);
}

await browser.close();
