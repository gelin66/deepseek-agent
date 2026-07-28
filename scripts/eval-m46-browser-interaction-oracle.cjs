#!/usr/bin/env node
"use strict";

// Eval-only interaction oracle. This is not a production sidecar. It performs
// one pre-registered role/name click on an exact loopback origin, returns one
// bounded semantic observation, and never emits HTML, storage, cookies,
// screenshots, pixels, coordinates, or arbitrary JavaScript results.

const { chromium } = require("playwright");
const playwrightVersion = require("playwright/package.json").version;

async function main() {
  const taskId = process.env.M46_INTERACTION_TASK_ID;
  const targetUrl = process.env.M46_INTERACTION_TARGET_URL;
  const allowedOrigin = process.env.M46_INTERACTION_ALLOWED_ORIGIN;
  const chromePath = process.env.M46_INTERACTION_CHROME_PATH;
  const action = JSON.parse(process.env.M46_INTERACTION_ACTION || "null");
  const expected = JSON.parse(process.env.M46_INTERACTION_EXPECTED_OBSERVATION || "null");
  const maxBytes = Number(process.env.M46_INTERACTION_MAX_OBSERVATION_BYTES);
  if (!taskId || !targetUrl || !allowedOrigin || !chromePath || !action || !expected || !maxBytes) {
    throw new Error("m46_interaction_oracle_environment_incomplete");
  }
  if (action.family !== "click" || action.selector !== "exact_role_name") {
    throw new Error("m46_interaction_oracle_action_not_admitted");
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

  let observation;
  try {
    const page = await context.newPage();
    await page.goto(targetUrl, { waitUntil: "domcontentloaded", timeout: 10000 });
    const target = page.getByRole(action.role, {
      name: action.accessible_name,
      exact: true,
    });
    await target.waitFor({ state: "visible", timeout: 5000 });
    const targetCount = await target.count();
    if (targetCount !== 1) throw new Error(`action_target_count:${targetCount}`);

    const resultBeforeAction = page.getByRole(expected.role, {
      name: expected.accessible_name,
      exact: true,
    });
    if ((await resultBeforeAction.count()) !== 0) {
      throw new Error("post_action_observation_present_before_click");
    }

    await target.click({ timeout: 5000 });
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

    observation = {
      task_id: taskId,
      title,
      action: {
        family: "click",
        selector: "exact_role_name",
        role: action.role,
        accessible_name: action.accessible_name,
        count: 1,
      },
      result: {
        node_count: nodeCount,
        role: expected.role,
        accessible_name: expected.accessible_name,
        state: {
          attribute: expected.state_attribute,
          value: stateValue,
        },
      },
      trust: "external_untrusted",
      blocked_external_origins: [...blockedOrigins].sort(),
      playwright_version: playwrightVersion,
      node_version: process.version,
      chrome_version: browser.version(),
      screenshot_count: 0,
    };
  } finally {
    await context.close();
    await browser.close();
  }

  observation.teardown = { context_closed: true, browser_closed: true };
  const encoded = JSON.stringify(observation);
  if (Buffer.byteLength(encoded, "utf8") > maxBytes) {
    throw new Error("bounded_observation_exceeded");
  }
  process.stdout.write(encoded + "\n");
}

main().catch((error) => {
  process.stderr.write(`${error.message}\n`);
  process.exit(1);
});
