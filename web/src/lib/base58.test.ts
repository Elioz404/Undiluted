import assert from "node:assert/strict";
import { test } from "node:test";

import { baseEncode } from "near-api-js";

import { base58ToHex } from "./base58.ts";

test("decodes what NEAR encodes, leading zero bytes included", () => {
  for (const hex of ["00", "0000ff", "ab".repeat(32), "00" + "11".repeat(31)]) {
    assert.equal(base58ToHex(baseEncode(Buffer.from(hex, "hex"))), hex);
  }
});

test("refuses characters outside the alphabet", () => {
  assert.throws(() => base58ToHex("0OIl"));
});
