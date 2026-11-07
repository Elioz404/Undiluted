import assert from "node:assert/strict";
import { test } from "node:test";

import { nearAmount, printedHash } from "./near.ts";

test("yoctoNEAR becomes the amount the near tool reads, exactly", () => {
  assert.equal(nearAmount(10_000_000_000_000_000_000_000n), "0.01 NEAR");
  assert.equal(nearAmount(1n), "0.000000000000000000000001 NEAR");
  assert.equal(nearAmount(2n * 10n ** 24n), "2 NEAR");
});

test("the transaction hash is found in decorated output, even after a timeout", () => {
  const output = "Could not confirm whether the transaction was included.\nTransaction ID: 44m35cj8ib51bJQuYXKUNodsCSB4ifUCkMaZSPVmwSFT\nTo see";
  assert.equal(printedHash(output), "44m35cj8ib51bJQuYXKUNodsCSB4ifUCkMaZSPVmwSFT");
  assert.equal(printedHash("Error: the request failed"), null);
});
