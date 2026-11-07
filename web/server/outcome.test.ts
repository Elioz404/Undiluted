import assert from "node:assert/strict";
import { test } from "node:test";

import { isTimeout, readOutcome } from "./outcome.ts";

const encode = (text: string) => Buffer.from(text).toString("base64");

test("a successful call returns its decoded value", () => {
  const outcome = readOutcome("h", {
    status: { SuccessValue: encode('{"status":"identity"}') },
    receipts_outcome: [{ outcome: { logs: ["hello"] } }],
  });
  assert.equal(outcome.succeeded, true);
  assert.equal(outcome.value, '{"status":"identity"}');
  assert.equal(outcome.refusal, null);
});

test("a refusal is read from the logs even when the panic message is lost", () => {
  const outcome = readOutcome("h", {
    status: { Failure: { ActionError: {} } },
    receipts_outcome: [{ outcome: { logs: ["undiluted refused: invalid enclave signature"] } }],
  });
  assert.equal(outcome.succeeded, false);
  assert.equal(outcome.refusal, "invalid enclave signature");
});

test("a failure without a reason is neither a success nor a refusal", () => {
  const outcome = readOutcome("h", { status: { Failure: {} }, receipts_outcome: [] });
  assert.equal(outcome.succeeded, false);
  assert.equal(outcome.refusal, null);
});

test("an empty return value is reported as none", () => {
  const outcome = readOutcome("h", { status: { SuccessValue: "" } });
  assert.equal(outcome.succeeded, true);
  assert.equal(outcome.value, null);
});

test("timeouts are told apart from real failures", () => {
  assert.equal(isTimeout(new Error("The request failed with timeout error")), true);
  assert.equal(isTimeout(new Error("TIMEOUT_ERROR")), true);
  assert.equal(isTimeout(new Error("insufficient balance")), false);
});
