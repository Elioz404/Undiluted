import assert from "node:assert/strict";
import { test } from "node:test";

import { settlementFrom } from "./payments.ts";

test("a settled payment gives what the treasury received and where", () => {
  const answer = {
    status: "SUCCESS",
    swapDetails: { amountOut: "1993690", destinationChainTxHashes: [{ hash: "Erw1" }] },
  };
  assert.deepEqual(settlementFrom(answer), { reference: "Erw1", receivedMicros: "1993690" });
});

test("a payment in progress is not settled yet", () => {
  for (const status of ["PENDING_DEPOSIT", "KNOWN_DEPOSIT_TX", "PROCESSING"]) {
    assert.equal(settlementFrom({ status }), null);
  }
});

test("a refunded payment is a failure", () => {
  assert.throws(() => settlementFrom({ status: "REFUNDED" }), /REFUNDED/);
});

test("success without details is not trusted", () => {
  assert.throws(() => settlementFrom({ status: "SUCCESS", swapDetails: {} }));
});
