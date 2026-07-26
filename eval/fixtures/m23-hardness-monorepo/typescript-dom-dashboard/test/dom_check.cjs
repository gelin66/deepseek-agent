const { chromium } = require("playwright");

(async () => {
  const baseURL = process.env.M23_DOM_BASE_URL;
  const browser = await chromium.launch({
    executablePath: "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    headless: true,
  });
  const page = await browser.newPage();
  try {
    await page.goto(baseURL, { waitUntil: "networkidle" });
    const button = page.locator("#toggle");
    const list = page.locator("#run-list");
    const empty = page.locator("#empty");
    if (await button.getAttribute("aria-expanded") !== "true") throw new Error("initial aria");
    if (await list.locator("li").count() !== 2) throw new Error("initial rows");
    await button.click();
    if (await button.getAttribute("aria-expanded") !== "false") throw new Error("collapsed aria");
    if (await list.locator("li").count() !== 1) throw new Error("filtered rows");
    await page.evaluate(async () => {
      const { renderRuns } = await import("/src/view.js");
      const state = document.querySelector("#empty");
      document.querySelector("#run-list").replaceChildren();
      renderRuns(document.querySelector("#run-list"), state, []);
    });
    if (await empty.isHidden()) throw new Error("empty status");
    if (await empty.textContent() !== "No active runs") throw new Error("empty text");
  } finally {
    await browser.close();
  }
})().catch((error) => {
  console.error(error.message);
  process.exit(1);
});
