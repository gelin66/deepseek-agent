#!/usr/bin/env node
"use strict";

// Eval-only DOM/accessibility oracle. It is not a production sidecar and it
// never returns page HTML, script, storage, cookies, screenshots, or pixels.

const { chromium } = require("playwright");
const playwrightVersion = require("playwright/package.json").version;

async function main() {
  const taskId = process.env.M46_TASK_ID;
  const targetUrl = process.env.M46_TARGET_URL;
  const allowedOrigin = process.env.M46_ALLOWED_ORIGIN;
  const chromePath = process.env.M46_CHROME_PATH;
  const expected = JSON.parse(process.env.M46_EXPECTED_OBSERVATION);
  const maxBytes = Number(process.env.M46_MAX_OBSERVATION_BYTES);
  if (!taskId || !targetUrl || !allowedOrigin || !chromePath || !maxBytes) {
    throw new Error("m46_oracle_environment_incomplete");
  }

  const browser = await chromium.launch({
    executablePath: chromePath,
    headless: true,
    args: [
      "--disable-background-networking",
      "--disable-component-update",
      "--disable-default-apps",
      "--disable-extensions",
      "--disable-sync",
      "--disable-translate",
      "--metrics-recording-only",
      "--no-default-browser-check",
      "--no-first-run",
    ],
  });
  const context = await browser.newContext({
    acceptDownloads: false,
    bypassCSP: false,
    ignoreHTTPSErrors: false,
    javaScriptEnabled: true,
    serviceWorkers: "block",
  });
  const blockedOrigins = new Set();
  await context.route("**/*", async (route) => {
    const requestOrigin = new URL(route.request().url()).origin;
    if (requestOrigin === allowedOrigin) {
      await route.continue();
    } else {
      blockedOrigins.add(requestOrigin);
      await route.abort("blockedbyclient");
    }
  });

  const page = await context.newPage();
  try {
    await page.goto(targetUrl, { waitUntil: "domcontentloaded", timeout: 10000 });
    const semanticNode = page.getByRole(expected.role, {
      name: expected.accessible_name,
      exact: true,
    });
    await semanticNode.waitFor({ state: "visible", timeout: 5000 });
    const nodeCount = await semanticNode.count();
    if (nodeCount !== 1) throw new Error(`semantic_node_count:${nodeCount}`);
    const stateValue = await semanticNode.getAttribute(expected.state_attribute);
    const title = await page.title();
    if (title !== expected.title) throw new Error(`title_mismatch:${title}`);
    if (stateValue !== expected.state_value) {
      throw new Error(`state_mismatch:${stateValue}`);
    }

    const observation = {
      task_id: taskId,
      title,
      node_count: nodeCount,
      role: expected.role,
      accessible_name: expected.accessible_name,
      state: {
        attribute: expected.state_attribute,
        value: stateValue,
      },
      trust: "external_untrusted",
      blocked_external_origins: [...blockedOrigins].sort(),
      playwright_version: playwrightVersion,
      node_version: process.version,
      chrome_version: browser.version(),
      screenshot_count: 0,
    };
    const encoded = JSON.stringify(observation);
    if (Buffer.byteLength(encoded, "utf8") > maxBytes) {
      throw new Error("bounded_observation_exceeded");
    }
    process.stdout.write(encoded + "\n");
  } finally {
    await context.close();
    await browser.close();
  }
}

main().catch((error) => {
  process.stderr.write(`${error.message}\n`);
  process.exit(1);
});
