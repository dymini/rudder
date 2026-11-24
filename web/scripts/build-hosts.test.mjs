import assert from "node:assert/strict";
import { test } from "node:test";
import { cachePercent, waitingReason } from "../src/build-hosts.ts";

test("cache usage bars handle normal, empty, zero-limit and invalid measurements", () => {
  assert.equal(cachePercent(5, 20), 25);
  assert.equal(cachePercent(30, 20), 100);
  assert.equal(cachePercent(0, 20), 0);
  assert.equal(cachePercent(0, 0), 0);
  assert.equal(cachePercent(1, 0), 100);
  assert.equal(cachePercent(-1, 20), 0);
  assert.equal(cachePercent(NaN, 20), 0);
  assert.equal(cachePercent(1, Infinity), 0);
});

test("queued deployments expose the latest complete waiting reason without stale finished-job reasons", () => {
  const logs = "Cloning\nWaiting: host is offline\nWaiting: all builder slots are busy\n";
  assert.equal(waitingReason("queued", logs), "Waiting: all builder slots are busy");
  assert.equal(waitingReason("queued", "\n Waiting: panel load guard \n"), "Waiting: panel load guard");
  assert.equal(waitingReason("queued", "Building on panel\n"), null);
  assert.equal(waitingReason("queued", ""), null);
  assert.equal(waitingReason("queued", null), null);
  assert.equal(waitingReason("running", logs), null);
  assert.equal(waitingReason("failed", logs), null);
  assert.equal(waitingReason("succeeded", logs), null);
});
