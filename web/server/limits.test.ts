import assert from "node:assert/strict";
import { test } from "node:test";

import { Budget, RateLimiter } from "./limits.ts";

test("a caller gets a few calls per window, then waits", () => {
  let now = 0;
  const limiter = new RateLimiter(2, 1000, () => now);
  assert.equal(limiter.allow("a"), true);
  assert.equal(limiter.allow("a"), true);
  assert.equal(limiter.allow("a"), false);
  assert.equal(limiter.allow("b"), true, "another caller has its own window");
  now = 1000;
  assert.equal(limiter.allow("a"), true);
});

test("the daily budget caps what all callers together can spend", () => {
  let now = 0;
  const budget = new Budget(10n, () => now);
  assert.equal(budget.reserve(6n), true);
  assert.equal(budget.reserve(5n), false);
  budget.release(6n);
  assert.equal(budget.reserve(10n), true);
  now = 24 * 60 * 60 * 1000;
  assert.equal(budget.reserve(10n), true, "a new day starts a new budget");
});
