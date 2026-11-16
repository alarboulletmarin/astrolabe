// Renders video.html frame by frame: node render.js <outdir> [t1,t2,...]  (no list = every frame).
const { chromium } = require("playwright");
const path = require("path"), fs = require("fs");
(async () => {
  const out = process.argv[2]; fs.mkdirSync(out, { recursive: true });
  const only = process.argv[3] ? process.argv[3].split(",").map(Number) : null;
  const browser = await chromium.launch({ args: ["--allow-file-access-from-files", "--force-color-profile=srgb"] });
  const page = await browser.newPage({ viewport: { width: 1920, height: 1080 }, deviceScaleFactor: 1 });
  page.on("pageerror", e => console.error("page error:", e.message));
  page.on("console", m => console.log("console:", m.text()));
  await page.goto("file://" + path.resolve(__dirname, "video.html"));
  await page.evaluate(() => window.ready);
  const times = only || Array.from({ length: 900 }, (_, i) => i / 30);
  const stage = await page.$("#stage");
  for (let i = 0; i < times.length; i++) {
    await page.evaluate(t => window.render(t), times[i]);
    const name = only ? `t${times[i].toFixed(2)}.png` : `f${String(i).padStart(4, "0")}.png`;
    await stage.screenshot({ path: path.join(out, name) });
    if (!only && i % 60 === 0) console.log("frame", i);
  }
  await browser.close();
})();
