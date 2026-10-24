// =============================================================================
//  SysPulse - Multipurpose System Utility
//  File   : scripts/screenshots.mjs
//  Author : Praneeth
//  License: MIT
//  Purpose: Regenerate the README screenshots (docs/screenshots/*.png) from
//           the UI in browser demo mode, so no real system data is shown.
//
//    1. npm --prefix ui run dev          (UI dev server on :1420, demo data)
//    2. npm i --no-save puppeteer-core   (once, in a scratch folder or here)
//    3. node scripts/screenshots.mjs     (uses /snap/bin/chromium)
// =============================================================================
import puppeteer from "puppeteer-core";
const OUT = new URL("../docs/screenshots", import.meta.url).pathname;
const BASE = "http://localhost:1420/?shot&page=";
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const browser = await puppeteer.launch({
  executablePath: "/snap/bin/chromium",
  headless: true,
  userDataDir: `${process.env.HOME}/snap/chromium/common/pptr-profile`,
  args: ["--window-size=1440,900", "--hide-scrollbars"],
  defaultViewport: { width: 1440, height: 900, deviceScaleFactor: 1 },
});
const page = await browser.newPage();
async function clickText(sel, text) {
  const ok = await page.evaluate((sel, text) => {
    const el = [...document.querySelectorAll(sel)].find((e) => e.textContent.trim().startsWith(text));
    if (el) el.click();
    return !!el;
  }, sel, text);
  if (!ok) console.log("  (not found)", sel, text);
}
async function shot(name, id, prep, wait = 2500) {
  await page.goto(BASE + id, { waitUntil: "networkidle0" });
  await sleep(wait);
  if (prep) await prep();
  await page.screenshot({ path: `${OUT}/${name}.png` });
  console.log("saved", name);
}

await shot("dashboard", "dashboard", null, 15000);
await shot("netlens", "netlens", async () => {
  await page.click(".plist .row");
  await page.keyboard.press("Home");
  await sleep(700);
  for (let i = 0; i < 5; i++) { await page.keyboard.press("ArrowDown"); await sleep(120); }
  await sleep(1200);
  // Open the TLS record -> Client Hello -> Extensions, then select the SNI line.
  for (const prefix of ["TLSv1.0 Record Layer", "Handshake Protocol: Client Hello", "Extensions ("]) {
    await page.evaluate((prefix) => {
      const line = [...document.querySelectorAll(".tree .line")].find((l) => l.querySelector(".lbl")?.textContent.startsWith(prefix));
      line?.querySelector("button.tw")?.click();
    }, prefix);
    await sleep(300);
  }
  await page.evaluate(() => {
    const line = [...document.querySelectorAll(".tree .line")].find((l) => l.textContent.includes("server_name: github.com"));
    line?.click();
    line?.scrollIntoView({ block: "center" });
  });
  await sleep(800);
});
await shot("processes", "processes", null, 4000);
await shot("largefiles", "largefiles", async () => {
  await clickText("button", "Scan");
  await sleep(2500);
  await clickText("button", "Disk map");
  await sleep(1200);
});
await shot("launchers", "launchers", async () => {
  await clickText("button", "Create the first launcher");
  await sleep(2500);
});
await shot("gpu", "gpu", null, 9000);
await browser.close();
